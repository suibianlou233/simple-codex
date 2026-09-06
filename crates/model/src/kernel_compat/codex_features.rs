//! Simple-owned Codex compatibility layer.
use std::path::Path;

use serde_json::{Value, json};
use thiserror::Error;

use crate::{CodexKernelError, CodexRpc};

const MAX_MCP_NAME_CHARS: usize = 64;
const MAX_MCP_ARGUMENTS: usize = 128;
const MAX_ENVIRONMENT_VARIABLES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalMcpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    /// Names inherited from the user's process environment. Values are never
    /// accepted here, so credentials cannot be copied into config.toml.
    pub environment_variables: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Error)]
pub enum CodexFeatureError {
    #[error(transparent)]
    Kernel(#[from] CodexKernelError),
    #[error("Codex 功能响应无效：{0}")]
    InvalidResponse(&'static str),
    #[error("本地 MCP 名称只能包含字母、数字、短横线和下划线，且长度为 1–64")]
    InvalidMcpName,
    #[error("本地 MCP 启动命令不能为空")]
    EmptyMcpCommand,
    #[error("本地 MCP 参数数量超过安全上限")]
    TooManyMcpArguments,
    #[error("本地 MCP 环境变量数量超过安全上限")]
    TooManyEnvironmentVariables,
    #[error("本地 MCP 环境变量名称无效")]
    InvalidEnvironmentVariable,
    #[error("本地 MCP 工作目录必须是绝对路径")]
    RelativeMcpWorkingDirectory,
}

pub struct CodexFeatureBridge<R> {
    rpc: R,
}

impl<R> CodexFeatureBridge<R>
where
    R: CodexRpc,
{
    pub fn new(rpc: R) -> Self {
        Self { rpc }
    }

    pub async fn list_skills(&self, cwd: &Path) -> Result<Vec<Value>, CodexFeatureError> {
        let response = self
            .rpc
            .request(
                "skills/list",
                json!({ "cwds": [cwd], "forceReload": false }),
            )
            .await?;
        response
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .ok_or(CodexFeatureError::InvalidResponse("skills/list 缺少 data"))
    }

    pub async fn set_thread_memory(
        &self,
        thread_id: &str,
        enabled: bool,
    ) -> Result<(), CodexFeatureError> {
        require_non_empty(thread_id, "thread id 不能为空")?;
        // The pinned Simple kernel applies this to generation AND future memory
        // tool/context contributions. Existing conversation history is retained.
        let thread = self
            .rpc
            .request("thread/read", json!({"threadId":thread_id}))
            .await?;
        if thread["thread"]["status"]["type"] == "active" {
            return Err(CodexFeatureError::InvalidResponse(
                "请先停止当前任务，再修改记忆设置",
            ));
        }
        self.rpc
            .request(
                "thread/memoryMode/set",
                json!({
                    "threadId": thread_id,
                    "mode": if enabled { "enabled" } else { "disabled" }
                }),
            )
            .await?;
        Ok(())
    }

    pub async fn reset_memory(&self) -> Result<(), CodexFeatureError> {
        self.rpc.request("memory/reset", Value::Null).await?;
        Ok(())
    }

    pub async fn forget_previous_memory_sources(&self) -> Result<i64, CodexFeatureError> {
        let response = self.rpc.request("memory/forget", Value::Null).await?;
        response
            .get("excludedBeforeAt")
            .and_then(Value::as_i64)
            .filter(|cutoff| *cutoff >= 0)
            .ok_or(CodexFeatureError::InvalidResponse(
                "memory/forget 缺少来源截止时间",
            ))
    }

    pub async fn list_mcp_servers(
        &self,
        thread_id: Option<&str>,
    ) -> Result<Vec<Value>, CodexFeatureError> {
        let response = self
            .rpc
            .request(
                "mcpServerStatus/list",
                json!({
                    "cursor": null,
                    "limit": 200,
                    "detail": "toolsAndAuthOnly",
                    "threadId": thread_id
                }),
            )
            .await?;
        let data = response
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .ok_or(CodexFeatureError::InvalidResponse(
                "mcpServerStatus/list 缺少 data",
            ))?;
        if response
            .get("nextCursor")
            .is_some_and(|value| !value.is_null())
        {
            return Err(CodexFeatureError::InvalidResponse(
                "MCP 服务数量超过当前本地上限",
            ));
        }
        Ok(data)
    }

    pub async fn save_local_mcp_server(
        &self,
        config: &LocalMcpServerConfig,
    ) -> Result<(), CodexFeatureError> {
        validate_local_mcp_server(config)?;
        self.rpc
            .request(
                "config/value/write",
                json!({
                    "keyPath": format!("mcp_servers.{}", config.name),
                    "value": {
                        "command": config.command,
                        "args": config.args,
                        "cwd": config.cwd,
                        "env_vars": config.environment_variables,
                        "enabled": config.enabled
                    },
                    "mergeStrategy": "replace",
                    "filePath": null,
                    "expectedVersion": null
                }),
            )
            .await?;
        self.rpc
            .request("config/mcpServer/reload", Value::Null)
            .await?;
        Ok(())
    }

    pub async fn remove_local_mcp_server(&self, name: &str) -> Result<(), CodexFeatureError> {
        validate_mcp_name(name)?;
        self.rpc
            .request(
                "config/value/write",
                json!({
                    "keyPath": format!("mcp_servers.{name}"),
                    "value": null,
                    "mergeStrategy": "replace",
                    "filePath": null,
                    "expectedVersion": null
                }),
            )
            .await?;
        self.rpc
            .request("config/mcpServer/reload", Value::Null)
            .await?;
        Ok(())
    }
}

fn validate_local_mcp_server(config: &LocalMcpServerConfig) -> Result<(), CodexFeatureError> {
    validate_mcp_name(&config.name)?;
    if config.command.trim().is_empty() {
        return Err(CodexFeatureError::EmptyMcpCommand);
    }
    if config.args.len() > MAX_MCP_ARGUMENTS {
        return Err(CodexFeatureError::TooManyMcpArguments);
    }
    if config.environment_variables.len() > MAX_ENVIRONMENT_VARIABLES {
        return Err(CodexFeatureError::TooManyEnvironmentVariables);
    }
    if config
        .environment_variables
        .iter()
        .any(|name| !valid_environment_variable(name))
    {
        return Err(CodexFeatureError::InvalidEnvironmentVariable);
    }
    if config
        .cwd
        .as_deref()
        .is_some_and(|cwd| !Path::new(cwd).is_absolute())
    {
        return Err(CodexFeatureError::RelativeMcpWorkingDirectory);
    }
    Ok(())
}

fn validate_mcp_name(name: &str) -> Result<(), CodexFeatureError> {
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_MCP_NAME_CHARS
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(CodexFeatureError::InvalidMcpName)
    }
}

fn valid_environment_variable(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn require_non_empty(value: &str, message: &'static str) -> Result<(), CodexFeatureError> {
    if value.is_empty() {
        Err(CodexFeatureError::InvalidResponse(message))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::*;

    #[derive(Clone)]
    struct FakeRpc {
        responses: Arc<Mutex<VecDeque<Value>>>,
        calls: Arc<Mutex<Vec<(String, Value)>>>,
    }

    impl FakeRpc {
        fn new(responses: Vec<Value>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into())),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl CodexRpc for FakeRpc {
        async fn request(&self, method: &str, params: Value) -> Result<Value, CodexKernelError> {
            self.calls
                .lock()
                .map_err(|_| CodexKernelError::Unavailable)?
                .push((method.to_owned(), params));
            self.responses
                .lock()
                .map_err(|_| CodexKernelError::Unavailable)?
                .pop_front()
                .ok_or(CodexKernelError::Unavailable)
        }
    }

    #[tokio::test]
    async fn source_forgetting_requires_a_native_cutoff_and_never_falls_back_to_reset() {
        let rpc = FakeRpc::new(vec![json!({"excludedBeforeAt": 1788600000})]);
        assert_eq!(
            CodexFeatureBridge::new(rpc.clone())
                .forget_previous_memory_sources()
                .await
                .expect("cutoff"),
            1788600000
        );
        assert_eq!(
            *rpc.calls.lock().expect("calls"),
            vec![("memory/forget".into(), Value::Null)]
        );
        for response in [
            json!({}),
            json!({"excludedBeforeAt": "1788600000"}),
            json!({"excludedBeforeAt": -1}),
        ] {
            let rpc = FakeRpc::new(vec![response]);
            assert!(
                CodexFeatureBridge::new(rpc.clone())
                    .forget_previous_memory_sources()
                    .await
                    .is_err()
            );
            assert_eq!(
                *rpc.calls.lock().expect("calls"),
                vec![("memory/forget".into(), Value::Null)]
            );
        }
    }

    fn local_server() -> LocalMcpServerConfig {
        LocalMcpServerConfig {
            name: "docs-local".to_owned(),
            command: "node".to_owned(),
            args: vec!["server.js".to_owned()],
            cwd: Some("C:\\workspace".to_owned()),
            environment_variables: vec!["DOCS_TOKEN".to_owned()],
            enabled: true,
        }
    }

    #[tokio::test]
    async fn saves_only_stdio_configuration_and_inherited_secret_names()
    -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![json!({"status": "ok"}), json!({})]);
        CodexFeatureBridge::new(rpc.clone())
            .save_local_mcp_server(&local_server())
            .await?;
        let calls = rpc.calls.lock().map_err(|_| "test call lock poisoned")?;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "config/value/write");
        assert_eq!(calls[0].1["keyPath"], "mcp_servers.docs-local");
        assert_eq!(calls[0].1["value"]["env_vars"], json!(["DOCS_TOKEN"]));
        assert!(calls[0].1["value"].get("env").is_none());
        assert_eq!(calls[1].0, "config/mcpServer/reload");
        Ok(())
    }

