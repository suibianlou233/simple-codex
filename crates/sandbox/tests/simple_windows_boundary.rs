#![cfg(windows)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use local_agent_sandbox::{
    SandboxCommandRequest, SandboxCommandResponse, install_workspace_sandbox,
    run_workspace_command, workspace_sandbox_install_paths,
};
use simple_windows_sandbox::{ElevatedSandboxCaptureRequest, SandboxPolicy};

#[test]
#[ignore = "requires UAC and the Simple Windows sandbox helper binaries"]
fn installs_simple_elevated_sandbox() {
    let paths = workspace_sandbox_install_paths().expect("Simple helpers should be discoverable");
    let health = install_workspace_sandbox(&paths).expect("Simple sandbox setup should succeed");
    assert!(health.is_ready(), "{health:?}");
}

#[test]
#[ignore = "requires the installed Simple Windows sandbox"]
fn simple_internal_accounts_hide_existing_profile_directories() {
    const HIDDEN_AND_SYSTEM: u32 = 0x2 | 0x4;
    let root = workspace("hide-profile");
    let sandbox_home =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA should be available"))
            .join("Simple")
            .join("sandbox-home");
    let policy = SandboxPolicy::WorkspaceWrite {
        writable_roots: Vec::new(),
        network_access: false,
        exclude_tmpdir_env_var: true,
        exclude_slash_tmp: true,
    };
    let policy_json = serde_json::to_string(&policy).expect("serialize sandbox policy");
    let output =
        simple_windows_sandbox::run_windows_sandbox_capture(ElevatedSandboxCaptureRequest {
            policy_json_or_preset: &policy_json,
            sandbox_policy_cwd: &root,
            sandbox_home: &sandbox_home,
            command: vec![
                "cmd.exe".to_owned(),
                "/D".to_owned(),
                "/C".to_owned(),
                "exit 0".to_owned(),
            ],
            cwd: &root,
            env_map: std::collections::HashMap::new(),
            timeout_ms: Some(30_000),
            use_private_desktop: false,
            proxy_enforced: false,
            read_roots_override: None,
            read_roots_include_platform_defaults: true,
            write_roots_override: None,
            deny_write_paths_override: &[],
            cancellation: None,
        })
        .expect("offline sandbox command should run");
    assert_eq!(output.exit_code, 0, "offline maintenance probe failed");

    let profile = PathBuf::from(std::env::var_os("SystemDrive").unwrap_or_else(|| "C:".into()))
        .join("Users")
        .join("SimpleSandboxOffline");
    let attributes = fs::metadata(&profile)
        .expect("offline sandbox profile should exist")
        .file_attributes();
    assert_eq!(
        attributes & HIDDEN_AND_SYSTEM,
        HIDDEN_AND_SYSTEM,
        "{} should be hidden and system",
        profile.display()
    );
    fs::remove_dir(root).expect("remove workspace");
}

#[test]
#[ignore = "requires the installed Simple Windows sandbox"]
fn simple_runner_writes_inside_workspace() {
    let root = workspace("inside");
    let output = completed(run(
        &root,
        "cmd.exe",
        vec![
            "/D".to_owned(),
            "/S".to_owned(),
            "/C".to_owned(),
            "echo sandbox-ok>probe.txt".to_owned(),
        ],
    ));
    assert_eq!(output.exit_code, Some(0), "{}", output.stderr);
    assert_eq!(
        fs::read_to_string(root.join("probe.txt")).expect("inside probe"),
        "sandbox-ok\r\n"
    );
    fs::remove_file(root.join("probe.txt")).expect("remove probe");
    fs::remove_dir(root).expect("remove workspace");
}

#[test]
#[ignore = "requires the installed Simple Windows sandbox"]
fn simple_runner_blocks_write_outside_workspace() {
    let project = std::env::current_dir().expect("current directory");
    let root = workspace("outside");
    let outside = project
        .join("target")
        .join("simple-sandbox-outside-probe.txt");
    let _ = fs::remove_file(&outside);
    let output = completed(run(
        &root,
        "cmd.exe",
        vec![
            "/D".to_owned(),
            "/S".to_owned(),
            "/C".to_owned(),
            format!("echo escaped>\"{}\"", outside.display()),
        ],
    ));
    assert_ne!(output.exit_code, Some(0), "{}", output.stdout);
    assert!(!outside.exists(), "Simple sandbox boundary was escaped");
    fs::remove_dir(root).expect("remove workspace");
}

