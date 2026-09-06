use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

#[cfg(windows)]
use process_wrap::tokio::JobObject;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::sync::OnceLock;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::{Workspace, WorkspaceError};
use local_agent_sandbox::{
    SandboxHealth, SandboxRuntime, SandboxRuntimeError, WindowsSandboxError,
};

const MAX_OUTPUT_BYTES: usize = 1_048_576;
const MAX_TIMEOUT_MS: u64 = 120_000;
const TERMINATION_WAIT: Duration = Duration::from_secs(5);
const UTF8_RUNTIME_ENVIRONMENT: [(&str, &str); 4] = [
    ("PYTHONUTF8", "1"),
    ("PYTHONIOENCODING", "utf-8"),
    ("LANG", "C.UTF-8"),
    ("LC_ALL", "C.UTF-8"),
];

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error("程序名不能为空")]
    EmptyProgram,
    #[error("命令超过本地安全限制")]
    InvalidRequest,
    #[error("命令的第 {index} 个参数疑似包含凭据，已拒绝执行")]
    SensitiveArgument { index: usize },
    #[error("命令执行超时")]
    TimedOut,
    #[error("命令已取消")]
    Cancelled,
    #[error("命令进程启动失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("读取命令输出失败")]
    OutputReader,
    #[error("命令工作目录无效；省略 cwd 可使用项目根目录，或者填写当前操作系统中真实存在的目录")]
    InvalidWorkingDirectory,
    #[error(transparent)]
    Sandbox(#[from] SandboxRuntimeError),
    #[error("项目沙箱健康检查声称可用，但安全执行器尚未安装；已拒绝宿主机降级")]
    SandboxRunnerUnavailable,
    #[error(transparent)]
    WindowsSandbox(#[from] WindowsSandboxError),
    #[error("项目沙箱中的命令执行失败：{kind}：{message}")]
    SandboxCommandFailed { kind: String, message: String },
    #[error("无法生成命令权限策略标识")]
    PermissionProfileEncoding(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandErrorKind {
    InvalidRequest,
    SensitiveArgument,
    SandboxDenied,
    SandboxUnavailable,
    SandboxSetupDrift,
    SandboxUnsupportedPlatform,
    SandboxUnsupportedPolicy,
    SandboxRunnerUnavailable,
    SandboxExecutionFailed,
    SpawnFailed,
    TimedOut,
    Cancelled,
    OutputFailed,
    InvalidWorkingDirectory,
    PolicyEncoding,
}

impl CommandError {
    #[must_use]
    pub const fn kind(&self) -> CommandErrorKind {
        match self {
            Self::EmptyProgram | Self::InvalidRequest => CommandErrorKind::InvalidRequest,
            Self::SensitiveArgument { .. } => CommandErrorKind::SensitiveArgument,
            Self::TimedOut => CommandErrorKind::TimedOut,
            Self::Cancelled => CommandErrorKind::Cancelled,
            Self::Io(_) => CommandErrorKind::SpawnFailed,
            Self::OutputReader => CommandErrorKind::OutputFailed,
            Self::InvalidWorkingDirectory | Self::Workspace(_) => {
                CommandErrorKind::InvalidWorkingDirectory
            }
            Self::Sandbox(SandboxRuntimeError::Unavailable { health, .. }) => match health {
                SandboxHealth::NeedsSetup { .. } => CommandErrorKind::SandboxUnavailable,
                SandboxHealth::Drifted { .. } => CommandErrorKind::SandboxSetupDrift,
                SandboxHealth::UnsupportedPlatform => CommandErrorKind::SandboxUnsupportedPlatform,
                SandboxHealth::Ready { .. } => CommandErrorKind::SandboxDenied,
            },
            Self::Sandbox(SandboxRuntimeError::UnsupportedPolicy) => {
                CommandErrorKind::SandboxUnsupportedPolicy
            }
            Self::SandboxRunnerUnavailable => CommandErrorKind::SandboxRunnerUnavailable,
            Self::WindowsSandbox(WindowsSandboxError::UnsupportedPlatform) => {
                CommandErrorKind::SandboxUnsupportedPlatform
            }
            Self::WindowsSandbox(WindowsSandboxError::NeedsSetup) => {
                CommandErrorKind::SandboxUnavailable
            }
            Self::WindowsSandbox(WindowsSandboxError::InvalidSetup) => {
                CommandErrorKind::SandboxSetupDrift
            }
            Self::WindowsSandbox(WindowsSandboxError::MissingHelper(_)) => {
                CommandErrorKind::SandboxRunnerUnavailable
            }
            Self::WindowsSandbox(
                WindowsSandboxError::Spawn(_)
                | WindowsSandboxError::SetupFailed(_)
                | WindowsSandboxError::ExecutionFailed(_)
                | WindowsSandboxError::Protocol(_),
            )
            | Self::SandboxCommandFailed { .. } => CommandErrorKind::SandboxExecutionFailed,
            Self::PermissionProfileEncoding(_) => CommandErrorKind::PolicyEncoding,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    /// The process-lifecycle boundary used for this invocation.
    ///
    /// This is deliberately not named a sandbox: neither variant restricts
    /// filesystem, network, registry, or current-user permissions.
    pub process_boundary: ProcessBoundary,
    /// Authorization boundary used for this command.
    pub execution_boundary: CommandExecutionBoundary,
    /// Stable identity of the exact technical permission profile used.
    pub permission_profile_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessBoundary {
    /// Windows Job Object containment. Cancelling, timing out, or dropping the
    /// managed child terminates processes that remain in the job.
    WindowsJobObject,
    /// Only the directly spawned process is managed by this layer.
    ProcessOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandExecutionBoundary {
    ApprovedHost,
    WorkspaceSandbox,
    SystemFullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandExecutionPolicy {
    /// A user-approved command running with the current user's host access.
    ApprovedHost,
    /// An automatic command that must be isolated to the project directory.
    WorkspaceSandbox,
    /// An automatic command running with the current user's full host access.
    SystemFullAccess,
}

enum Controlled<T> {
    Completed(T),
    Cancelled,
    TimedOut,
}

struct CollectedOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    truncated: bool,
}

pub async fn run_command(
    workspace: &Workspace,
    request: &CommandRequest,
    cancellation: CancellationToken,
) -> Result<CommandOutput, CommandError> {
    run_command_with_policy(
        workspace,
        request,
        cancellation,
        CommandExecutionPolicy::ApprovedHost,
    )
    .await
}

pub async fn run_command_with_policy(
    workspace: &Workspace,
    request: &CommandRequest,
    cancellation: CancellationToken,
    policy: CommandExecutionPolicy,
) -> Result<CommandOutput, CommandError> {
    crate::command_runtime::execute_command(workspace, request, cancellation, policy).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostCwdScope {
    Workspace,
    System,
}

pub(crate) async fn execute_host_command(
    workspace: &Workspace,
    request: &CommandRequest,
    cancellation: CancellationToken,
    cwd_scope: HostCwdScope,
    execution_boundary: CommandExecutionBoundary,
    permission_profile_hash: String,
) -> Result<CommandOutput, CommandError> {
    let cwd = match cwd_scope {
        HostCwdScope::Workspace => workspace
            .resolve_directory(&request.cwd)
            .map_err(|_| CommandError::InvalidWorkingDirectory)?,
        HostCwdScope::System => resolve_system_cwd(workspace, &request.cwd)
            .map_err(|_| CommandError::InvalidWorkingDirectory)?,
    };
    if cancellation.is_cancelled() {
        return Err(CommandError::Cancelled);
    }
    let timeout = Duration::from_millis(request.timeout_ms.clamp(1, MAX_TIMEOUT_MS));
    let deadline = Instant::now() + timeout;
    let program = resolve_command_program(&request.program);
    let mut command = CommandWrap::with_new(&program, |command| {
        command
            .args(&request.args)
            .current_dir(cwd)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in safe_environment_names() {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        for (name, value) in UTF8_RUNTIME_ENVIRONMENT {
            command.env(name, value);
        }
    });
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    command.wrap(JobObject);

    let mut child = command.spawn()?;
    let stdout = child.stdout().take().ok_or(CommandError::OutputReader)?;
    let stderr = child.stderr().take().ok_or(CommandError::OutputReader)?;
    let mut output_task = tokio::spawn(read_outputs(stdout, stderr));

    let status = match await_controlled(child.wait(), &cancellation, deadline).await {
        Controlled::Completed(Ok(status)) => status,
        Controlled::Completed(Err(error)) => {
            abort_output(&mut output_task).await;
            return Err(CommandError::Io(error));
        }
        Controlled::Cancelled => {
            terminate_managed_process(&mut child).await;
            abort_output(&mut output_task).await;
            return Err(CommandError::Cancelled);
        }
        Controlled::TimedOut => {
            terminate_managed_process(&mut child).await;
            abort_output(&mut output_task).await;
            return Err(CommandError::TimedOut);
        }
    };
    let output = match await_controlled(&mut output_task, &cancellation, deadline).await {
        Controlled::Completed(Ok(Ok(output))) => output,
        Controlled::Completed(Ok(Err(error))) => return Err(CommandError::Io(error)),
        Controlled::Completed(Err(_)) => return Err(CommandError::OutputReader),
        Controlled::Cancelled => {
            terminate_managed_process(&mut child).await;
            abort_output(&mut output_task).await;
            return Err(CommandError::Cancelled);
        }
        Controlled::TimedOut => {
            terminate_managed_process(&mut child).await;
            abort_output(&mut output_task).await;
            return Err(CommandError::TimedOut);
        }
    };
    Ok(CommandOutput {
        exit_code: status.code(),
        stdout: decode_command_output(&output.stdout),
        stderr: decode_command_output(&output.stderr),
        truncated: output.truncated,
        process_boundary: process_boundary(),
        execution_boundary,
        permission_profile_hash,
    })
}

#[cfg(not(windows))]
fn resolve_command_program(program: &str) -> PathBuf {
    PathBuf::from(program)
}

#[cfg(windows)]
fn resolve_command_program(program: &str) -> PathBuf {
    let Some(path) = std::env::var_os("PATH") else {
        return PathBuf::from(program);
    };
    let path_ext = std::env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
    resolve_windows_program_from(program, &path, &path_ext)
        .unwrap_or_else(|| PathBuf::from(program))
}

#[cfg(windows)]
fn resolve_windows_program_from(
    program: &str,
    path: &std::ffi::OsStr,
    path_ext: &std::ffi::OsStr,
) -> Option<PathBuf> {
    let requested = Path::new(program);
    if requested.components().count() > 1 || requested.extension().is_some() {
        return None;
    }
    let extensions = path_ext
        .to_string_lossy()
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            if extension.starts_with('.') {
                extension.to_owned()
            } else {
                format!(".{extension}")
            }
        })
        .collect::<Vec<_>>();
    std::env::split_paths(path).find_map(|directory| {
        extensions.iter().find_map(|extension| {
            let candidate = directory.join(format!("{program}{extension}"));
            candidate.is_file().then_some(candidate)
        })
    })
}

#[must_use]
pub fn workspace_sandbox_health() -> SandboxHealth {
    SandboxRuntime::current().health().clone()
}

#[must_use]
pub fn workspace_sandbox_available() -> bool {
    workspace_sandbox_health().is_ready()
}

fn resolve_system_cwd(workspace: &Workspace, value: &str) -> Result<PathBuf, WorkspaceError> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return workspace.resolve_directory(value);
    }
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            WorkspaceError::NotFound
        } else {
            WorkspaceError::Io(error)
        }
    })?;
    if !canonical.is_dir() {
        return Err(WorkspaceError::NotADirectory);
    }
    Ok(canonical)
}

/// Validates limits and rejects arguments that appear to contain credentials.
///
/// The error intentionally reports only the argument position, never the
/// rejected value. Callers should invoke this before persisting a proposed
/// request; [`run_command`] invokes it again immediately before execution.
pub fn validate_command_request(request: &CommandRequest) -> Result<(), CommandError> {
    if request.program.trim().is_empty() {
        return Err(CommandError::EmptyProgram);
    }
    let argument_bytes = request
        .args
        .iter()
        .try_fold(0_usize, |total, argument| total.checked_add(argument.len()))
        .ok_or(CommandError::InvalidRequest)?;
    if request.program.len() > 1_024
        || request.args.len() > 256
        || argument_bytes > 65_536
        || request.cwd.len() > 2_048
    {
        return Err(CommandError::InvalidRequest);
    }
    if let Some(index) = sensitive_argument_index(&request.args) {
        return Err(CommandError::SensitiveArgument { index });
    }
    Ok(())
}

fn sensitive_argument_index(arguments: &[String]) -> Option<usize> {
    let mut pem_begin = None;
    let mut pem_private_key_end = false;
    for (index, argument) in arguments.iter().enumerate() {
        let normalized = argument.trim().to_ascii_lowercase();
        if normalized.contains("-----begin") {
            pem_begin.get_or_insert(index);
        }
        pem_private_key_end |= normalized.contains("private key-----");
        if is_sensitive_flag(&normalized)
            || contains_authorization(&normalized)
            || contains_secret_key_shape(&normalized)
            || (normalized.contains("-----begin") && normalized.contains("private key-----"))
        {
            return Some(index + 1);
        }
    }
    if pem_private_key_end {
        pem_begin.map(|index| index + 1)
    } else {
        None
    }
}

fn is_sensitive_flag(argument: &str) -> bool {
    const FLAG_NAMES: &[&str] = &[
        "--access-token",
        "--api-key",
        "--api_key",
        "--apikey",
        "--auth-token",
        "--authorization",
        "--client-secret",
        "--password",
        "--private-key",
        "--secret",
        "--token",
        "/api-key",
        "/api_key",
        "/apikey",
        "/authorization",
        "/password",
        "/secret",
        "/token",
    ];
    const ASSIGNMENT_NAMES: &[&str] = &[
        "access_token",
        "api-key",
        "api_key",
        "apikey",
        "authorization",
        "password",
        "secret",
        "token",
    ];
    let separator = argument.find(['=', ':']);
    let name = separator.map_or(argument, |position| &argument[..position]);
    FLAG_NAMES.contains(&name) || (argument.contains('=') && ASSIGNMENT_NAMES.contains(&name))
}

fn contains_authorization(argument: &str) -> bool {
    argument.contains("authorization:")
        || argument.starts_with("bearer ")
        || argument.contains(" bearer ")
}

fn contains_secret_key_shape(argument: &str) -> bool {
    let mut search_from = 0;
    while let Some(relative) = argument[search_from..].find("sk-") {
        let start = search_from + relative + 3;
        let payload_length = argument[start..]
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
            .count();
        if payload_length >= 8 {
            return true;
        }
        search_from = start;
    }
    false
}

async fn await_controlled<F, T>(
    future: F,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Controlled<T>
where
    F: Future<Output = T>,
{
    tokio::pin!(future);
    tokio::select! {
        () = cancellation.cancelled() => Controlled::Cancelled,
        () = tokio::time::sleep_until(deadline) => Controlled::TimedOut,
        result = &mut future => Controlled::Completed(result),
    }
}

async fn abort_output(output_task: &mut JoinHandle<Result<CollectedOutput, std::io::Error>>) {
    output_task.abort();
    let _ = output_task.await;
}

async fn terminate_managed_process(child: &mut Box<dyn ChildWrapper>) {
    // JobObject's implementation targets the entire job on Windows. On other
    // platforms this delegates to Tokio and only targets the direct child.
    let _ = tokio::time::timeout(TERMINATION_WAIT, Box::into_pin(child.kill())).await;
}

const fn process_boundary() -> ProcessBoundary {
    if cfg!(windows) {
        ProcessBoundary::WindowsJobObject
    } else {
        ProcessBoundary::ProcessOnly
    }
}

fn safe_environment_names() -> &'static [&'static str] {
    &[
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMFILES",
        "ProgramFiles(x86)",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "LANG",
        "LC_ALL",
    ]
}

fn decode_command_output(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    #[cfg(windows)]
    if let Some(decoded) = decode_windows_legacy_output(bytes, windows_active_code_page()) {
        return decoded;
    }
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(windows)]
fn decode_windows_legacy_output(bytes: &[u8], code_page: Option<u16>) -> Option<String> {
    let encoding = codepage::to_encoding_no_replacement(code_page?)?;
    let (decoded, _, _) = encoding.decode(bytes);
    Some(decoded.into_owned())
}

#[cfg(windows)]
fn windows_active_code_page() -> Option<u16> {
    static ACTIVE_CODE_PAGE: OnceLock<Option<u16>> = OnceLock::new();
    *ACTIVE_CODE_PAGE.get_or_init(detect_windows_active_code_page)
}

#[cfg(windows)]
fn detect_windows_active_code_page() -> Option<u16> {
    let command = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("cmd.exe"))
        .unwrap_or_else(|| PathBuf::from("cmd.exe"));
    let mut process = std::process::Command::new(command);
    process
        .args(["/D", "/C", "chcp"])
        .creation_flags(CREATE_NO_WINDOW);
    let output = process.output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .split(|character: char| !character.is_ascii_digit())
        .filter_map(|value| value.parse::<u16>().ok())
        .next_back()
}

async fn read_limited(
    mut reader: impl AsyncRead + Unpin,
) -> Result<(Vec<u8>, bool), std::io::Error> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8_192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(output.len());
        let kept = read.min(remaining);
        output.extend_from_slice(&buffer[..kept]);
        truncated |= kept < read;
    }
    Ok((output, truncated))
}

