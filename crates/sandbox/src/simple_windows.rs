use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use simple_windows_sandbox::{
    ElevatedSandboxCaptureRequest, SandboxPolicy, SandboxSetupRequest, SetupRootOverrides,
};

use crate::windows::{
    SandboxCommandRequest, SandboxCommandResponse, SandboxInstallPaths, SandboxProcessOutput,
    WindowsSandboxError,
};
use crate::{CURRENT_WINDOWS_SETUP_VERSION, SandboxBackend, SandboxHealth};

const OUTPUT_LIMIT_BYTES: usize = 1_048_576;
static SIMPLE_COMMAND_LOCK: Mutex<()> = Mutex::new(());

pub fn simple_sandbox_install_paths() -> Result<SandboxInstallPaths, WindowsSandboxError> {
    let helpers = find_simple_helpers().ok_or(WindowsSandboxError::NeedsSetup)?;
    Ok(SandboxInstallPaths {
        setup_helper: helpers.setup,
        command_runner: helpers.runner,
    })
}

pub fn install_simple_workspace_sandbox(
    paths: &SandboxInstallPaths,
) -> Result<SandboxHealth, WindowsSandboxError> {
    if simple_workspace_sandbox_health().is_ready() {
        return Ok(simple_workspace_sandbox_health());
    }
    let helpers = SimpleHelpers {
        setup: paths.setup_helper.clone(),
        runner: paths.command_runner.clone(),
    };
    if !helpers.setup.is_file() || !helpers.runner.is_file() {
        return Err(WindowsSandboxError::MissingHelper(format!(
            "{}; {}",
            helpers.setup.display(),
            helpers.runner.display()
        )));
    }
    let sandbox_home = simple_sandbox_home()?;
    fs::create_dir_all(&sandbox_home)?;
    let cwd = std::env::current_dir()?;
    let policy = workspace_policy();
    let environment = sandbox_environment(&sandbox_home);
    simple_windows_sandbox::run_elevated_setup(
        SandboxSetupRequest {
            policy: &policy,
            policy_cwd: &cwd,
            command_cwd: &cwd,
            env_map: &environment,
            sandbox_home: &sandbox_home,
            proxy_enforced: false,
        },
        SetupRootOverrides::default(),
    )
    .map_err(|error| WindowsSandboxError::SetupFailed(error.to_string()))?;
    let health = simple_workspace_sandbox_health();
    if !health.is_ready() {
        return Err(WindowsSandboxError::SetupFailed(format!(
            "Simple elevated sandbox 安装完成后仍不可用：{}",
            health.status_name()
        )));
    }
    Ok(health)
}

