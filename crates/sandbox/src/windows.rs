use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::SandboxHealth;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxCommandRequest {
    pub workspace_root: PathBuf,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub timeout_ms: u64,
    pub response_path: PathBuf,
    pub cancellation_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SandboxCommandResponse {
    Completed(SandboxProcessOutput),
    Failed { kind: String, message: String },
}

#[derive(Debug, Error)]
pub enum WindowsSandboxError {
    #[error("Windows 项目沙箱只支持 Windows")]
    UnsupportedPlatform,
    #[error("Simple Windows 沙箱尚未安装")]
    NeedsSetup,
    #[error("Simple Windows 沙箱安装无效")]
    InvalidSetup,
    #[error("缺少 Simple Windows 沙箱组件：{0}")]
    MissingHelper(String),
    #[error("无法启动 Simple Windows 沙箱：{0}")]
    Spawn(#[from] std::io::Error),
    #[error("Simple Windows 沙箱初始化失败：{0}")]
    SetupFailed(String),
    #[error("Simple Windows 沙箱执行失败：{0}")]
    ExecutionFailed(String),
    #[error("Simple Windows 沙箱协议失败：{0}")]
    Protocol(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxInstallPaths {
    pub setup_helper: PathBuf,
    pub command_runner: PathBuf,
}

pub fn workspace_sandbox_install_paths() -> Result<SandboxInstallPaths, WindowsSandboxError> {
    #[cfg(windows)]
    {
        crate::simple_windows::simple_sandbox_install_paths()
    }
    #[cfg(not(windows))]
    {
        Err(WindowsSandboxError::UnsupportedPlatform)
    }
}

pub fn install_workspace_sandbox(
    paths: &SandboxInstallPaths,
) -> Result<SandboxHealth, WindowsSandboxError> {
    #[cfg(windows)]
    {
        crate::simple_windows::install_simple_workspace_sandbox(paths)
    }
    #[cfg(not(windows))]
    {
        let _ = paths;
        Err(WindowsSandboxError::UnsupportedPlatform)
    }
}

pub fn run_workspace_command(
    request: SandboxCommandRequest,
    cancellation_requested: impl FnMut() -> bool + Send + 'static,
) -> Result<SandboxCommandResponse, WindowsSandboxError> {
    #[cfg(windows)]
    {
        crate::simple_windows::run_simple_workspace_command(request, cancellation_requested)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        let _ = cancellation_requested;
        Err(WindowsSandboxError::UnsupportedPlatform)
    }
}

pub(crate) fn current_windows_health() -> SandboxHealth {
    #[cfg(windows)]
    {
        crate::simple_windows::simple_workspace_sandbox_health()
    }
    #[cfg(not(windows))]
    {
        SandboxHealth::UnsupportedPlatform
    }
}

#[cfg(test)]
mod tests {
    use super::SandboxCommandRequest;

    #[test]
    fn adapter_protocol_round_trips() {
        let request = SandboxCommandRequest {
            workspace_root: "C:\\work".into(),
            program: "node.exe".to_owned(),
            args: vec!["--test".to_owned()],
            cwd: ".".to_owned(),
            timeout_ms: 5_000,
            response_path: Default::default(),
            cancellation_path: Default::default(),
        };
        let encoded = serde_json::to_vec(&request).expect("request should encode");
        let decoded: SandboxCommandRequest =
            serde_json::from_slice(&encoded).expect("request should decode");
        assert_eq!(decoded, request);
    }
}
