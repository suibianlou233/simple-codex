//! Cooperative project ownership across this user's Simple data directories.
//! A live OS handle owns the lease; stale files are never treated as live owners.
use super::*;

pub(super) struct ProjectLease {
    _files: Vec<fs::File>,
}

impl ProjectLease {
    pub(super) fn acquire_instance(data_directory: &Path) -> Result<Self, DesktopError> {
        let identity = fs::canonicalize(data_directory)?;
        Self::acquire_key("instance", &identity, data_directory).map_err(|error| match error {
            DesktopError::ProjectBusy => DesktopError::DesktopAlreadyRunning,
            error => error,
        })
    }

    pub(super) fn acquire(root: &Path, data_directory: &Path) -> Result<Self, DesktopError> {
        // Use the OS user-data location, never a per-installation/model setting.
        // Keep this namespace stable across versions; changing it splits owners.
        let shared = dirs::data_local_dir()
            .ok_or(DesktopError::InvalidStoredPath)?
            .join("simple-coordination");
        Self::acquire_project_in(root, data_directory, &shared)
    }

    fn acquire_project_in(
        root: &Path,
        data_directory: &Path,
        shared: &Path,
    ) -> Result<Self, DesktopError> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(DesktopError::InvalidStoredPath);
        }
        let mut shared_lease = Self::acquire_key("project", &root, shared)?;
        let shared_identity = fs::canonicalize(shared)?;
        if fs::canonicalize(data_directory).is_ok_and(|data| data == shared_identity) {
            return Ok(shared_lease);
        }
        // Retain the old data-directory lock for cooperating older instances.
        // Any second-lock error drops the shared handle; never fall back unlocked.
        let mut local_lease = Self::acquire_key("project", &root, data_directory)?;
        shared_lease._files.append(&mut local_lease._files);
        Ok(shared_lease)
    }

    fn acquire_key(kind: &str, root: &Path, data_directory: &Path) -> Result<Self, DesktopError> {
        let directory = data_directory.join("review-locks");
        fs::create_dir_all(&directory)?;
        let directory = fs::canonicalize(directory)?;
        let key = hash_bytes(encode_path_identity(root).as_bytes());
        let path = directory.join(format!("{kind}-{key}.lock"));
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Do not allow a live lock's path to be removed/replaced.
            options.share_mode(3).custom_flags(0x00200000);
        }
        let file = options.open(&path)?;
        if !file.metadata()?.is_file()
            || fs::symlink_metadata(&path)?.file_type().is_symlink()
            || fs::canonicalize(&path)? != path
        {
            return Err(DesktopError::InvalidStoredPath);
        }
        match file.try_lock() {
            Ok(()) => Ok(Self { _files: vec![file] }),
            Err(fs::TryLockError::WouldBlock) => Err(DesktopError::ProjectBusy),
            Err(fs::TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, Write};

    #[test]
    fn different_data_directories_cannot_own_the_same_project() {
        let temp = tempfile::tempdir().expect("fixture");
        let project = temp.path().join("中文 project");
        fs::create_dir(&project).expect("project");
        let first_data = temp.path().join("profile-a");
        let second_data = temp.path().join("profile-b");
        let first = ProjectLease::acquire(&project, &first_data).expect("first owner");
        assert!(matches!(
            ProjectLease::acquire(&project, &second_data),
            Err(DesktopError::ProjectBusy)
        ));
        drop(first);
        let _second = ProjectLease::acquire(&project, &second_data).expect("owner released");
    }

    #[test]
    fn shared_and_local_registry_aliases_do_not_lock_the_same_file_twice() {
        let temp = tempfile::tempdir().expect("fixture");
        let data = temp.path().join("registry");
        let _owner = ProjectLease::acquire_project_in(temp.path(), &data.join("."), &data)
            .expect("one handle for same registry");
        assert!(matches!(
            ProjectLease::acquire_project_in(temp.path(), &data, &data),
            Err(DesktopError::ProjectBusy)
        ));
    }

    #[test]
    fn old_local_lock_is_respected_and_failed_acquisition_releases_shared_lock() {
        let temp = tempfile::tempdir().expect("fixture");
        let project = fs::canonicalize(temp.path()).expect("root");
        let data = temp.path().join("legacy-data");
        let legacy = ProjectLease::acquire_key("project", &project, &data).expect("old owner");
        assert!(matches!(
            ProjectLease::acquire(&project, &data),
            Err(DesktopError::ProjectBusy)
        ));
        let other = ProjectLease::acquire(&project, &temp.path().join("other-data"))
            .expect("shared handle was released");
        drop(other);
        drop(legacy);
        let _owner = ProjectLease::acquire(&project, &data).expect("both locks available");
    }

    #[test]
    fn shared_registry_failure_does_not_fall_back_to_a_local_only_lock() {
        let temp = tempfile::tempdir().expect("fixture");
        let registry = temp.path().join("not-a-directory");
        fs::write(&registry, b"unchanged").expect("registry fault");
        let data = temp.path().join("unused-data");
        assert!(ProjectLease::acquire_project_in(temp.path(), &data, &registry).is_err());
        assert!(!data.exists());
        assert_eq!(fs::read(&registry).expect("preserved"), b"unchanged");
    }

    #[cfg(windows)]
    #[test]
    fn windows_case_and_dot_aliases_share_the_project_lock() {
        let temp = tempfile::tempdir().expect("fixture");
        let root = temp.path().join("MiXeD 中文");
        fs::create_dir(&root).expect("project");
        let _owner = ProjectLease::acquire(&root, &temp.path().join("first")).expect("owner");
        let alias = temp.path().join("mixed 中文").join(".");
        assert!(matches!(
            ProjectLease::acquire(&alias, &temp.path().join("second")),
            Err(DesktopError::ProjectBusy)
        ));
    }

    #[test]
    fn separate_handles_conflict_and_release_does_not_require_deleting_lock_files() {
        let temp = tempfile::tempdir().expect("fixture");
        let project = temp.path().join("project");
        fs::create_dir(&project).expect("project");
        let data = temp.path().join("data");
        let lease = ProjectLease::acquire(&project, &data).expect("first owner");
        assert!(matches!(
            ProjectLease::acquire(&project, &data),
            Err(DesktopError::ProjectBusy)
        ));
        let second_project = temp.path().join("second");
        fs::create_dir(&second_project).expect("second project");
        let _other = ProjectLease::acquire(&second_project, &data).expect("independent project");
        drop(lease);
        let _reopened = ProjectLease::acquire(&project, &data).expect("released owner");
        assert_eq!(
            fs::read_dir(data.join("review-locks"))
                .expect("persistent lock files")
                .count(),
            2
        );
    }

    #[test]
    fn instance_lease_prevents_a_second_startup_from_recovering_live_tasks() {
        let temp = tempfile::tempdir().expect("fixture");
        let lease = ProjectLease::acquire_instance(temp.path()).expect("first instance");
        let database = temp.path().join("data.db");
        fs::write(&database, "not a database").expect("database access tripwire");
        let error = match DesktopState::open(&database, Err("test kernel not selected".into())) {
            Ok(_) => panic!("second instance must not open"),
            Err(error) => error,
        };
        assert_eq!(error, DesktopError::DesktopAlreadyRunning.to_string());
        assert!(matches!(
            ProjectLease::acquire_instance(temp.path()),
            Err(DesktopError::DesktopAlreadyRunning)
        ));
        drop(lease);
        let _next = ProjectLease::acquire_instance(temp.path()).expect("next startup");
    }

    #[test]
    #[ignore = "subprocess helper"]
    fn lease_child() {
        let root = std::env::var_os("SIMPLE_TEST_LEASE_PROJECT").expect("project fixture");
        let data = std::env::var_os("SIMPLE_TEST_LEASE_DATA").expect("data fixture");
        let _lease =
            ProjectLease::acquire(Path::new(&root), Path::new(&data)).expect("child lease");
        println!("LEASE_READY");
        std::io::stdout().flush().expect("flush handshake");
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .expect("wait for parent");
    }

    #[test]
    fn actual_process_exit_releases_lease_without_stale_file_cleanup() {
        let temp = tempfile::tempdir().expect("fixture");
        let project = temp.path().join("project");
        fs::create_dir(&project).expect("project");
        let data = temp.path().join("data");
        let child = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "runtime::project_lease::tests::lease_child",
                "--ignored",
                "--nocapture",
            ])
            .env("SIMPLE_TEST_LEASE_PROJECT", &project)
            .env("SIMPLE_TEST_LEASE_DATA", &data)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("child");
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = ChildGuard(child);
        let output = child.0.stdout.take().expect("child stdout");
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in std::io::BufReader::new(output)
                .lines()
                .map_while(Result::ok)
            {
                if line.contains("LEASE_READY") {
                    let _ = tx.send(());
                    break;
                }
            }
        });
        rx.recv_timeout(Duration::from_secs(10))
            .expect("live child handshake");
        reader.join().expect("reader");
        assert!(matches!(
            ProjectLease::acquire(&project, &data),
            Err(DesktopError::ProjectBusy)
        ));
        let other_data = temp.path().join("other-installation");
        assert!(matches!(
            ProjectLease::acquire(&project, &other_data),
            Err(DesktopError::ProjectBusy)
        ));
        child.0.kill().expect("simulate fixture process crash");
        child.0.wait().expect("child exited");
        let _lease = ProjectLease::acquire(&project, &other_data)
            .expect("OS releases shared lock after crash");
    }
}
