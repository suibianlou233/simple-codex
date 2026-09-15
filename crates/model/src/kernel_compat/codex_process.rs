//! Simple-owned Codex compatibility layer.
//! Lifecycle of Simple's managed native kernel; not a filesystem/network sandbox.
use std::io;
use std::process::Command;
use std::time::Duration;

use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};

pub(crate) fn spawn(command: Command) -> io::Result<Box<dyn ChildWrapper>> {
    let mut command = CommandWrap::from(tokio::process::Command::from(command));
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    {
        // Keep the hidden-window flag through JobObject's temporary suspension.
        // JobObject assigns before resuming and fails closed if assignment fails.
        let mut flags = process_wrap::tokio::CreationFlags(Default::default());
        flags.0.0 = 0x08000000; // CREATE_NO_WINDOW
        command.wrap(flags).wrap(process_wrap::tokio::JobObject);
    }
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    command.spawn()
}

pub(crate) async fn terminate(child: &mut Box<dyn ChildWrapper>) -> io::Result<()> {
    // On Windows this targets the job even when the direct child already exited.
    // A successful wait for that child alone says nothing about its descendants.
    #[cfg(windows)]
    child.start_kill()?;
    #[cfg(unix)]
    if let Err(error) = child.start_kill() {
        // ESRCH means the entire process group has already exited.
        if error.raw_os_error() != Some(3) {
            return Err(error);
        }
    }
    #[cfg(not(any(windows, unix)))]
    if child.try_wait()?.is_none() {
        child.start_kill()?;
    }
    tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "managed kernel termination not confirmed",
            )
        })??;
    Ok(())
}

#[cfg(all(test, windows))]
#[path = "codex_process_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;

    #[tokio::test]
    async fn termination_reaps_the_managed_process_and_is_repeatable() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 60 & wait"]);
        let mut child = spawn(command).expect("spawn process group");
        terminate(&mut child).await.expect("terminate group");
        assert!(child.try_wait().expect("wait").is_some());
        terminate(&mut child).await.expect("already terminated");
    }
}
