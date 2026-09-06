//! Bounded local inspection, never inference or silent creation of artifacts.
use super::project_memory::ProjectMemoryLocation;
use super::{DesktopError, DesktopRuntime, hash_bytes, parse_task_id, user_visible_path};
use serde::Serialize;
use std::io::Read;
use std::path::Path;

const MAX_FILE_BYTES: u64 = 256 * 1024;

pub(super) struct MemoryInspection {
    view: MemoryView,
    artifact_home: std::path::PathBuf,
}

impl MemoryInspection {
    // Resolve from the persisted task, never a frontend-supplied filesystem path.
    // This may register its history location, but does not launch a kernel.
    pub fn prepare(
        runtime: &mut DesktopRuntime,
        managed_root: &Path,
        task_id: String,
    ) -> Result<Self, DesktopError> {
        let id = parse_task_id(&task_id)?;
        let root = runtime.task_project_root(id)?;
        let default_history = hash_bytes(b"simple-native-history-v2");
        let history = runtime.storage.resolve_codex_history_home(
            &task_id,
            managed_root,
            &default_history,
            local_agent_model::CodexHistoryLayout::legacy_memory().storage_probe(),
        )?;
        let location = ProjectMemoryLocation::resolve(managed_root, &history, &root)?;
        Ok(Self {
            view: MemoryView {
                task_id,
                project_path: user_visible_path(&root),
                legacy_history: history != default_history,
                documents: Vec::new(),
            },
            artifact_home: location.artifact_home,
        })
    }

    pub fn read(mut self) -> MemoryView {
        self.view.documents = read_documents(&self.artifact_home);
        self.view
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryView {
    pub task_id: String,
    pub project_path: String,
    pub legacy_history: bool,
    pub documents: Vec<MemoryDocument>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDocument {
    name: &'static str,
    title: &'static str,
    status: &'static str,
    content: Option<String>,
    hash: Option<String>,
}

pub(super) fn read_documents(root: &Path) -> Vec<MemoryDocument> {
    [
        ("memory_summary.md", "自动载入的摘要"),
        ("MEMORY.md", "长期记忆索引"),
        ("raw_memories.md", "提炼来源记录"),
    ]
    .into_iter()
    .map(|(name, title)| {
        let result = read_one(root, name);
        match result {
            Ok(content) => MemoryDocument {
                name,
                title,
                status: "ready",
                hash: Some(local_agent_tools::hash_bytes(content.as_bytes())),
                content: Some(content),
            },
            Err(status) => MemoryDocument {
                name,
                title,
                status,
                content: None,
                hash: None,
            },
        }
    })
    .collect()
}

fn read_one(root: &Path, name: &str) -> Result<String, &'static str> {
    let metadata = std::fs::symlink_metadata(root).map_err(read_error)?;
    if unsafe_metadata(&metadata) || !metadata.is_dir() {
        return Err("unsafe");
    }
    let path = root.join(name);
    let metadata = std::fs::symlink_metadata(&path).map_err(read_error)?;
    if unsafe_metadata(&metadata) || !metadata.is_file() {
        return Err("unsafe");
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path).map_err(read_error)?;
    let metadata = file.metadata().map_err(read_error)?;
    if unsafe_metadata(&metadata) || !metadata.is_file() {
        return Err("unsafe");
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err("tooLarge");
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("tooLarge");
    }
    String::from_utf8(bytes).map_err(|_| "unreadable")
}

fn read_error(error: std::io::Error) -> &'static str {
    if error.kind() == std::io::ErrorKind::NotFound {
        "missing"
    } else {
        "unreadable"
    }
}

fn unsafe_metadata(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

#[cfg(test)]
#[path = "memory_view_tests.rs"]
mod tests;