async fn read_outputs(
    stdout: impl AsyncRead + Unpin,
    stderr: impl AsyncRead + Unpin,
) -> Result<CollectedOutput, std::io::Error> {
    let (stdout, stderr) = tokio::try_join!(read_limited(stdout), read_limited(stderr))?;
    Ok(CollectedOutput {
        stdout: stdout.0,
        stderr: stderr.0,
        truncated: stdout.1 || stderr.1,
    })
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::process::Command as StdCommand;
    #[cfg(windows)]
    use std::time::{Duration, Instant};

    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    use super::{
        CommandError, CommandErrorKind, CommandExecutionBoundary, CommandExecutionPolicy,
        CommandRequest, Controlled, ProcessBoundary, UTF8_RUNTIME_ENVIRONMENT, await_controlled,
        decode_command_output, resolve_system_cwd, run_command, run_command_with_policy,
        validate_command_request, workspace_sandbox_available, workspace_sandbox_health,
    };
    use crate::Workspace;

    #[tokio::test]
    async fn command_runs_without_a_shell_in_workspace() {
        let directory = tempdir().expect("workspace should be created");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let request = if cfg!(windows) {
            CommandRequest {
                program: "where.exe".to_owned(),
                args: vec!["rustc.exe".to_owned()],
                cwd: ".".to_owned(),
                timeout_ms: 5_000,
            }
        } else {
            CommandRequest {
                program: "printf".to_owned(),
                args: vec!["safe".to_owned()],
                cwd: ".".to_owned(),
                timeout_ms: 5_000,
            }
        };
        let output = run_command(&workspace, &request, CancellationToken::new())
            .await
            .expect("command should run");
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(
            output.execution_boundary,
            CommandExecutionBoundary::ApprovedHost
        );
        assert_eq!(
            output.process_boundary,
            if cfg!(windows) {
                ProcessBoundary::WindowsJobObject
            } else {
                ProcessBoundary::ProcessOnly
            }
        );
    }

    #[test]
    fn command_environment_forces_python_and_locale_utf8() {
        assert!(UTF8_RUNTIME_ENVIRONMENT.contains(&("PYTHONUTF8", "1")));
        assert!(UTF8_RUNTIME_ENVIRONMENT.contains(&("PYTHONIOENCODING", "utf-8")));
        assert!(UTF8_RUNTIME_ENVIRONMENT.contains(&("LANG", "C.UTF-8")));
        assert_eq!(
            decode_command_output("你好，Simple".as_bytes()),
            "你好，Simple"
        );
    }

    #[cfg(windows)]
    #[test]
    fn bare_windows_commands_resolve_through_pathext() {
        let directory = tempdir().expect("path directory should be created");
        let shim = directory.path().join("npm.CMD");
        std::fs::write(&shim, "@echo off\r\n").expect("shim should be written");
        let path = std::env::join_paths([directory.path()]).expect("path should join");

        let resolved = super::resolve_windows_program_from("npm", &path, ".EXE;.CMD".as_ref());
        assert_eq!(resolved.as_deref(), Some(shim.as_path()));
    }

    #[cfg(windows)]
    #[test]
    fn legacy_windows_code_page_output_falls_back_without_replacement_characters() {
        let (encoded, _, had_errors) = encoding_rs::GBK.encode("你好，Simple");
        assert!(!had_errors);
        assert_eq!(
            super::decode_windows_legacy_output(&encoded, Some(936)).as_deref(),
            Some("你好，Simple")
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn python_unicode_output_is_forced_to_utf8_when_python_is_available() {
        if std::process::Command::new("python")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let directory = tempdir().expect("workspace should be created");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let request = CommandRequest {
            program: "python".to_owned(),
            args: vec!["-c".to_owned(), "print('你好，Simple')".to_owned()],
            cwd: ".".to_owned(),
            timeout_ms: 5_000,
        };
        let output = run_command(&workspace, &request, CancellationToken::new())
            .await
            .expect("python should run");
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout.trim(), "你好，Simple");
        assert!(!output.stdout.contains('\u{fffd}'));
    }

    #[tokio::test]
    async fn cancellation_stops_a_running_command() {
        let directory = tempdir().expect("workspace should be created");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let request = if cfg!(windows) {
            CommandRequest {
                program: "ping.exe".to_owned(),
                args: vec!["127.0.0.1".to_owned(), "-n".to_owned(), "30".to_owned()],
                cwd: ".".to_owned(),
                timeout_ms: 30_000,
            }
        } else {
            CommandRequest {
                program: "sleep".to_owned(),
                args: vec!["30".to_owned()],
                cwd: ".".to_owned(),
                timeout_ms: 30_000,
            }
        };
        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();
        let cancellation_task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            trigger.cancel();
        });

        let result = run_command(&workspace, &request, cancellation).await;
        cancellation_task
            .await
            .expect("cancellation trigger should finish");
        assert!(matches!(result, Err(CommandError::Cancelled)));
    }

    #[tokio::test]
    async fn a_pre_cancelled_request_never_attempts_to_spawn() {
        let directory = tempdir().expect("workspace should be created");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let request = CommandRequest {
            program: "local-agent-command-that-does-not-exist".to_owned(),
            args: Vec::new(),
            cwd: ".".to_owned(),
            timeout_ms: 5_000,
        };

        let result = run_command(&workspace, &request, cancellation).await;
        assert!(matches!(result, Err(CommandError::Cancelled)));
    }

    #[tokio::test]
    async fn invalid_working_directory_returns_actionable_command_error() {
        let directory = tempdir().expect("workspace should be created");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let request = CommandRequest {
            program: "program-that-must-not-spawn".to_owned(),
            args: Vec::new(),
            cwd: if cfg!(windows) {
                "/dev/stdin".to_owned()
            } else {
                "missing-directory".to_owned()
            },
            timeout_ms: 5_000,
        };
        let result = run_command(&workspace, &request, CancellationToken::new()).await;
        assert!(matches!(result, Err(CommandError::InvalidWorkingDirectory)));
    }

    #[tokio::test]
    async fn workspace_sandbox_health_is_truthful_before_spawning() {
        let directory = tempdir().expect("workspace should be created");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let request = CommandRequest {
            program: "command-that-must-not-run".to_owned(),
            args: Vec::new(),
            cwd: ".".to_owned(),
            timeout_ms: 5_000,
        };

        if workspace_sandbox_available() {
            assert!(workspace_sandbox_health().is_ready());
            return;
        }
        let result = run_command_with_policy(
            &workspace,
            &request,
            CancellationToken::new(),
            CommandExecutionPolicy::WorkspaceSandbox,
        )
        .await;
        assert_eq!(
            result.as_ref().err().map(CommandError::kind),
            Some(CommandErrorKind::SandboxUnavailable)
        );
        assert!(matches!(
            result,
            Err(CommandError::Sandbox(
                local_agent_sandbox::SandboxRuntimeError::Unavailable { .. }
            ))
        ));
    }

    #[test]
    fn system_full_access_can_resolve_an_absolute_directory_outside_the_project() {
        let project = tempdir().expect("workspace should be created");
        let outside = tempdir().expect("outside directory should be created");
        let workspace = Workspace::open(project.path()).expect("workspace should open");
        let resolved = resolve_system_cwd(&workspace, &outside.path().to_string_lossy())
            .expect("system access should accept an absolute directory");
        assert_eq!(
            resolved,
            std::fs::canonicalize(outside.path()).expect("outside directory should canonicalize")
        );
    }

    #[tokio::test]
    async fn absolute_deadline_also_limits_output_drain() {
        let cancellation = CancellationToken::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(20);
        let outcome = await_controlled(std::future::pending::<()>(), &cancellation, deadline).await;
        assert!(matches!(outcome, Controlled::TimedOut));
    }

    #[test]
    fn credential_shaped_command_arguments_are_rejected_without_echoing_them() {
        let cases = [
            vec!["--token".to_owned(), "example-value".to_owned()],
            vec!["--api-key=example-value".to_owned()],
            vec!["--api_key".to_owned(), "example-value".to_owned()],
            vec!["TOKEN=example-value".to_owned()],
            vec!["Authorization: Bearer example-value".to_owned()],
            vec![
                "-H".to_owned(),
                "authorization: Bearer example-value".to_owned(),
            ],
            vec!["-----BEGIN PRIVATE KEY-----\nexample-only\n-----END PRIVATE KEY-----".to_owned()],
            vec![
                "-----BEGIN".to_owned(),
                "example-only".to_owned(),
                "PRIVATE KEY-----".to_owned(),
            ],
            vec!["sk-example-not-a-real-secret".to_owned()],
        ];
        for args in cases {
            let request = request_with_args(args);
            let error = validate_command_request(&request)
                .expect_err("credential-shaped argument should be rejected");
            assert!(matches!(error, CommandError::SensitiveArgument { .. }));
            let message = error.to_string();
            assert!(!message.contains("example-value"));
            assert!(!message.contains("example-only"));
            assert!(!message.contains("sk-example"));
        }
    }

    #[test]
    fn common_command_errors_are_presented_in_chinese() {
        assert_eq!(CommandError::TimedOut.to_string(), "命令执行超时");
        assert_eq!(CommandError::Cancelled.to_string(), "命令已取消");
        assert!(
            CommandError::InvalidWorkingDirectory
                .to_string()
                .starts_with("命令工作目录无效")
        );
    }

    #[test]
    fn ordinary_similar_arguments_remain_allowed() {
        let request = request_with_args(vec![
            "tokenizer".to_owned(),
            "token".to_owned(),
            "secret".to_owned(),
            "authorization_tests".to_owned(),
            "sketch.rs".to_owned(),
            "--token-file".to_owned(),
            "fixtures/token.txt".to_owned(),
            "--api-key-file=fixtures/key.txt".to_owned(),
        ]);
        validate_command_request(&request).expect("non-secret arguments should remain valid");
    }

    #[test]
    fn command_request_size_limits_are_validated_publicly() {
        let empty_program = CommandRequest {
            program: " ".to_owned(),
            ..request_with_args(Vec::new())
        };
        assert!(matches!(
            validate_command_request(&empty_program),
            Err(CommandError::EmptyProgram)
        ));

        let oversized_program = CommandRequest {
            program: "p".repeat(1_025),
            ..request_with_args(Vec::new())
        };
        let too_many_arguments = request_with_args(vec!["a".to_owned(); 257]);
        let oversized_arguments = request_with_args(vec!["a".repeat(65_537)]);
        let oversized_cwd = CommandRequest {
            cwd: "c".repeat(2_049),
            ..request_with_args(Vec::new())
        };
        for request in [
            oversized_program,
            too_many_arguments,
            oversized_arguments,
            oversized_cwd,
        ] {
            assert!(matches!(
                validate_command_request(&request),
                Err(CommandError::InvalidRequest)
            ));
        }
    }

    fn request_with_args(args: Vec<String>) -> CommandRequest {
        CommandRequest {
            program: "example-program".to_owned(),
            args,
            cwd: ".".to_owned(),
            timeout_ms: 5_000,
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "subprocess helper"]
    fn job_object_descendant_leaf() {
        std::thread::sleep(Duration::from_secs(300));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "subprocess helper"]
    fn job_object_descendant_parent() {
        let executable = std::env::current_exe().expect("test executable should resolve");
        let mut descendant = StdCommand::new(executable)
            .args([
                "command::tests::job_object_descendant_leaf",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .spawn()
            .expect("descendant should spawn");
        std::fs::write("job-object-descendant.pid", descendant.id().to_string())
            .expect("descendant pid should be recorded");
        let _ = descendant.wait();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn timeout_terminates_the_windows_job_process_tree() {
        let directory = tempdir().expect("workspace should be created");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let executable = std::env::current_exe().expect("test executable should resolve");
        let request = CommandRequest {
            program: executable.to_string_lossy().into_owned(),
            args: vec![
                "command::tests::job_object_descendant_parent".to_owned(),
                "--exact".to_owned(),
                "--ignored".to_owned(),
                "--nocapture".to_owned(),
            ],
            cwd: ".".to_owned(),
            timeout_ms: 1_000,
        };

        let result = run_command(&workspace, &request, CancellationToken::new()).await;
        assert!(matches!(result, Err(CommandError::TimedOut)));

        let pid_path = directory.path().join("job-object-descendant.pid");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !pid_path.exists() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let pid = std::fs::read_to_string(pid_path)
            .expect("parent should record the descendant pid")
            .trim()
            .parse::<u32>()
            .expect("recorded pid should be numeric");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if !windows_process_exists(pid) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "descendant process {pid} survived termination of its job"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[cfg(windows)]
    fn windows_process_exists(pid: u32) -> bool {
        let filter = format!("PID eq {pid}");
        let output = StdCommand::new("tasklist.exe")
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist should be available on Windows");
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains(&format!(",\"{pid}\","))
    }
}
