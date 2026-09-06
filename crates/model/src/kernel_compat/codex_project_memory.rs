//! Simple-owned Codex compatibility layer.
//! Typed launch boundary for the native project's generated memory. History
//! ownership and model routing are independent from this artifact directory.
use std::path::{Path, PathBuf};

use crate::CodexKernelError;

#[derive(Debug, Clone)]
pub struct CodexProjectMemory {
    project_root: PathBuf,
    storage_home: PathBuf,
}

impl CodexProjectMemory {
    pub fn new(project_root: &Path, storage_home: &Path) -> Result<Self, CodexKernelError> {
        let project_root =
            std::fs::canonicalize(project_root).map_err(CodexKernelError::InvalidMemoryPath)?;
        if !project_root.is_dir() || !storage_home.is_absolute() {
            return Err(CodexKernelError::InvalidMemoryScope);
        }
        // Refuse unrepresentable paths rather than binding a lossy alias.
        if project_root.to_str().is_none() || storage_home.to_str().is_none() {
            return Err(CodexKernelError::InvalidMemoryScope);
        }
        Ok(Self {
            project_root,
            storage_home: storage_home.to_owned(),
        })
    }

    pub(crate) fn configure_command(
        &self,
        command: &mut std::process::Command,
    ) -> Result<(), CodexKernelError> {
        for (name, path) in [
            ("project_root", &self.project_root),
            ("storage_home", &self.storage_home),
        ] {
            let text = path.to_str().ok_or(CodexKernelError::InvalidMemoryScope)?;
            // JSON strings use escapes accepted by TOML basic strings. Pass a
            // single argv item; do not compose a shell command for Windows.
            let quoted =
                serde_json::to_string(text).map_err(|_| CodexKernelError::InvalidMemoryScope)?;
            command.args(["-c", &format!("memories.project_scope.{name}={quoted}")]);
        }
        Ok(())
    }
}
