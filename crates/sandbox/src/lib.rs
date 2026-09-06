//! Truthful sandbox policy and runtime planning.
//!
//! This crate deliberately contains no command runner. It is the stable safety
//! boundary between user authorization, requested permissions, and a platform
//! backend that can actually enforce them.

mod policy;
mod runtime;
#[cfg(windows)]
mod simple_windows;
mod windows;

pub use policy::{
    FileSystemPermission, ManagedPermissionProfile, NetworkPermission, PermissionProfile,
};
pub use runtime::{
    CURRENT_WINDOWS_SETUP_VERSION, ExecutionTarget, SandboxBackend, SandboxCapability,
    SandboxHealth, SandboxRuntime, SandboxRuntimeError,
};
#[cfg(windows)]
#[doc(hidden)]
pub use simple_windows::{
    install_simple_workspace_sandbox, run_simple_workspace_command, simple_sandbox_install_paths,
    simple_workspace_sandbox_health,
};
pub use windows::{
    SandboxCommandRequest, SandboxCommandResponse, SandboxInstallPaths, SandboxProcessOutput,
    WindowsSandboxError, install_workspace_sandbox, run_workspace_command,
    workspace_sandbox_install_paths,
};