    #[tokio::test]
    async fn rejects_path_injection_in_server_name() -> Result<(), Box<dyn std::error::Error>> {
        let mut config = local_server();
        config.name = "docs.other".to_owned();
        let rpc = FakeRpc::new(Vec::new());
        let error = CodexFeatureBridge::new(rpc)
            .save_local_mcp_server(&config)
            .await
            .err()
            .ok_or("expected invalid name")?;
        assert!(matches!(error, CodexFeatureError::InvalidMcpName));
        Ok(())
    }

    #[tokio::test]
    async fn memory_mode_cannot_change_during_an_active_turn()
    -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![json!({"thread":{"status":{"type":"active"}}})]);
        assert!(
            CodexFeatureBridge::new(rpc.clone())
                .set_thread_memory("thread-1", false)
                .await
                .is_err()
        );
        let calls = rpc.calls.lock().map_err(|_| "test call lock poisoned")?;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "thread/read");
        Ok(())
    }

    #[tokio::test]
    async fn memory_mode_is_thread_scoped() -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![
            json!({"thread":{"status":{"type":"idle"}}}),
            json!({}),
        ]);
        CodexFeatureBridge::new(rpc.clone())
            .set_thread_memory("thread-1", false)
            .await?;
        let calls = rpc.calls.lock().map_err(|_| "test call lock poisoned")?;
        assert_eq!(calls[1].0, "thread/memoryMode/set");
        assert_eq!(
            calls[1].1,
            json!({"threadId": "thread-1", "mode": "disabled"})
        );
        Ok(())
    }
}
