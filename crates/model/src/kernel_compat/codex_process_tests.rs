//! Simple-owned Codex compatibility layer.
use super::*;
use std::fs::{self, File, OpenOptions};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Stdio;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const HELPER: &str = "kernel_compat::codex_process::tests::process_tree_fixture";
const NO_WINDOW: u32 = 0x08000000;

fn helper(root: &Path, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--ignored", "--exact", HELPER, "--nocapture"])
        .env("SIMPLE_PROCESS_FIXTURE_ROOT", root)
        .env("SIMPLE_PROCESS_FIXTURE_ROLE", role)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(NO_WINDOW);
    command
}

fn locked(root: &Path, name: &str) -> io::Result<bool> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join(name))?;
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(fs::TryLockError::WouldBlock) => Ok(true),
        Err(fs::TryLockError::Error(error)) => Err(error),
    }
}

async fn wait_ready(root: &Path) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !(root.join("worker.ready").is_file() && root.join("leaf.ready").is_file()) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    if !locked(root, "leaf.lock")? {
        return Err("leaf did not retain its actual lock".into());
    }
    Ok(())
}

async fn wait_released(root: &Path) -> TestResult {
    tokio::time::timeout(Duration::from_secs(5), async {
        while locked(root, "worker.lock")? || locked(root, "leaf.lock")? {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_, io::Error>(())
    })
    .await
    .map_err(|_| "managed descendant still holds a live file lock")??;
    Ok(())
}

struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        // Only fixture processes observe this exit request. Failed assertions
        // must not leave test grandchildren running or target unrelated PIDs.
        let _ = fs::write(self.0.join("exit.request"), b"exit");
        for _ in 0..100 {
            if !locked(&self.0, "worker.lock").unwrap_or(true)
                && !locked(&self.0, "leaf.lock").unwrap_or(true)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[tokio::test]
async fn managed_shutdown_releases_descendant_after_direct_child_already_exited() -> TestResult {
    let temp = tempfile::tempdir()?;
    let _cleanup = Cleanup(temp.path().into());
    let mut command = helper(temp.path(), "worker");
    command.env("SIMPLE_PROCESS_FIXTURE_EXIT_ROOT", "yes");
    let mut child = spawn(command)?;
    wait_ready(temp.path()).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while child.try_wait()?.is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_, io::Error>(())
    })
    .await??;
    assert!(
        locked(temp.path(), "leaf.lock")?,
        "grandchild must still be active after root exit"
    );
    terminate(&mut child).await?;
    wait_released(temp.path()).await
}

#[tokio::test]
async fn managed_drop_releases_direct_child_and_grandchild() -> TestResult {
    let temp = tempfile::tempdir()?;
    let _cleanup = Cleanup(temp.path().into());
    let child = spawn(helper(temp.path(), "worker"))?;
    wait_ready(temp.path()).await?;
    assert!(locked(temp.path(), "worker.lock")?);
    drop(child);
    wait_released(temp.path()).await
}

#[tokio::test]
async fn hard_host_exit_releases_managed_descendants_without_rust_drop() -> TestResult {
    let temp = tempfile::tempdir()?;
    let _cleanup = Cleanup(temp.path().into());
    let mut host = helper(temp.path(), "host").spawn()?;
    let ready = wait_ready(temp.path()).await;
    // The exact host is a child we just created. TerminateProcess skips Rust Drop.
    host.kill()?;
    host.wait()?;
    ready?;
    wait_released(temp.path()).await
}

async fn native_fixture(root: &Path) -> TestResult<crate::CodexKernelClient> {
    let executable = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER")
        .ok_or("explicit pinned kernel required")?;
    let (client, _events) = crate::CodexKernelClient::start(crate::CodexKernelConfig::new(
        executable,
        root.join("native-home"),
        crate::ResponsesGatewayConfig::new("http://127.0.0.1:9", "unused-no-inference", None),
    ))
    .await?;
    let started=client.request("process/spawn",serde_json::json!({
        "command":[std::env::current_exe()?.to_str().ok_or("test executable encoding")?,"--ignored","--exact",HELPER,"--nocapture"],
        "processHandle":"simple-lifecycle-fixture","cwd":root,"timeoutMs":null,
        "env":{"SIMPLE_PROCESS_FIXTURE_ROOT":root,"SIMPLE_PROCESS_FIXTURE_ROLE":"worker"}
    })).await;
    if let Err(error) = started {
        let _ = client.shutdown().await;
        return Err(error.into());
    }
    Ok(client)
}

#[tokio::test]
#[ignore = "explicit pinned native kernel; real process/spawn, no model inference"]
async fn native_kernel_shutdown_releases_actual_nested_processes() -> TestResult {
    let temp = tempfile::tempdir()?;
    let _cleanup = Cleanup(temp.path().into());
    let client = native_fixture(temp.path()).await?;
    let ready = wait_ready(temp.path()).await;
    let shutdown = client.shutdown().await;
    ready?;
    shutdown?;
    wait_released(temp.path()).await
}

#[tokio::test]
#[ignore = "explicit pinned native kernel; hard host crash while native grandchildren own file locks"]
async fn native_kernel_host_crash_releases_actual_nested_processes() -> TestResult {
    std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER").ok_or("explicit pinned kernel required")?;
    let temp = tempfile::tempdir()?;
    let _cleanup = Cleanup(temp.path().into());
    let mut host = helper(temp.path(), "native-host").spawn()?;
    let ready = wait_ready(temp.path()).await;
    host.kill()?;
    host.wait()?;
    ready?;
    wait_released(temp.path()).await
}

#[tokio::test]
#[ignore = "isolated process-tree fixture; invoked by its exact test name and temp directory"]
async fn process_tree_fixture() -> TestResult {
    let root = std::path::PathBuf::from(
        std::env::var_os("SIMPLE_PROCESS_FIXTURE_ROOT").ok_or("fixture root")?,
    );
    let role = std::env::var("SIMPLE_PROCESS_FIXTURE_ROLE")?;
    if role == "native-host" {
        let _client = native_fixture(&root).await?;
        while !root.join("exit.request").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        return Ok(());
    }
    if role == "host" {
        let _child = spawn(helper(&root, "worker"))?;
        while !root.join("exit.request").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        return Ok(());
    }
    if role != "worker" && role != "leaf" {
        return Err("invalid fixture role".into());
    }
    let lock = File::create(root.join(format!("{role}.lock")))?;
    lock.lock()?;
    if role == "worker" {
        let _leaf = helper(&root, "leaf").spawn()?;
        while !root.join("leaf.ready").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    fs::write(root.join(format!("{role}.ready")), b"ready")?;
    if role == "worker" && std::env::var("SIMPLE_PROCESS_FIXTURE_EXIT_ROOT").as_deref() == Ok("yes")
    {
        return Ok(());
    }
    while !root.join("exit.request").exists() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    drop(lock);
    Ok(())
}