#[test]
#[ignore = "requires the installed Simple Windows sandbox"]
fn simple_runner_allows_outbound_network() {
    let root = workspace("network");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP probe");
    let address = listener.local_addr().expect("read local HTTP address");
    listener
        .set_nonblocking(true)
        .expect("make local HTTP probe nonblocking");
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Winsock inherits the listener's nonblocking mode. Read
                    // with a bounded blocking timeout, not a scheduling race.
                    stream
                        .set_nonblocking(false)
                        .expect("blocking probe stream");
                    // Read the request before closing the socket. Dropping a
                    // socket with unread inbound data can reset an otherwise
                    // successful loopback connection on Windows.
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .expect("request read timeout");
                    let mut request = Vec::new();
                    let mut byte = [0_u8; 1];
                    while !request.ends_with(b"\r\n\r\n") {
                        assert!(request.len() < 8192, "oversized probe request");
                        assert_eq!(stream.read(&mut byte).expect("read probe request"), 1);
                        request.push(byte[0]);
                    }
                    stream
                        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .expect("write local HTTP response");
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "local HTTP probe timed out");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("local HTTP probe failed: {error}"),
            }
        }
    });
    let output = completed(run(
        &root,
        "node.exe",
        vec![
            "-e".to_owned(),
            format!(
                "const timer=setTimeout(()=>process.exit(8),5000); fetch('http://{address}/').then(r=>{{clearTimeout(timer); console.log(`NETWORK_ALLOWED ${{r.status}}`); process.exit(0)}}).catch(e=>{{clearTimeout(timer); console.error(e.message); process.exit(7)}})"
            ),
        ],
    ));
    server.join().expect("local HTTP probe should finish");
    assert_eq!(output.exit_code, Some(0), "{}", output.stderr);
    assert!(
        output.stdout.contains("NETWORK_ALLOWED"),
        "{}",
        output.stdout
    );
    fs::remove_dir(root).expect("remove workspace");
}

#[test]
#[ignore = "requires the installed Simple Windows sandbox"]
fn simple_child_process_inherits_workspace_boundary() {
    let project = std::env::current_dir().expect("current directory");
    let root = workspace("child");
    let outside = project
        .join("target")
        .join("simple-sandbox-child-outside-probe.txt");
    let inside = root.join("simple-sandbox-child-inside-probe.txt");
    let _ = fs::remove_file(&outside);
    let child_command = format!(
        "echo inside-ok>simple-sandbox-child-inside-probe.txt& echo escaped>\"{}\"",
        outside.display()
    );
    let child_command_json = serde_json::to_string(&child_command).expect("command json");
    let script = format!(
        "const {{spawnSync}}=require('node:child_process'); const child=spawnSync(process.env.ComSpec,['/D','/S','/C',{child_command_json}],{{encoding:'utf8'}}); console.log(JSON.stringify({{status:child.status,signal:child.signal,error:child.error&&{{name:child.error.name,code:child.error.code,message:child.error.message,path:child.error.path}},stdout:child.stdout,stderr:child.stderr}})); process.exit(child.status===null?9:child.status===0?10:0)"
    );
    let output = completed(run(&root, "node.exe", vec!["-e".to_owned(), script]));
    assert_eq!(
        output.exit_code,
        Some(0),
        "{}\n{}",
        output.stdout,
        output.stderr
    );
    assert_eq!(
        fs::read_to_string(&inside)
            .expect("child should write inside workspace")
            .trim(),
        "inside-ok"
    );
    assert!(
        !outside.exists(),
        "child escaped the Simple sandbox boundary"
    );
    fs::remove_file(inside).expect("remove inside probe");
    fs::remove_dir(root).expect("remove workspace");
}

#[test]
#[ignore = "requires the installed Simple Windows sandbox"]
fn simple_runner_cancels_a_running_process_tree() {
    let root = workspace("cancel");
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation_signal = Arc::clone(&cancelled);
    let trigger = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        cancellation_signal.store(true, Ordering::Release);
    });
    let started = Instant::now();
    let response = run_workspace_command(
        SandboxCommandRequest {
            workspace_root: root.clone(),
            program: "cmd.exe".to_owned(),
            args: vec![
                "/D".to_owned(),
                "/S".to_owned(),
                "/C".to_owned(),
                "ping 127.0.0.1 -n 30 >NUL".to_owned(),
            ],
            cwd: ".".to_owned(),
            timeout_ms: 30_000,
            response_path: Default::default(),
            cancellation_path: Default::default(),
        },
        move || cancelled.load(Ordering::Acquire),
    )
    .expect("Simple sandbox cancellation should be reported");
    trigger.join().expect("cancellation trigger should finish");
    assert!(
        matches!(response, SandboxCommandResponse::Failed { ref kind, .. } if kind == "cancelled"),
        "{response:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    fs::remove_dir(root).expect("remove workspace");
}

fn workspace(label: &str) -> std::path::PathBuf {
    let root = std::env::current_dir()
        .expect("current directory")
        .join("target")
        .join(format!("simple-sandbox-{label}-{}", std::process::id()));
    fs::create_dir_all(&root).expect("sandbox workspace");
    root
}

fn run(root: &Path, program: &str, args: Vec<String>) -> SandboxCommandResponse {
    run_workspace_command(
        SandboxCommandRequest {
            workspace_root: root.to_path_buf(),
            program: program.to_owned(),
            args,
            cwd: ".".to_owned(),
            timeout_ms: 30_000,
            response_path: Default::default(),
            cancellation_path: Default::default(),
        },
        || false,
    )
    .expect("Simple sandbox command should run")
}

fn completed(response: SandboxCommandResponse) -> local_agent_sandbox::SandboxProcessOutput {
    match response {
        SandboxCommandResponse::Completed(output) => output,
        failure => panic!("Simple sandbox runner returned {failure:?}"),
    }
}
