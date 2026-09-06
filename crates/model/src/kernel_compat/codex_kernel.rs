//! Simple-owned Codex compatibility layer.
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use process_wrap::tokio::ChildWrapper;
use serde_json::Value;
use thiserror::Error;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::timeout;

use crate::ResponsesGatewayConfig;
use crate::ResponsesGatewayError;
use crate::ResponsesGatewayHandle;

const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// Simple-owned launch configuration for the embedded slim Codex app-server.
///
/// Provider credentials belong only to `gateway`; the child receives a
/// loopback URL, a short-lived gateway token, and a model alias.
#[derive(Debug)]
pub struct CodexKernelConfig {
    /// A frozen, verified version package; None preserves the legacy launch path.
    pub package: Option<crate::KernelPackage>,
    pub executable: PathBuf,
    pub codex_home: PathBuf,
    pub gateway: ResponsesGatewayConfig,
    pub arguments: Vec<OsString>,
    pub handshake_timeout: Duration,
    pub project_memory: Option<crate::CodexProjectMemory>,
}

impl CodexKernelConfig {
    pub fn new(
        executable: impl Into<PathBuf>,
        codex_home: impl Into<PathBuf>,
        gateway: ResponsesGatewayConfig,
    ) -> Self {
        Self {
            package: None,
            executable: executable.into(),
            codex_home: codex_home.into(),
            gateway,
            arguments: Vec::new(),
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            project_memory: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum CodexKernelError {
    #[error(transparent)]
    Package(#[from] crate::KernelPackageError),
    #[error("内核进程退出尚未确认")]
    Termination(#[source] std::io::Error),
    #[error("无法确认项目记忆路径")]
    InvalidMemoryPath(#[source] std::io::Error),
    #[error("项目记忆必须绑定有效项目与独立的绝对存储路径")]
    InvalidMemoryScope,
    #[error("无法启动 Simple 本地模型网关")]
    Gateway(#[from] ResponsesGatewayError),
    #[error("无法准备 Codex 本地数据目录")]
    CreateHome(#[source] std::io::Error),
    #[error("无法准备 Codex 本地功能配置")]
    PrepareConfig(#[source] std::io::Error),
    #[error("无法启动内嵌 Codex 内核：{0}")]
    Spawn(#[source] std::io::Error),
    #[error("内嵌 Codex 内核缺少标准输入或输出管道")]
    MissingPipe,
    #[error("本地内核缺少配套的文件修改工具，请重新同步或安装完整的 Simple 运行组件")]
    MissingPatchTool,
    #[error("无法向内嵌 Codex 内核发送消息")]
    Write(#[source] std::io::Error),
    #[error("无法读取内嵌 Codex 内核消息")]
    Read(#[source] std::io::Error),
    #[error("内嵌 Codex 内核在响应前退出")]
    UnexpectedExit,
    #[error("内嵌 Codex 内核返回了无效 JSON")]
    InvalidMessage(#[source] serde_json::Error),
    #[error("内嵌 Codex 内核返回了无效 JSON-RPC 消息：{0}")]
    InvalidWireMessage(&'static str),
    #[error("等待内嵌 Codex 内核响应超时")]
    Timeout,
    #[error("内嵌 Codex 内核拒绝请求：{0}")]
    Rpc(Value),
    #[error("内嵌 Codex 内核通信任务已经结束")]
    Unavailable,
}

/// One classified JSON-RPC message received from the slim app-server.
///
/// App-server initiated requests deliberately remain distinct from responses:
/// both peers allocate numeric ids independently, so comparing only `id` can
/// mistake an approval callback for the response to a Simple request.
#[derive(Debug, Clone, PartialEq)]
pub enum CodexKernelWireMessage {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Option<Value>,
        error: Option<Value>,
    },
}

impl CodexKernelWireMessage {
    pub fn classify(message: &Value) -> Result<Self, CodexKernelError> {
        let object = message
            .as_object()
            .ok_or(CodexKernelError::InvalidWireMessage("顶层必须是对象"))?;
        let id = object.get("id").cloned();
        let method = object.get("method");

        if let Some(method) = method {
            let method = method
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or(CodexKernelError::InvalidWireMessage(
                    "method 必须是非空字符串",
                ))?
                .to_owned();
            let params = object.get("params").cloned().unwrap_or(Value::Null);
            return match id {
                Some(id) => {
                    validate_rpc_id(&id)?;
                    Ok(Self::Request { id, method, params })
                }
                None => Ok(Self::Notification { method, params }),
            };
        }

        let id = id.ok_or(CodexKernelError::InvalidWireMessage("响应缺少 id"))?;
        validate_rpc_id(&id)?;
        let result = object.get("result").cloned();
        let error = object.get("error").cloned();
        if result.is_some() == error.is_some() {
            return Err(CodexKernelError::InvalidWireMessage(
                "响应必须只包含 result 或 error",
            ));
        }
        Ok(Self::Response { id, result, error })
    }
}

fn validate_rpc_id(id: &Value) -> Result<(), CodexKernelError> {
    if id.is_number() || id.is_string() {
        Ok(())
    } else {
        Err(CodexKernelError::InvalidWireMessage(
            "id 必须是数字或字符串",
        ))
    }
}

/// Low-level owner of one initialized app-server process and its matching
/// ephemeral gateway. Desktop code should use [`CodexKernelClient`] so reads,
/// requests, interrupts and approval responses share one full-duplex actor.
pub struct CodexKernelProcess {
    adapter: super::KernelAdapter,
    child: Box<dyn ChildWrapper>,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    pending_messages: VecDeque<Value>,
    next_request_id: u64,
    gateway: Option<ResponsesGatewayHandle>,
}

impl CodexKernelProcess {
    pub async fn start(config: CodexKernelConfig) -> Result<Self, CodexKernelError> {
        let adapter = config
            .package
            .as_ref()
            .map_or(super::KernelAdapter::SlimV1, |package| package.adapter());
        if let Some(package) = &config.package {
            package.verify()?;
            if config.executable != package.executable() {
                return Err(
                    crate::KernelPackageError::Incompatible("启动路径与版本包不一致").into(),
                );
            }
        }
        #[cfg(windows)]
        if config.executable.is_file()
            && !config
                .executable
                .with_file_name("apply_patch.exe")
                .is_file()
        {
            return Err(CodexKernelError::MissingPatchTool);
        }
        std::fs::create_dir_all(&config.codex_home).map_err(CodexKernelError::CreateHome)?;
        adapter.prepare_home(&config.codex_home)?;
        let gateway = ResponsesGatewayHandle::start(config.gateway).await?;

        let mut standard_command = std::process::Command::new(&config.executable);
        adapter.configure(
            &mut standard_command,
            &config.arguments,
            config.project_memory.as_ref(),
            &gateway,
            &config.codex_home,
        )?;
        standard_command
            .current_dir(&config.codex_home)
            .env("CODEX_HOME", &config.codex_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child =
            super::codex_process::spawn(standard_command).map_err(CodexKernelError::Spawn)?;
        let stdin = child.stdin().take().ok_or(CodexKernelError::MissingPipe)?;
        let stdout = child.stdout().take().ok_or(CodexKernelError::MissingPipe)?;
        let mut process = Self {
            adapter,
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            pending_messages: VecDeque::new(),
            next_request_id: 0,
            gateway: Some(gateway),
        };

        let initialize = process.request_with_timeout(
            "initialize",
            serde_json::json!({
                "clientInfo": {
                    "name": "simple-desktop",
                    "title": "Simple",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true,
                    "optOutNotificationMethods": null
                }
            }),
            config.handshake_timeout,
        );
        initialize.await?;
        process.notify("initialized", None).await?;
        if let Some(settings) = adapter.feature_configuration() {
            process.request("config/batchWrite", settings).await?;
        }
        Ok(process)
    }

    pub async fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, CodexKernelError> {
        self.request_with_timeout(method, params, DEFAULT_HANDSHAKE_TIMEOUT)
            .await
    }

    pub async fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        wait: Duration,
    ) -> Result<Value, CodexKernelError> {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let wire_request_id = Value::String(format!("simple:{request_id}"));
        self.write_message(&serde_json::json!({
            "id": wire_request_id.clone(),
            "method": method,
            "params": params,
        }))
        .await?;

        timeout(wait, async {
            loop {
                let message = self.read_wire_message().await?;
                match CodexKernelWireMessage::classify(&message)? {
                    CodexKernelWireMessage::Response { id, result, error }
                        if id == wire_request_id =>
                    {
                        if let Some(error) = error {
                            return Err(CodexKernelError::Rpc(error));
                        }
                        return Ok(result.unwrap_or(Value::Null));
                    }
                    _ => {}
                }
                self.pending_messages.push_back(message);
            }
        })
        .await
        .map_err(|_| CodexKernelError::Timeout)?
    }

    pub async fn notify(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), CodexKernelError> {
        let mut message = serde_json::json!({"method": method});
        if let Some(params) = params {
            message["params"] = params;
        }
        self.write_message(&message).await
    }

    /// Responds to a request initiated by app-server, such as an approval.
    pub async fn respond(&mut self, id: Value, result: Value) -> Result<(), CodexKernelError> {
        validate_rpc_id(&id)?;
        self.write_message(&serde_json::json!({
            "id": id,
            "result": result,
        }))
        .await
    }

    /// Responds to an app-server request with a JSON-RPC error object.
    pub async fn respond_error(&mut self, id: Value, error: Value) -> Result<(), CodexKernelError> {
        validate_rpc_id(&id)?;
        self.write_message(&serde_json::json!({
            "id": id,
            "error": error,
        }))
        .await
    }

    pub async fn next_message(&mut self) -> Result<Value, CodexKernelError> {
        if let Some(message) = self.pending_messages.pop_front() {
            return Ok(message);
        }
        self.read_wire_message().await
    }

    pub async fn next_wire_message(&mut self) -> Result<CodexKernelWireMessage, CodexKernelError> {
        let message = self.next_message().await?;
        CodexKernelWireMessage::classify(&message)
    }

    pub async fn shutdown(mut self) -> Result<(), CodexKernelError> {
        self.stdin.take();
        let termination = super::codex_process::terminate(&mut self.child).await;
        // Stop the local gateway even if native termination cannot be confirmed.
        let gateway_shutdown = if let Some(gateway) = self.gateway.take() {
            gateway.shutdown().await.map_err(CodexKernelError::from)
        } else {
            Ok(())
        };
        termination.map_err(CodexKernelError::Termination)?;
        gateway_shutdown?;
        Ok(())
    }

    async fn write_message(&mut self, value: &Value) -> Result<(), CodexKernelError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or(CodexKernelError::UnexpectedExit)?;
        let mut bytes = serde_json::to_vec(value).map_err(CodexKernelError::InvalidMessage)?;
        bytes.push(b'\n');
        stdin
            .write_all(&bytes)
            .await
            .map_err(CodexKernelError::Write)?;
        stdin.flush().await.map_err(CodexKernelError::Write)
    }

    async fn read_wire_message(&mut self) -> Result<Value, CodexKernelError> {
        let mut line = Vec::new();
        let bytes = self
            .stdout
            .read_until(b'\n', &mut line)
            .await
            .map_err(CodexKernelError::Read)?;
        if bytes == 0 {
            return Err(CodexKernelError::UnexpectedExit);
        }
        serde_json::from_slice(&line).map_err(CodexKernelError::InvalidMessage)
    }
}

impl Drop for CodexKernelProcess {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.start_kill();
    }
}

enum CodexKernelCommand {
    RegisterModel {
        config: ResponsesGatewayConfig,
        reply: oneshot::Sender<Result<(), CodexKernelError>>,
    },
    Request {
        method: String,
        params: Value,
        wait: Duration,
        reply: oneshot::Sender<Result<Value, CodexKernelError>>,
    },
    Notify {
        method: String,
        params: Option<Value>,
        reply: oneshot::Sender<Result<(), CodexKernelError>>,
    },
    Respond {
        id: Value,
        result: Value,
        reply: oneshot::Sender<Result<(), CodexKernelError>>,
    },
    RespondError {
        id: Value,
        error: Value,
        reply: oneshot::Sender<Result<(), CodexKernelError>>,
    },
    Shutdown {
        reply: oneshot::Sender<Result<(), CodexKernelError>>,
    },
}

/// Cloneable command handle for the process-owning kernel task.
///
/// The actor is the only owner of stdio. This lets the desktop listen for
/// streamed notifications while independently sending interrupt and approval
/// responses, without two readers racing on the same pipe.
#[derive(Clone)]
pub struct CodexKernelClient {
    commands: mpsc::Sender<CodexKernelCommand>,
    instance_id: String,
}

/// Single-consumer stream of notifications and app-server initiated requests.
pub struct CodexKernelEvents {
    messages: mpsc::UnboundedReceiver<Result<CodexKernelWireMessage, CodexKernelError>>,
}

impl CodexKernelEvents {
    pub async fn next(&mut self) -> Option<Result<CodexKernelWireMessage, CodexKernelError>> {
        self.messages.recv().await
    }
}

impl CodexKernelClient {
    /// Add a revision without replacing the process that owns live histories.
    pub async fn register_model(
        &self,
        config: ResponsesGatewayConfig,
    ) -> Result<(), CodexKernelError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CodexKernelCommand::RegisterModel { config, reply })
            .await
            .map_err(|_| CodexKernelError::Unavailable)?;
        response.await.map_err(|_| CodexKernelError::Unavailable)?
    }

    fn from_commands(commands: mpsc::Sender<CodexKernelCommand>) -> Self {
        Self {
            commands,
            instance_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Identifies a single actor/process lifetime, not a model configuration.
    /// A restarted kernel must never inherit another process's live operations.
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
    /// A closed control channel means checks cannot continue. It is not proof
    /// that every external descendant process has exited.
    pub fn control_is_closed(&self) -> bool {
        self.commands.is_closed()
    }
    pub async fn start(
        config: CodexKernelConfig,
    ) -> Result<(Self, CodexKernelEvents), CodexKernelError> {
        let process = CodexKernelProcess::start(config).await?;
        let (command_tx, command_rx) = mpsc::channel(64);
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_kernel_actor(process, command_rx, event_tx));
        Ok((
            Self::from_commands(command_tx),
            CodexKernelEvents { messages: event_rx },
        ))
    }

    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> Result<Value, CodexKernelError> {
        self.request_with_timeout(method, params, DEFAULT_HANDSHAKE_TIMEOUT)
            .await
    }

    pub async fn request_with_timeout(
        &self,
        method: impl Into<String>,
        params: Value,
        wait: Duration,
    ) -> Result<Value, CodexKernelError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CodexKernelCommand::Request {
                method: method.into(),
                params,
                wait,
                reply,
            })
            .await
            .map_err(|_| CodexKernelError::Unavailable)?;
        response.await.map_err(|_| CodexKernelError::Unavailable)?
    }

    pub async fn notify(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<(), CodexKernelError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CodexKernelCommand::Notify {
                method: method.into(),
                params,
                reply,
            })
            .await
            .map_err(|_| CodexKernelError::Unavailable)?;
        response.await.map_err(|_| CodexKernelError::Unavailable)?
    }

    pub async fn respond(&self, id: Value, result: Value) -> Result<(), CodexKernelError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CodexKernelCommand::Respond { id, result, reply })
            .await
            .map_err(|_| CodexKernelError::Unavailable)?;
        response.await.map_err(|_| CodexKernelError::Unavailable)?
    }

    pub async fn respond_error(&self, id: Value, error: Value) -> Result<(), CodexKernelError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CodexKernelCommand::RespondError { id, error, reply })
            .await
            .map_err(|_| CodexKernelError::Unavailable)?;
        response.await.map_err(|_| CodexKernelError::Unavailable)?
    }

    pub async fn shutdown(&self) -> Result<(), CodexKernelError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CodexKernelCommand::Shutdown { reply })
            .await
            .map_err(|_| CodexKernelError::Unavailable)?;
        response.await.map_err(|_| CodexKernelError::Unavailable)?
    }
}

async fn run_kernel_actor(
    mut process: CodexKernelProcess,
    mut commands: mpsc::Receiver<CodexKernelCommand>,
    events: mpsc::UnboundedSender<Result<CodexKernelWireMessage, CodexKernelError>>,
) {
    loop {
        tokio::select! {
            message = process.next_wire_message() => {
                match message {
                    Ok(message) => {
                        let _ = events.send(Ok(message));
                    }
                    Err(error) => {
                        let _ = events.send(Err(error));
                        let _ = process.shutdown().await;
                        return;
                    }
                }
            }
            command = commands.recv() => {
                let Some(command) = command else {
                    let _ = process.shutdown().await;
                    return;
                };
                match command {
                    CodexKernelCommand::RegisterModel { config, reply } => {
                        let result = match process.gateway.as_ref() {
                            Some(gateway) => process.adapter.validate_model_alias(gateway.model_alias(), &config.codex_model_alias)
                                .and_then(|()| gateway.register_model(config).map_err(CodexKernelError::from)),
                            None => Err(CodexKernelError::Unavailable),
                        };
                        let _ = reply.send(result);
                    }
                    CodexKernelCommand::Request { method, params, wait, reply } => {
                        let _ = reply.send(process.request_with_timeout(&method, params, wait).await);
                    }
                    CodexKernelCommand::Notify { method, params, reply } => {
                        let _ = reply.send(process.notify(&method, params).await);
                    }
                    CodexKernelCommand::Respond { id, result, reply } => {
                        let _ = reply.send(process.respond(id, result).await);
                    }
                    CodexKernelCommand::RespondError { id, error, reply } => {
                        let _ = reply.send(process.respond_error(id, error).await);
                    }
                    CodexKernelCommand::Shutdown { reply } => {
                        let result = process.shutdown().await;
                        let _ = reply.send(result);
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Bytes;
    use axum::http::StatusCode;
    use axum::http::header::CONTENT_TYPE;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use tokio::net::TcpListener;

    use super::*;
    use crate::ApiKey;
    use crate::ChatDialect;
    use crate::CodexKernelEvent;

    #[test]
    fn process_identity_survives_clone_but_not_restart() {
        let (first_commands, _first_receiver) = mpsc::channel(1);
        let (next_commands, _next_receiver) = mpsc::channel(1);
        let first = CodexKernelClient::from_commands(first_commands);
        let clone = first.clone();
        let restarted = CodexKernelClient::from_commands(next_commands);
        assert_eq!(first.instance_id(), clone.instance_id());
        assert_ne!(first.instance_id(), restarted.instance_id());
    }

    #[test]
    fn classifies_colliding_server_request_as_request_not_response() {
        let message = serde_json::json!({
            "id": 0,
            "method": "item/commandExecution/requestApproval",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "itemId": "item-1"
            }
        });

        let classified = CodexKernelWireMessage::classify(&message).expect("valid request");
        assert!(matches!(
            classified,
            CodexKernelWireMessage::Request { id, method, .. }
                if id == serde_json::json!(0)
                    && method == "item/commandExecution/requestApproval"
        ));
    }

    #[test]
    fn classifies_response_only_when_method_is_absent() {
        let message = serde_json::json!({
            "id": 0,
            "result": { "turn": { "id": "turn-1" } }
        });

        let classified = CodexKernelWireMessage::classify(&message).expect("valid response");
        assert!(matches!(
            classified,
            CodexKernelWireMessage::Response { id, result: Some(_), error: None }
                if id == serde_json::json!(0)
        ));
    }

    #[test]
    fn rejects_ambiguous_response_shape() {
        let message = serde_json::json!({
            "id": 0,
            "result": {},
            "error": { "code": -32603 }
        });

        assert!(matches!(
            CodexKernelWireMessage::classify(&message),
            Err(CodexKernelError::InvalidWireMessage(_))
        ));
    }

    #[tokio::test]
    async fn missing_kernel_binary_fails_without_exposing_provider_secret() {
        let home = std::env::temp_dir().join(format!("simple-codex-test-{}", uuid::Uuid::new_v4()));
        let config = CodexKernelConfig::new(
            home.join("missing-codex-app-server.exe"),
            &home,
            ResponsesGatewayConfig::new(
                "http://127.0.0.1:1/v1",
                "deepseek-test",
                Some(ApiKey::new("provider-secret").expect("valid test key")),
            ),
        );
        let error = match CodexKernelProcess::start(config).await {
            Ok(process) => {
                process
                    .shutdown()
                    .await
                    .expect("shutdown unexpected process");
                panic!("missing kernel unexpectedly started")
            }
            Err(error) => error,
        };
        let rendered = format!("{error:?}");
        assert!(matches!(error, CodexKernelError::Spawn(_)));
        assert!(!rendered.contains("provider-secret"));
        let _ = std::fs::remove_dir_all(home);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn missing_patch_companion_fails_before_starting_gateway_or_creating_home() {
        let root = std::env::temp_dir().join(format!("simple-patch-gate-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("fixture directory");
        let executable = root.join("codex-app-server.exe");
        std::fs::write(&executable, []).expect("fixture executable");
        let home = root.join("kernel-home");
        let config = CodexKernelConfig::new(
            executable,
            &home,
            ResponsesGatewayConfig::new("http://127.0.0.1:1", "fixture", None),
        );
        assert!(matches!(
            CodexKernelProcess::start(config).await,
            Err(CodexKernelError::MissingPatchTool)
        ));
        assert!(!home.exists());
        std::fs::remove_dir_all(root).expect("remove isolated fixture");
    }

    #[tokio::test]
    async fn initializes_real_slim_kernel_when_test_binary_is_configured() {
        let Some(executable) = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER") else {
            return;
        };
        let home = std::env::temp_dir().join(format!("simple-codex-test-{}", uuid::Uuid::new_v4()));
        let config = CodexKernelConfig::new(
            executable,
            &home,
            ResponsesGatewayConfig::new("http://127.0.0.1:1/v1", "deepseek-test", None),
        );
        let (client, _events) = CodexKernelClient::start(config)
            .await
            .expect("initialize real slim kernel");
        client.shutdown().await.expect("shutdown slim kernel");
        let _ = std::fs::remove_dir_all(home);
    }

    async fn deepseek_turn_upstream(body: Bytes) -> impl IntoResponse {
        let request: Value = serde_json::from_slice(&body).expect("valid translated request");
        assert_eq!(request["model"], "deepseek-test");
        assert_eq!(request["stream"], true);
        assert!(
            request["messages"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        (
            StatusCode::OK,
            [(CONTENT_TYPE, "text/event-stream")],
            concat!(
                "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"reasoning_content\":\"local reasoning\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"SIMPLE_KERNEL_OK\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":3}}\n\n",
                "data: [DONE]\n\n"
            ),
        )
    }

    #[tokio::test]
    async fn completes_real_codex_turn_through_deepseek_gateway_when_configured() {
        let Some(executable) = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER") else {
            return;
        };
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind fake DeepSeek upstream");
        let port = listener.local_addr().expect("fake upstream address").port();
        let upstream = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/v1/chat/completions", post(deepseek_turn_upstream)),
            )
            .await
        });
        let home = std::env::temp_dir().join(format!("simple-codex-test-{}", uuid::Uuid::new_v4()));
        let gateway = ResponsesGatewayConfig::new(
            format!("http://127.0.0.1:{port}/v1"),
            "deepseek-test",
            None,
        )
        .with_chat_completions(ChatDialect::DeepSeek);
        let (client, mut events) =
            CodexKernelClient::start(CodexKernelConfig::new(executable, &home, gateway))
                .await
                .expect("start real slim kernel actor");

        let thread = client
            .request(
                "thread/start",
                serde_json::json!({
                    "cwd": home.to_string_lossy(),
                    "model": "deepseek-test",
                    "approvalPolicy": "never",
                    "sandbox": "read-only",
                    "config": {
                        "web_search": "disabled",
                        "model_reasoning_effort": "high"
                    }
                }),
            )
            .await
            .expect("start Codex thread");
        let thread_id = thread["thread"]["id"]
            .as_str()
            .expect("thread id")
            .to_owned();
        let features = crate::CodexFeatureBridge::new(client.clone());
        features
            .list_skills(&home)
            .await
            .expect("list local skills through real app-server");
        features
            .set_thread_memory(&thread_id, false)
            .await
            .expect("disable thread memory through real app-server");
        features
            .set_thread_memory(&thread_id, true)
            .await
            .expect("enable thread memory through real app-server");
        features
            .reset_memory()
            .await
            .expect("reset local memory through real app-server");
        features
            .list_mcp_servers(Some(&thread_id))
            .await
            .expect("list local MCP servers through real app-server");
        let started_turn = client
            .request(
                "turn/start",
                serde_json::json!({
                    "threadId": thread_id,
                    "input": [{
                        "type": "text",
                        "text": "Reply with the requested marker.",
                        "text_elements": []
                    }]
                }),
            )
            .await
            .expect("start Codex turn");
        let turn_id = started_turn["turn"]["id"]
            .as_str()
            .expect("turn id")
            .to_owned();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        let mut observed = Vec::new();
        let mut completed_assistant_text = None;
        let assistant_text = loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let message = timeout(remaining, events.next())
                .await
                .unwrap_or_else(|_| panic!("Codex turn timed out after events: {observed:#?}"))
                .expect("kernel event stream remains open")
                .expect("read Codex event");
            let event = CodexKernelEvent::project(message).expect("project Codex event");
            match &event {
                CodexKernelEvent::ItemCompleted { item, .. } if item.kind == "agentMessage" => {
                    completed_assistant_text =
                        Some(item.agent_text().unwrap_or_default().to_owned());
                }
                CodexKernelEvent::TurnCompleted { status, .. } => {
                    assert_eq!(
                        *status,
                        crate::CodexTurnStatus::Completed,
                        "events before terminal: {observed:#?}; terminal: {event:#?}"
                    );
                    break completed_assistant_text.unwrap_or_else(|| {
                        panic!("Codex turn completed without an assistant item: {observed:#?}")
                    });
                }
                _ => {}
            }
            observed.push(event);
        };
        assert_eq!(assistant_text, "SIMPLE_KERNEL_OK");

        let forked = client
            .request(
                "thread/fork",
                serde_json::json!({
                    "threadId": thread_id,
                    "lastTurnId": turn_id,
                    "excludeTurns": true
                }),
            )
            .await
            .expect("fork completed Codex turn");
        let forked_thread_id = forked["thread"]["id"].as_str().expect("forked thread id");
        assert_ne!(forked_thread_id, thread_id);

        client
            .request(
                "thread/revert",
                serde_json::json!({
                    "threadId": thread_id,
                    "beforeTurnId": turn_id
                }),
            )
            .await
            .expect("revert original Codex thread");
        let turns = client
            .request(
                "thread/turns/list",
                serde_json::json!({
                    "threadId": thread_id,
                    "cursor": null,
                    "limit": 20,
                    "sortDirection": "asc",
                    "itemsView": "notLoaded"
                }),
            )
            .await
            .expect("read reverted Codex history");
        assert_eq!(turns["data"], serde_json::json!([]));

        client.shutdown().await.expect("shutdown slim kernel");
        upstream.abort();
        let _ = std::fs::remove_dir_all(home);
    }
}
