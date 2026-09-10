//! User-operated interactive terminal. Never exposed as an agent tool.
use super::{DesktopState, command_error, parse_task_id};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, TryRecvError},
    },
};
use tauri::State;

#[derive(Default)]
pub(crate) struct PtyState(Arc<Mutex<HashMap<String, Session>>>);
struct Session {
    output: Receiver<Vec<u8>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Box<dyn Child + Send + Sync>,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
impl PtyState {
    pub(crate) fn close_all(&self) {
        if let Ok(mut sessions) = self.0.lock() {
            sessions.clear();
        }
    }
}
#[derive(Serialize)]
pub(crate) struct Opened {
    id: String,
    cwd: String,
}
#[derive(Serialize)]
pub(crate) struct Output {
    data: Vec<u8>,
    exited: bool,
}
fn size(cols: u16, rows: u16) -> Result<PtySize, String> {
    if !(2..=1000).contains(&cols) || !(1..=500).contains(&rows) {
        return Err("终端尺寸无效".into());
    }
    Ok(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}
fn spawn(root: &std::path::Path, dimensions: PtySize) -> Result<Session, String> {
    let pair = native_pty_system()
        .openpty(dimensions)
        .map_err(|e| e.to_string())?;
    let mut command = if cfg!(windows) {
        let mut cmd = CommandBuilder::new("powershell.exe");
        cmd.args(["-NoLogo"]);
        cmd
    } else {
        CommandBuilder::new_default_prog()
    };
    command.cwd(root);
    command.env("TERM", "xterm-256color");
    // Do not pass Simple's provider or internal gateway credentials to a shell.
    let sensitive: Vec<_> = command
        .iter_full_env_as_str()
        .filter_map(|(key, _)| {
            let name = key.to_ascii_uppercase();
            (name.contains("API_KEY")
                || name.contains("TOKEN")
                || name.contains("SECRET")
                || name.starts_with("SIMPLE_CODEX_"))
            .then(|| key.to_owned())
        })
        .collect();
    for key in sensitive {
        command.env_remove(key);
    }
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    // portable-pty enables ConPTY cursor inheritance. Its initial cursor
    // query must be answered before CreateProcessW can finish; the fresh
    // frontend terminal starts at row 1, column 1.
    #[cfg(windows)]
    writer.write_all(b"\x1b[1;1R").map_err(|e| e.to_string())?;
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    drop(pair.slave);
    let (sender, output) = mpsc::sync_channel(32);
    if let Err(error) = std::thread::Builder::new()
        .name("simple-terminal-output".into())
        .spawn(move || {
            let mut reader = reader;
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    // ConPTY close waits for its output to drain. Keep reading even
                    // after the UI receiver is gone; never abandon the output pipe.
                    Ok(count) => {
                        let _ = sender.send(buffer[..count].to_vec());
                    }
                }
            }
        })
    {
        let _ = child.kill();
        return Err(error.to_string());
    }
    Ok(Session {
        master: Arc::new(Mutex::new(pair.master)),
        writer: Arc::new(Mutex::new(writer)),
        child,
        output,
    })
}
#[tauri::command]
pub(crate) async fn pty_open(
    task_id: String,
    cols: u16,
    rows: u16,
    state: State<'_, DesktopState>,
    terminals: State<'_, PtyState>,
) -> Result<Opened, String> {
    let dimensions = size(cols, rows)?;
    let root = {
        let runtime = state.lock().map_err(command_error)?;
        let id = parse_task_id(&task_id).map_err(command_error)?;
        let snapshot = runtime.core.snapshot();
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.id == id)
            .ok_or("任务不存在")?;
        snapshot
            .projects
            .iter()
            .find(|project| project.id == task.project_id)
            .ok_or("项目不存在")?
            .root
            .clone()
    };
    let terminals = terminals.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut sessions = terminals.lock().map_err(|_| "终端状态不可用")?;
        if sessions.len() >= 16 {
            return Err("打开的终端过多".into());
        }
        let session = spawn(&root, dimensions)?;
        let id = uuid::Uuid::new_v4().to_string();
        sessions.insert(id.clone(), session);
        Ok(Opened {
            id,
            cwd: super::user_visible_path(&root),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn pty_read(id: String, terminals: State<'_, PtyState>) -> Result<Output, String> {
    let mut sessions = terminals.0.lock().map_err(|_| "终端状态不可用")?;
    let session = sessions.get_mut(&id).ok_or("终端已关闭")?;
    let mut data = Vec::new();
    let mut exited = false;
    for _ in 0..16 {
        match session.output.try_recv() {
            Ok(chunk) => data.extend(chunk),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                exited = true;
                break;
            }
        }
    }
    Ok(Output { data, exited })
}
#[tauri::command]
pub(crate) async fn pty_write(
    id: String,
    data: String,
    terminals: State<'_, PtyState>,
) -> Result<(), String> {
    if data.len() > 65536 {
        return Err("单次终端输入过长".into());
    }
    let writer = terminals
        .0
        .lock()
        .map_err(|_| "终端状态不可用")?
        .get(&id)
        .ok_or("终端已关闭")?
        .writer
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut writer = writer.lock().map_err(|_| "终端输入不可用".to_owned())?;
        writer
            .write_all(data.as_bytes())
            .and_then(|_| writer.flush())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn pty_resize(
    id: String,
    cols: u16,
    rows: u16,
    terminals: State<'_, PtyState>,
) -> Result<(), String> {
    let dimensions = size(cols, rows)?;
    let master = terminals
        .0
        .lock()
        .map_err(|_| "终端状态不可用")?
        .get(&id)
        .ok_or("终端已关闭")?
        .master
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        master
            .lock()
            .map_err(|_| "终端尺寸不可用".to_owned())?
            .resize(dimensions)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn pty_close(id: String, terminals: State<'_, PtyState>) -> Result<(), String> {
    let terminals = terminals.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let session = terminals
            .lock()
            .map_err(|_| "终端状态不可用".to_owned())?
            .remove(&id);
        drop(session);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    fn wait_for(session: &Session, expected: &str) -> Result<(), String> {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut bytes = Vec::new();
        while std::time::Instant::now() < until {
            if let Ok(chunk) = session.output.recv_timeout(std::time::Duration::from_millis(100)) {
                bytes.extend(chunk);
                if String::from_utf8_lossy(&bytes).contains(expected) { return Ok(()); }
            }
        }
        Err(format!("等待终端阶段超时: {expected}"))
    }
    #[test]
    fn rejects_invalid_dimensions() {
        assert!(size(0, 24).is_err());
        assert!(size(80, 0).is_err());
        assert!(size(1001, 24).is_err());
        assert!(size(80, 24).is_ok());
    }
    #[test]
    #[cfg(windows)]
    fn conpty_interactive_shell_preserves_state_and_resizes() -> Result<(), String> {
        let session = spawn(&std::env::temp_dir(), size(80, 24)?)?;
        session
            .writer
            .lock()
            .map_err(|e| e.to_string())?
            .write_all(b"$simplePtyTest = 4321\r")
            .map_err(|e| e.to_string())?;
        session
            .writer
            .lock()
            .map_err(|e| e.to_string())?
            .write_all(b"Write-Output ($simplePtyTest + 1)\r")
            .map_err(|e| e.to_string())?;
        session
            .master
            .lock()
            .map_err(|e| e.to_string())?
            .resize(size(100, 30)?)
            .map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut bytes = Vec::new();
        while std::time::Instant::now() < deadline {
            if let Ok(chunk) = session
                .output
                .recv_timeout(std::time::Duration::from_millis(100))
            {
                bytes.extend(chunk);
                if String::from_utf8_lossy(&bytes).contains("4322") {
                    session
                        .writer
                        .lock()
                        .map_err(|e| e.to_string())?
                        .write_all(b"Write-Output ('RUN'+'NING'); Start-Sleep -Seconds 30\r")
                        .map_err(|e| e.to_string())?;
                    wait_for(&session, "RUNNING")?;
                    session
                        .writer
                        .lock()
                        .map_err(|e| e.to_string())?
                        .write_all(b"\x03")
                        .map_err(|e| e.to_string())?;
                    wait_for(&session, "PS ")?;
                    session
                        .writer
                        .lock()
                        .map_err(|e| e.to_string())?
                        .write_all(b"Write-Output (9000 + 9)\r")
                        .map_err(|e| e.to_string())?;
                    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    let mut interrupted = Vec::new();
                    while std::time::Instant::now() < until {
                        if let Ok(chunk) = session
                            .output
                            .recv_timeout(std::time::Duration::from_millis(100))
                        {
                            interrupted.extend(chunk);
                            if String::from_utf8_lossy(&interrupted).contains("9009") {
                                return Ok(());
                            }
                        }
                    }
                    return Err("Ctrl+C 未中断长命令并恢复交互".into());
                }
            }
        }
        Err("交互终端未返回保留变量后的结果".into())
    }
}
