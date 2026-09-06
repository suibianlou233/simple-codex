// Derived from OpenAI Codex windows-sandbox-rs at rust-v0.130.0.
// Modified by the Simple project: removed Agent/runtime dependencies, telemetry,
// legacy execution and TTY support; renamed product resources and identities.
#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "windows")]
mod acl;
#[cfg(target_os = "windows")]
mod allow;
#[cfg(target_os = "windows")]
mod cap;
#[cfg(target_os = "windows")]
mod desktop;
#[cfg(target_os = "windows")]
mod dpapi;
#[cfg(target_os = "windows")]
mod elevated_impl;
#[cfg(target_os = "windows")]
mod env;
#[cfg(target_os = "windows")]
mod helper_materialization;
#[cfg(target_os = "windows")]
mod hide_users;
#[cfg(target_os = "windows")]
mod identity;
#[cfg(target_os = "windows")]
#[path = "elevated/ipc_framed.rs"]
mod ipc_framed;
#[cfg(target_os = "windows")]
mod local_sid;
#[cfg(target_os = "windows")]
mod logging;
#[cfg(target_os = "windows")]
mod path_normalization;
#[cfg(target_os = "windows")]
mod policy;
#[cfg(target_os = "windows")]
#[path = "proc_thread_attr.rs"]
mod proc_thread_attr;
#[cfg(target_os = "windows")]
mod process;
#[cfg(target_os = "windows")]
#[path = "elevated/runner_client.rs"]
mod runner_client;
#[cfg(target_os = "windows")]
#[path = "elevated/runner_pipe.rs"]
mod runner_pipe;
#[cfg(target_os = "windows")]
mod sandbox_utils;
#[cfg(target_os = "windows")]
#[path = "setup_orchestrator.rs"]
mod setup;
#[cfg(target_os = "windows")]
mod setup_error;
#[cfg(any(target_os = "windows", test))]
mod ssh_config_dependencies;
#[cfg(target_os = "windows")]
mod token;
#[cfg(target_os = "windows")]
mod wfp;
#[cfg(target_os = "windows")]
mod wfp_setup;
#[cfg(target_os = "windows")]
mod winutil;
#[cfg(target_os = "windows")]
mod workspace_acl;

#[derive(Debug, Default)]
pub struct CaptureResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

#[cfg(target_os = "windows")]
pub use acl::{
    add_deny_write_ace, allow_null_device, ensure_allow_mask_aces,
    ensure_allow_mask_aces_with_inheritance, ensure_allow_write_aces, fetch_dacl_handle,
    path_mask_allows,
};
#[cfg(target_os = "windows")]
pub use cap::{load_or_create_cap_sids, workspace_cap_sid_for_cwd};
#[cfg(target_os = "windows")]
pub use desktop::LaunchDesktop;
#[cfg(target_os = "windows")]
pub use dpapi::{protect as dpapi_protect, unprotect as dpapi_unprotect};
#[cfg(target_os = "windows")]
pub use elevated_impl::{ElevatedSandboxCaptureRequest, run_windows_sandbox_capture};
#[cfg(target_os = "windows")]
pub use helper_materialization::resolve_current_exe_for_launch;
#[cfg(target_os = "windows")]
pub use hide_users::{hide_current_user_profile_dir, hide_newly_created_users};
#[cfg(target_os = "windows")]
pub use identity::{require_logon_sandbox_creds, sandbox_setup_is_complete};
#[cfg(target_os = "windows")]
pub use ipc_framed::{
    ErrorPayload, ExitPayload, FramedMessage, Message, OutputPayload, OutputStream, ResizePayload,
    SpawnReady, SpawnRequest, decode_bytes, encode_bytes, read_frame, write_frame,
};
#[cfg(target_os = "windows")]
pub use local_sid::LocalSid;
#[cfg(target_os = "windows")]
pub use logging::{LOG_FILE_NAME, log_note};
#[cfg(target_os = "windows")]
pub use path_normalization::canonicalize_path;
#[cfg(target_os = "windows")]
pub use policy::{NetworkAccess, SandboxPolicy, parse_policy};
#[cfg(target_os = "windows")]
pub use process::{
    PipeSpawnHandles, StderrMode, StdinMode, create_process_as_user, read_handle_loop,
    spawn_process_with_pipes,
};
#[cfg(target_os = "windows")]
pub use setup::{
    SETUP_VERSION, SandboxSetupRequest, SetupRootOverrides, run_elevated_setup, run_setup_refresh,
    run_setup_refresh_with_extra_read_roots, sandbox_bin_dir, sandbox_dir, sandbox_secrets_dir,
};
#[cfg(target_os = "windows")]
pub use setup_error::{
    SetupErrorCode, SetupErrorReport, SetupFailure, extract_failure as extract_setup_failure,
    setup_error_path, write_setup_error_report,
};
#[cfg(target_os = "windows")]
pub use token::{
    convert_string_sid_to_sid, create_readonly_token_with_caps_and_user_from,
    create_workspace_write_token_with_caps_and_user_from, get_current_token_for_restriction,
};
#[cfg(target_os = "windows")]
pub use wfp::install_wfp_filters_for_account;
#[cfg(target_os = "windows")]
pub use wfp_setup::install_wfp_filters;
#[cfg(target_os = "windows")]
pub use winutil::{quote_windows_arg, string_from_sid_bytes, to_wide};
#[cfg(target_os = "windows")]
pub use workspace_acl::is_command_cwd_root;

#[cfg(not(target_os = "windows"))]
pub fn run_windows_sandbox_capture(
    _request: ElevatedSandboxCaptureRequest<'_>,
) -> anyhow::Result<CaptureResult> {
    anyhow::bail!("Simple Windows sandbox is only available on Windows")
}

#[cfg(not(target_os = "windows"))]
pub struct ElevatedSandboxCaptureRequest<'a> {
    pub policy_json_or_preset: &'a str,
    pub sandbox_policy_cwd: &'a std::path::Path,
    pub sandbox_home: &'a std::path::Path,
    pub command: Vec<String>,
    pub cwd: &'a std::path::Path,
    pub env_map: std::collections::HashMap<String, String>,
    pub timeout_ms: Option<u64>,
    pub use_private_desktop: bool,
    pub proxy_enforced: bool,
    pub read_roots_override: Option<&'a [std::path::PathBuf]>,
    pub read_roots_include_platform_defaults: bool,
    pub write_roots_override: Option<&'a [std::path::PathBuf]>,
    pub deny_write_paths_override: &'a [std::path::PathBuf],
    pub cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}