pub fn run_simple_workspace_command(
    request: SandboxCommandRequest,
    mut cancellation_requested: impl FnMut() -> bool + Send + 'static,
) -> Result<SandboxCommandResponse, WindowsSandboxError> {
    if cancellation_requested() {
        return Ok(SandboxCommandResponse::Failed {
            kind: "cancelled".to_owned(),
            message: "命令已取消".to_owned(),
        });
    }
    if !simple_workspace_sandbox_health().is_ready() {
        return Err(WindowsSandboxError::NeedsSetup);
    }
    let _guard = SIMPLE_COMMAND_LOCK
        .lock()
        .map_err(|_| WindowsSandboxError::ExecutionFailed("command lock poisoned".to_owned()))?;
    // `std::fs::canonicalize` returns a verbatim `\\?\` path on Windows. That form is
    // accepted by Win32 APIs but `cmd.exe` treats it as a UNC current directory and falls
    // back to `C:\Windows`, which breaks relative workspace commands (especially when the
    // workspace path contains non-ASCII characters). Keep the canonical path in normal form.
    let workspace_root = simple_windows_sandbox::canonicalize_path(&request.workspace_root);
    let cwd = resolve_workspace_cwd(&workspace_root, &request.cwd)?;
    let sandbox_home = simple_sandbox_home()?;
    let policy = workspace_policy();
    let policy_json = serde_json::to_string(&policy)?;
    let environment = sandbox_environment(&sandbox_home);
    let cancelled = Arc::new(AtomicBool::new(false));
    let polling_finished = Arc::new(AtomicBool::new(false));
    let cancellation_poll = {
        let cancelled = Arc::clone(&cancelled);
        let polling_finished = Arc::clone(&polling_finished);
        thread::spawn(move || {
            while !polling_finished.load(Ordering::Acquire) {
                if cancellation_requested() {
                    cancelled.store(true, Ordering::Release);
                    break;
                }
                thread::sleep(Duration::from_millis(40));
            }
        })
    };
    let result =
        simple_windows_sandbox::run_windows_sandbox_capture(ElevatedSandboxCaptureRequest {
            policy_json_or_preset: &policy_json,
            sandbox_policy_cwd: &workspace_root,
            sandbox_home: &sandbox_home,
            command: std::iter::once(request.program)
                .chain(request.args)
                .collect(),
            cwd: &cwd,
            env_map: environment,
            timeout_ms: Some(request.timeout_ms.clamp(1, 120_000)),
            use_private_desktop: false,
            proxy_enforced: false,
            read_roots_override: None,
            read_roots_include_platform_defaults: true,
            write_roots_override: None,
            deny_write_paths_override: &[],
            cancellation: Some(Arc::clone(&cancelled)),
        });
    polling_finished.store(true, Ordering::Release);
    let _ = cancellation_poll.join();
    let result = result.map_err(|error| WindowsSandboxError::ExecutionFailed(error.to_string()))?;
    if cancelled.load(Ordering::Acquire) {
        return Ok(SandboxCommandResponse::Failed {
            kind: "cancelled".to_owned(),
            message: "命令已取消".to_owned(),
        });
    }
    if result.timed_out {
        return Ok(SandboxCommandResponse::Failed {
            kind: "timed_out".to_owned(),
            message: "命令执行超时".to_owned(),
        });
    }
    let (stdout, stdout_truncated) = cap_output(result.stdout);
    let (stderr, stderr_truncated) = cap_output(result.stderr);
    Ok(SandboxCommandResponse::Completed(SandboxProcessOutput {
        exit_code: Some(result.exit_code),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        truncated: stdout_truncated || stderr_truncated,
    }))
}

pub fn simple_workspace_sandbox_health() -> SandboxHealth {
    let installed_setup_version = simple_sandbox_home()
        .ok()
        .and_then(|home| setup_version(&home));
    if find_simple_helpers().is_none() {
        return SandboxHealth::NeedsSetup {
            backend: SandboxBackend::WindowsSimple,
            expected_setup_version: CURRENT_WINDOWS_SETUP_VERSION,
        };
    }
    let ready = simple_sandbox_home().is_ok_and(|home| {
        simple_windows_sandbox::sandbox_setup_is_complete(&home)
            && setup_version(&home) == Some(CURRENT_WINDOWS_SETUP_VERSION)
    });
    if ready {
        SandboxHealth::Ready {
            backend: SandboxBackend::WindowsSimple,
            setup_version: CURRENT_WINDOWS_SETUP_VERSION,
        }
    } else if installed_setup_version.is_some() {
        SandboxHealth::Drifted {
            backend: SandboxBackend::WindowsSimple,
            expected_setup_version: CURRENT_WINDOWS_SETUP_VERSION,
            installed_setup_version,
        }
    } else {
        SandboxHealth::NeedsSetup {
            backend: SandboxBackend::WindowsSimple,
            expected_setup_version: CURRENT_WINDOWS_SETUP_VERSION,
        }
    }
}

fn workspace_policy() -> SandboxPolicy {
    SandboxPolicy::WorkspaceWrite {
        writable_roots: Vec::new(),
        network_access: true,
        exclude_tmpdir_env_var: true,
        exclude_slash_tmp: true,
    }
}

struct SimpleHelpers {
    setup: PathBuf,
    runner: PathBuf,
}

