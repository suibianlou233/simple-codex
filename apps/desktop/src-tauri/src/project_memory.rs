//! Desktop routing only. Native memory selection, provenance and history stay
//! with the pinned kernel; old histories are never copied into a new home.
use super::{DesktopError, hash_bytes};
use local_agent_model::CodexProjectMemory;
use std::path::Path;

pub(super) struct ProjectMemoryLocation {
    pub cache_key: String,
    pub scope: CodexProjectMemory,
    pub artifact_home: std::path::PathBuf,
}

impl ProjectMemoryLocation {
    pub fn resolve(
        managed_root: &Path,
        history_name: &str,
        project: &Path,
    ) -> Result<Self, DesktopError> {
        if history_name.len() != 64
            || !history_name
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(DesktopError::UnsafeMemoryPath);
        }
        let project = std::fs::canonicalize(project)?;
        let text = project.to_str().ok_or(DesktopError::UnsafeMemoryPath)?;
        let project_key = hash_bytes(text.as_bytes());
        let managed_root = std::path::absolute(managed_root)?;
        let storage = managed_root
            .join("project-memory")
            .join(&project_key)
            .join(history_name);
        // Inspect parents before children so a known reparse point is rejected
        // before inspecting anything behind it. This is not an atomic sandbox.
        let ancestors = storage.ancestors().collect::<Vec<_>>();
        for ancestor in ancestors.into_iter().rev() {
            match std::fs::symlink_metadata(ancestor) {
                Ok(metadata) => {
                    #[cfg(windows)]
                    let reparse = {
                        use std::os::windows::fs::MetadataExt;
                        metadata.file_attributes() & 0x400 != 0
                    };
                    #[cfg(not(windows))]
                    let reparse = false;
                    if metadata.file_type().is_symlink() || reparse || !metadata.is_dir() {
                        return Err(DesktopError::UnsafeMemoryPath);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(Self {
            cache_key: format!("history:{history_name}:project:{project_key}"),
            scope: CodexProjectMemory::new(&project, &storage)?,
            artifact_home: storage.join("memories"),
        })
    }
}

#[cfg(test)]
#[path = "project_memory_tests.rs"]
mod tests;