fn find_simple_helpers() -> Option<SimpleHelpers> {
    if let (Some(setup), Some(runner)) = (
        std::env::var_os("SIMPLE_SANDBOX_SETUP_EXE"),
        std::env::var_os("SIMPLE_SANDBOX_RUNNER_EXE"),
    ) {
        let helpers = SimpleHelpers {
            setup: setup.into(),
            runner: runner.into(),
        };
        if helpers.setup.is_file() && helpers.runner.is_file() {
            return Some(helpers);
        }
    }
    let mut directories = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        directories.push(directory.to_path_buf());
        directories.push(directory.join("simple-resources"));
        if directory.file_name().is_some_and(|name| name == "deps")
            && let Some(parent) = directory.parent()
        {
            directories.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        directories.push(cwd.join("target").join("debug"));
        directories.push(cwd.join("target").join("release"));
    }
    directories.into_iter().find_map(|directory| {
        let helpers = SimpleHelpers {
            setup: directory.join("simple-windows-sandbox-setup.exe"),
            runner: directory.join("simple-command-runner.exe"),
        };
        (helpers.setup.is_file() && helpers.runner.is_file()).then_some(helpers)
    })
}

fn simple_sandbox_home() -> Result<PathBuf, WindowsSandboxError> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or(WindowsSandboxError::InvalidSetup)?;
    Ok(PathBuf::from(local).join("Simple").join("sandbox-home"))
}

#[derive(Deserialize)]
struct SetupMarker {
    version: u32,
}

fn setup_version(sandbox_home: &Path) -> Option<u32> {
    let marker = fs::read(sandbox_home.join(".sandbox").join("setup_marker.json")).ok()?;
    serde_json::from_slice::<SetupMarker>(&marker)
        .ok()
        .map(|marker| marker.version)
}

fn resolve_workspace_cwd(root: &Path, requested: &str) -> Result<PathBuf, WindowsSandboxError> {
    let requested = Path::new(requested);
    if requested.is_absolute() {
        return Err(WindowsSandboxError::ExecutionFailed(
            "sandbox cwd must be workspace-relative".to_owned(),
        ));
    }
    let cwd = simple_windows_sandbox::canonicalize_path(&root.join(requested));
    if !cwd.starts_with(root) || !cwd.is_dir() {
        return Err(WindowsSandboxError::ExecutionFailed(
            "sandbox cwd escaped the workspace".to_owned(),
        ));
    }
    Ok(cwd)
}

fn sandbox_environment(sandbox_home: &Path) -> HashMap<String, String> {
    const ALLOWED: [&str; 22] = [
        "APPDATA",
        "COMSPEC",
        "HOMEDRIVE",
        "HOMEPATH",
        "HOME",
        "LOCALAPPDATA",
        "NUMBER_OF_PROCESSORS",
        "PATH",
        "PATHEXT",
        "PROCESSOR_ARCHITECTURE",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "PSMODULEPATH",
        "SYSTEMDRIVE",
        "SYSTEMROOT",
        "TEMP",
        "TMP",
        "USERDOMAIN",
        "USERNAME",
        "USERPROFILE",
        "WINDIR",
    ];
    let mut environment = ALLOWED
        .into_iter()
        .filter_map(|name| {
            std::env::var_os(name)
                .and_then(|value| value.into_string().ok().map(|value| (name, value)))
        })
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<HashMap<_, _>>();
    environment.extend([
        (
            "SIMPLE_SANDBOX_HOME".to_owned(),
            sandbox_home.to_string_lossy().into_owned(),
        ),
        ("PYTHONUTF8".to_owned(), "1".to_owned()),
        ("PYTHONIOENCODING".to_owned(), "utf-8".to_owned()),
    ]);
    environment
}

fn cap_output(mut output: Vec<u8>) -> (Vec<u8>, bool) {
    let truncated = output.len() > OUTPUT_LIMIT_BYTES;
    output.truncate(OUTPUT_LIMIT_BYTES);
    (output, truncated)
}
