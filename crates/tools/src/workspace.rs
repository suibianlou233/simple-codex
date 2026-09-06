use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use similar::TextDiff;
use thiserror::Error;

const MAX_FILE_BYTES: u64 = 1_048_576;
const MAX_LIST_RESULTS: usize = 2_000;
const MAX_SEARCH_RESULTS: usize = 500;
const MAX_INCREMENTAL_EDITS: usize = 128;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("项目根目录无效")]
    InvalidRoot,
    #[error("路径必须是项目内的相对路径")]
    InvalidRelativePath,
    #[error("路径超出了项目范围")]
    PathEscapesWorkspace,
    #[error("自动工具不能访问敏感凭据文件")]
    SensitivePath,
    #[error("路径不存在")]
    NotFound,
    #[error("路径不是普通文件")]
    NotAFile,
    #[error("路径不是文件夹")]
    NotADirectory,
    #[error("文件超过 {maximum} 字节的安全限制")]
    FileTooLarge { maximum: u64 },
    #[error("文件不是有效的 UTF-8 文本")]
    NotText,
    #[error("搜索内容不能为空")]
    EmptyQuery,
    #[error("文件在生成预览后发生了变化")]
    FileChanged,
    #[error("至少需要一项增量修改")]
    EmptyEditSet,
    #[error("增量修改只支持已有文本文件")]
    IncrementalEditRequiresExistingFile,
    #[error("第 {edit_index} 项增量修改的原文本为空")]
    EmptyOldText { edit_index: usize },
    #[error("第 {edit_index} 项增量修改的匹配序号无效；序号从 1 开始")]
    InvalidOccurrence { edit_index: usize },
    #[error("第 {edit_index} 项增量修改没有匹配到文件内容")]
    EditMatchNotFound { edit_index: usize },
    #[error("第 {edit_index} 项增量修改需要唯一匹配，但找到了 {matches} 处")]
    AmbiguousEditMatch { edit_index: usize, matches: usize },
    #[error("第 {first_edit} 项与第 {second_edit} 项增量修改范围重叠")]
    OverlappingEdits {
        first_edit: usize,
        second_edit: usize,
    },
    #[error("增量修改数量超过 {maximum} 项的安全限制")]
    TooManyEdits { maximum: usize },
    #[error("本地文件操作失败：{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContent {
    pub path: String,
    pub content: String,
    pub sha256: String,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WritePreview {
    pub path: String,
    pub original_content: Option<String>,
    pub original_sha256: Option<String>,
    pub new_content: String,
    pub new_sha256: String,
    pub unified_diff: String,
}

/// Selects which match an incremental edit is allowed to replace.
///
/// `Unique` is the safe default: the preview fails when the old text occurs
/// zero or multiple times. Repeated text must use an explicit, one-based
/// occurrence so a model cannot silently modify every matching region.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditMatch {
    #[default]
    Unique,
    Occurrence {
        one_based: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEdit {
    pub old_text: String,
    pub new_text: String,
    #[serde(default)]
    pub match_selection: EditMatch,
}

#[derive(Debug)]
struct LocatedEdit<'a> {
    edit_index: usize,
    start: usize,
    end: usize,
    replacement: &'a str,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
    scope: WorkspaceScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceScope {
    Project,
    System,
}

impl Workspace {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = fs::canonicalize(root).map_err(|_| WorkspaceError::InvalidRoot)?;
        if !root.is_dir() {
            return Err(WorkspaceError::InvalidRoot);
        }
        Ok(Self {
            root,
            scope: WorkspaceScope::Project,
        })
    }

    /// Opens the project as the default working directory while permitting
    /// explicit absolute paths. Callers must expose this only after the user
    /// has selected whole-computer access for the current task.
    pub fn open_system(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let mut workspace = Self::open(root)?;
        workspace.scope = WorkspaceScope::System;
        Ok(workspace)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether this workspace accepts explicit absolute paths outside `root`.
    ///
    /// This is capability metadata for registry construction, not proof that
    /// the operating system sandbox enforces a boundary.
    #[must_use]
    pub const fn allows_outside_workspace(&self) -> bool {
        matches!(self.scope, WorkspaceScope::System)
    }

    pub fn list_files(&self, limit: usize) -> Result<Vec<String>, WorkspaceError> {
        let limit = limit.clamp(1, MAX_LIST_RESULTS);
        let mut files = Vec::new();
        let mut builder = WalkBuilder::new(&self.root);
        builder
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .parents(true)
            .follow_links(false)
            .filter_entry(|entry| !is_ignored_internal_entry(entry.path()));
        let walker = builder.build();
        for entry in walker.filter_map(Result::ok) {
            if files.len() >= limit {
                break;
            }
            let Some(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_file() {
                continue;
            }
            let canonical = fs::canonicalize(entry.path())?;
            if !canonical.starts_with(&self.root) {
                continue;
            }
            let relative = self.relative_display(&canonical)?;
            if is_sensitive_relative(Path::new(&relative)) {
                continue;
            }
            files.push(relative);
        }
        files.sort();
        Ok(files)
    }

    pub fn read_file(&self, relative: &str) -> Result<FileContent, WorkspaceError> {
        self.ensure_input_allowed(relative)?;
        let path = self.resolve_existing(relative)?;
        let canonical_relative = self.ensure_resolved_not_sensitive(&path)?;
        if !path.is_file() {
            return Err(WorkspaceError::NotAFile);
        }
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err(WorkspaceError::FileTooLarge {
                maximum: MAX_FILE_BYTES,
            });
        }
        let bytes = fs::read(&path)?;
        let content = String::from_utf8(bytes.clone()).map_err(|_| WorkspaceError::NotText)?;
        Ok(FileContent {
            path: canonical_relative,
            content,
            sha256: hash_bytes(&bytes),
            byte_len: metadata.len(),
        })
    }

    pub fn search_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchMatch>, WorkspaceError> {
        if query.is_empty() {
            return Err(WorkspaceError::EmptyQuery);
        }
        let limit = limit.clamp(1, MAX_SEARCH_RESULTS);
        let mut matches = Vec::new();
        for relative in self.list_files(MAX_LIST_RESULTS)? {
            if matches.len() >= limit {
                break;
            }
            let Ok(file) = self.read_file(&relative) else {
                continue;
            };
            for (line_index, line) in file.content.lines().enumerate() {
                let mut offset = 0;
                while let Some(found) = line[offset..].find(query) {
                    let byte_column = offset + found;
                    let column = line[..byte_column].chars().count() + 1;
                    matches.push(SearchMatch {
                        path: file.path.clone(),
                        line: line_index + 1,
                        column,
                        snippet: truncate_chars(line.trim(), 240),
                    });
                    if matches.len() >= limit {
                        return Ok(matches);
                    }
                    offset = byte_column + query.len();
                }
            }
        }
        Ok(matches)
    }

    pub fn preview_write(
        &self,
        relative: &str,
        new_content: String,
        expected_sha256: Option<&str>,
    ) -> Result<WritePreview, WorkspaceError> {
        self.ensure_input_allowed(relative)?;
        if u64::try_from(new_content.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
            return Err(WorkspaceError::FileTooLarge {
                maximum: MAX_FILE_BYTES,
            });
        }
        let candidate = self.candidate_path(relative)?;
        let (path, original_content, original_sha256) = if candidate.exists() {
            let file = self.read_file(relative)?;
            (
                self.resolve_existing(relative)?,
                Some(file.content),
                Some(file.sha256),
            )
        } else {
            (self.resolve_new(relative)?, None, None)
        };
        if expected_sha256 != original_sha256.as_deref() {
            return Err(WorkspaceError::FileChanged);
        }
        let old = original_content.as_deref().unwrap_or("");
        let relative_path = self.ensure_resolved_not_sensitive(&path)?;
        let unified_diff = TextDiff::from_lines(old, &new_content)
            .unified_diff()
            .context_radius(3)
            .header(&format!("a/{relative_path}"), &format!("b/{relative_path}"))
            .to_string();
        Ok(WritePreview {
            path: relative_path,
            original_content,
            original_sha256,
            new_sha256: hash_bytes(new_content.as_bytes()),
            new_content,
            unified_diff,
        })
    }

    /// Creates a normal hash-checked write preview from precise text edits.
    ///
    /// Every match is located in the same original file snapshot. This makes
    /// the request deterministic and lets us reject edits whose source ranges
    /// overlap before any content is changed. New files intentionally remain a
    /// full-write operation because they have no old text to anchor edits to.
    pub fn preview_edits(
        &self,
        relative: &str,
        expected_sha256: &str,
        edits: &[TextEdit],
    ) -> Result<WritePreview, WorkspaceError> {
        self.ensure_input_allowed(relative)?;
        if edits.is_empty() {
            return Err(WorkspaceError::EmptyEditSet);
        }
        if edits.len() > MAX_INCREMENTAL_EDITS {
            return Err(WorkspaceError::TooManyEdits {
                maximum: MAX_INCREMENTAL_EDITS,
            });
        }

        let file = match self.read_file(relative) {
            Ok(file) => file,
            Err(WorkspaceError::NotFound) => {
                return Err(WorkspaceError::IncrementalEditRequiresExistingFile);
            }
            Err(error) => return Err(error),
        };
        if file.sha256 != expected_sha256 {
            return Err(WorkspaceError::FileChanged);
        }

        let mut located = Vec::with_capacity(edits.len());
        for (zero_based_index, edit) in edits.iter().enumerate() {
            let edit_index = zero_based_index + 1;
            if edit.old_text.is_empty() {
                return Err(WorkspaceError::EmptyOldText { edit_index });
            }
            let matches = file
                .content
                .match_indices(&edit.old_text)
                .map(|(start, _)| start)
                .collect::<Vec<_>>();
            let start = match edit.match_selection {
                EditMatch::Unique => match matches.as_slice() {
                    [] => return Err(WorkspaceError::EditMatchNotFound { edit_index }),
                    [start] => *start,
                    _ => {
                        return Err(WorkspaceError::AmbiguousEditMatch {
                            edit_index,
                            matches: matches.len(),
                        });
                    }
                },
                EditMatch::Occurrence { one_based: 0 } => {
                    return Err(WorkspaceError::InvalidOccurrence { edit_index });
                }
                EditMatch::Occurrence { one_based } => *matches
                    .get(one_based - 1)
                    .ok_or(WorkspaceError::EditMatchNotFound { edit_index })?,
            };
            located.push(LocatedEdit {
                edit_index,
                start,
                end: start + edit.old_text.len(),
                replacement: &edit.new_text,
            });
        }

        located.sort_by_key(|edit| (edit.start, edit.end));
        for pair in located.windows(2) {
            if pair[1].start < pair[0].end {
                return Err(WorkspaceError::OverlappingEdits {
                    first_edit: pair[0].edit_index,
                    second_edit: pair[1].edit_index,
                });
            }
        }

        let removed_bytes = located
            .iter()
            .map(|edit| edit.end - edit.start)
            .sum::<usize>();
        let added_bytes = located
            .iter()
            .map(|edit| edit.replacement.len())
            .try_fold(0_usize, usize::checked_add)
            .ok_or(WorkspaceError::FileTooLarge {
                maximum: MAX_FILE_BYTES,
            })?;
        let projected_bytes = file
            .content
            .len()
            .checked_sub(removed_bytes)
            .and_then(|length| length.checked_add(added_bytes))
            .ok_or(WorkspaceError::FileTooLarge {
                maximum: MAX_FILE_BYTES,
            })?;
        if u64::try_from(projected_bytes).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
            return Err(WorkspaceError::FileTooLarge {
                maximum: MAX_FILE_BYTES,
            });
        }

        let mut new_content = file.content;
        for edit in located.into_iter().rev() {
            new_content.replace_range(edit.start..edit.end, edit.replacement);
        }
        self.preview_write(relative, new_content, Some(expected_sha256))
    }

    pub fn apply_write(&self, preview: &WritePreview) -> Result<(), WorkspaceError> {
        self.ensure_input_allowed(&preview.path)?;
        let target = self.candidate_path(&preview.path)?;
        let current = if target.exists() {
            Some(self.read_file(&preview.path)?.sha256)
        } else {
            None
        };
        if current != preview.original_sha256 {
            return Err(WorkspaceError::FileChanged);
        }
        let target = self.create_missing_parent_directories(&preview.path)?;
        self.ensure_resolved_not_sensitive(&target)?;
        atomic_replace(&target, preview.new_content.as_bytes())
    }

    pub fn undo_write(&self, preview: &WritePreview) -> Result<(), WorkspaceError> {
        self.ensure_input_allowed(&preview.path)?;
        let target = self.candidate_path(&preview.path)?;
        if !target.exists() || self.read_file(&preview.path)?.sha256 != preview.new_sha256 {
            return Err(WorkspaceError::FileChanged);
        }
        if let Some(original) = &preview.original_content {
            atomic_replace(&target, original.as_bytes())
        } else {
            fs::remove_file(target)?;
            Ok(())
        }
    }

    pub(crate) fn resolve_directory(&self, relative: &str) -> Result<PathBuf, WorkspaceError> {
        if !relative.trim().is_empty() && relative != "." {
            self.ensure_input_allowed(relative)?;
        }
        let path = if relative.trim().is_empty() || relative == "." {
            self.root.clone()
        } else {
            self.resolve_existing(relative)?
        };
        self.ensure_resolved_not_sensitive(&path)?;
        if !path.is_dir() {
            return Err(WorkspaceError::NotADirectory);
        }
        Ok(path)
    }

    fn resolve_existing(&self, relative: &str) -> Result<PathBuf, WorkspaceError> {
        let candidate = self.candidate_path(relative)?;
        let canonical = fs::canonicalize(candidate).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                WorkspaceError::NotFound
            } else {
                WorkspaceError::Io(error)
            }
        })?;
        if self.scope == WorkspaceScope::Project && !canonical.starts_with(&self.root) {
            return Err(WorkspaceError::PathEscapesWorkspace);
        }
        Ok(canonical)
    }

    fn resolve_new(&self, relative: &str) -> Result<PathBuf, WorkspaceError> {
        let target = self.candidate_path(relative)?;
        let parent = target.parent().ok_or(WorkspaceError::InvalidRelativePath)?;
        let mut existing_parent = parent;
        while !existing_parent.exists() {
            existing_parent = existing_parent
                .parent()
                .ok_or(WorkspaceError::InvalidRelativePath)?;
        }
        let canonical_existing_parent = fs::canonicalize(existing_parent)?;
        if !canonical_existing_parent.is_dir() {
            return Err(WorkspaceError::NotADirectory);
        }
        if self.scope == WorkspaceScope::Project
            && !canonical_existing_parent.starts_with(&self.root)
        {
            return Err(WorkspaceError::PathEscapesWorkspace);
        }
        let missing_parent = parent
            .strip_prefix(existing_parent)
            .map_err(|_| WorkspaceError::PathEscapesWorkspace)?;
        let canonical_parent = canonical_existing_parent.join(missing_parent);
        if self.scope == WorkspaceScope::Project && !canonical_parent.starts_with(&self.root) {
            return Err(WorkspaceError::PathEscapesWorkspace);
        }
        let name = target
            .file_name()
            .ok_or(WorkspaceError::InvalidRelativePath)?;
        Ok(canonical_parent.join(name))
    }

    fn create_missing_parent_directories(&self, relative: &str) -> Result<PathBuf, WorkspaceError> {
        let target = self.candidate_path(relative)?;
        if self.scope == WorkspaceScope::System && Path::new(relative).is_absolute() {
            let parent = target.parent().ok_or(WorkspaceError::InvalidRelativePath)?;
            fs::create_dir_all(parent)?;
            let parent = fs::canonicalize(parent)?;
            let name = target
                .file_name()
                .ok_or(WorkspaceError::InvalidRelativePath)?;
            return Ok(parent.join(name));
        }
        let relative = normalize_relative(relative)?;
        let parent = relative
            .parent()
            .ok_or(WorkspaceError::InvalidRelativePath)?;
        let name = relative
            .file_name()
            .ok_or(WorkspaceError::InvalidRelativePath)?;
        let mut canonical_parent = self.root.clone();

        for component in parent.components() {
            let Component::Normal(component) = component else {
                return Err(WorkspaceError::InvalidRelativePath);
            };
            let candidate = canonical_parent.join(component);
            match fs::symlink_metadata(&candidate) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&candidate)?;
                }
                Err(error) => return Err(error.into()),
            }
            canonical_parent = fs::canonicalize(&candidate)?;
            if !canonical_parent.starts_with(&self.root) {
                return Err(WorkspaceError::PathEscapesWorkspace);
            }
            self.ensure_resolved_not_sensitive(&canonical_parent)?;
            if !canonical_parent.is_dir() {
                return Err(WorkspaceError::NotADirectory);
            }
        }

        Ok(canonical_parent.join(name))
    }

    fn candidate_path(&self, value: &str) -> Result<PathBuf, WorkspaceError> {
        let path = Path::new(value);
        if path.is_absolute() {
            if self.scope == WorkspaceScope::System {
                return Ok(path.to_path_buf());
            }
            return Err(WorkspaceError::InvalidRelativePath);
        }
        Ok(self.root.join(normalize_relative(value)?))
    }

    fn ensure_input_allowed(&self, value: &str) -> Result<(), WorkspaceError> {
        if self.scope == WorkspaceScope::Project || !Path::new(value).is_absolute() {
            ensure_not_sensitive(value)?;
        }
        Ok(())
    }

    fn relative_display(&self, path: &Path) -> Result<String, WorkspaceError> {
        if self.scope == WorkspaceScope::System && !path.starts_with(&self.root) {
            return Ok(path.to_string_lossy().replace('\\', "/"));
        }
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| WorkspaceError::PathEscapesWorkspace)?;
        Ok(relative.to_string_lossy().replace('\\', "/"))
    }

    fn ensure_resolved_not_sensitive(&self, path: &Path) -> Result<String, WorkspaceError> {
        let relative = self.relative_display(path)?;
        if relative.is_empty() {
            return Ok(relative);
        }
        self.ensure_input_allowed(&relative)?;
        Ok(relative)
    }
}

#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn normalize_relative(value: &str) -> Result<PathBuf, WorkspaceError> {
    let path = Path::new(value);
    if path.is_absolute() || value.trim().is_empty() {
        return Err(WorkspaceError::InvalidRelativePath);
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(WorkspaceError::InvalidRelativePath);
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(WorkspaceError::InvalidRelativePath);
    }
    Ok(normalized)
}

fn ensure_not_sensitive(value: &str) -> Result<(), WorkspaceError> {
    let normalized = normalize_relative(value)?;
    if is_sensitive_relative(&normalized) {
        Err(WorkspaceError::SensitivePath)
    } else {
        Ok(())
    }
}

fn is_ignored_internal_entry(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | ".hg" | ".svn"))
}

pub(crate) fn is_sensitive_relative(path: &Path) -> bool {
    if path.components().any(|component| {
        component.as_os_str().to_str().is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                ".ssh" | ".aws" | ".azure"
            )
        })
    }) {
        return true;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let name = name.to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || matches!(
            name.as_str(),
            "id_rsa"
                | "id_ed25519"
                | "credentials"
                | "credentials.json"
                | ".npmrc"
                | ".pypirc"
                | ".netrc"
        )
        || name.ends_with(".pem")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.ends_with(".key")
}

fn atomic_replace(target: &Path, bytes: &[u8]) -> Result<(), WorkspaceError> {
    let parent = target.parent().ok_or(WorkspaceError::InvalidRelativePath)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WorkspaceError::InvalidRelativePath)?
        .as_nanos();
    let temporary = parent.join(format!(".local-agent-{}-{nonce}.tmp", std::process::id()));
    let backup = parent.join(format!(".local-agent-{}-{nonce}.bak", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    drop(file);
    if target.exists() {
        fs::rename(target, &backup)?;
        if let Err(error) = fs::rename(&temporary, target) {
            let _ = fs::rename(&backup, target);
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        fs::remove_file(backup)?;
    } else {
        fs::rename(temporary, target)?;
    }
    Ok(())
}

fn truncate_chars(value: &str, maximum: usize) -> String {
    if value.chars().count() <= maximum {
        value.to_owned()
    } else {
        format!("{}…", value.chars().take(maximum).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{EditMatch, TextEdit, Workspace, WorkspaceError, WritePreview, hash_bytes};

    fn unique_edit(old_text: &str, new_text: &str) -> TextEdit {
        TextEdit {
            old_text: old_text.to_owned(),
            new_text: new_text.to_owned(),
            match_selection: EditMatch::Unique,
        }
    }

    #[test]
    fn common_workspace_errors_are_presented_in_chinese() {
        assert_eq!(WorkspaceError::NotFound.to_string(), "路径不存在");
        assert_eq!(
            WorkspaceError::InvalidRelativePath.to_string(),
            "路径必须是项目内的相对路径"
        );
        assert_eq!(
            WorkspaceError::SensitivePath.to_string(),
            "自动工具不能访问敏感凭据文件"
        );
    }

    #[cfg(windows)]
    fn link_file(original: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(original, link)
    }

    #[cfg(not(windows))]
    fn link_file(original: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(original, link)
    }

    #[cfg(windows)]
    fn link_directory(original: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(original, link)
    }

    #[cfg(not(windows))]
    fn link_directory(original: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(original, link)
    }

    #[test]
    fn read_search_preview_apply_and_undo_stay_in_workspace() {
        let directory = tempdir().expect("workspace should be created");
        fs::write(directory.path().join("demo.txt"), "alpha\nbeta\n")
            .expect("fixture should be written");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let file = workspace.read_file("demo.txt").expect("file should read");
        assert_eq!(file.content, "alpha\nbeta\n");
        assert_eq!(
            workspace
                .search_text("beta", 10)
                .expect("search should work")[0]
                .line,
            2
        );

        let preview = workspace
            .preview_write("demo.txt", "alpha\ngamma\n".to_owned(), Some(&file.sha256))
            .expect("write should preview");
        assert!(preview.unified_diff.contains("-beta"));
        assert!(preview.unified_diff.contains("+gamma"));
        workspace
            .apply_write(&preview)
            .expect("preview should apply");
        assert_eq!(
            workspace
                .read_file("demo.txt")
                .expect("file should read")
                .content,
            "alpha\ngamma\n"
        );
        workspace.undo_write(&preview).expect("write should undo");
        assert_eq!(
            workspace
                .read_file("demo.txt")
                .expect("file should read")
                .content,
            "alpha\nbeta\n"
        );
    }

    #[test]
    fn system_scope_accepts_explicit_absolute_paths_outside_project() {
        let directory = tempdir().expect("temporary root should be created");
        let project = directory.path().join("project");
        let outside = directory.path().join("outside");
        fs::create_dir_all(&project).expect("project should be created");
        fs::create_dir_all(&outside).expect("outside directory should be created");
        let outside_file = outside.join("note.txt");
        fs::write(&outside_file, "before").expect("outside fixture should be written");
        let absolute = outside_file.to_string_lossy().to_string();

        let project_workspace = Workspace::open(&project).expect("project workspace should open");
        assert!(matches!(
            project_workspace.read_file(&absolute),
            Err(WorkspaceError::InvalidRelativePath)
        ));

        let system_workspace =
            Workspace::open_system(&project).expect("system workspace should open");
        let original = system_workspace
            .read_file(&absolute)
            .expect("absolute file should read");
        let preview = system_workspace
            .preview_write(&absolute, "after".to_owned(), Some(&original.sha256))
            .expect("absolute write should preview");
        system_workspace
            .apply_write(&preview)
            .expect("absolute write should apply");
        assert_eq!(
            fs::read_to_string(&outside_file).expect("outside file should remain readable"),
            "after"
        );
        system_workspace
            .undo_write(&preview)
            .expect("absolute write should undo");
        assert_eq!(
            fs::read_to_string(outside_file).expect("outside file should remain readable"),
            "before"
        );
    }

    #[test]
    fn approved_new_file_creates_missing_parent_directories() {
        let directory = tempdir().expect("workspace should be created");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let nested_directory = directory.path().join("web").join("templates");

        let preview = workspace
            .preview_write(
                "web/templates/index.html",
                "<h1>Simple</h1>\n".to_owned(),
                None,
            )
            .expect("nested new file should preview without creating directories");
        assert!(!nested_directory.exists());

        workspace
            .apply_write(&preview)
            .expect("approved nested new file should create its parents");
        assert_eq!(
            fs::read_to_string(nested_directory.join("index.html"))
                .expect("nested file should be readable"),
            "<h1>Simple</h1>\n"
        );

        workspace
            .undo_write(&preview)
            .expect("nested new file should remain undoable");
        assert!(!nested_directory.join("index.html").exists());
    }

    #[test]
    fn parent_redirect_after_preview_cannot_escape_workspace() {
        let directory = tempdir().expect("workspace should be created");
        let outside = tempdir().expect("outside directory should be created");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let preview = workspace
            .preview_write("generated/app.py", "print('safe')\n".to_owned(), None)
            .expect("missing parent should be accepted during preview");

        if link_directory(outside.path(), &directory.path().join("generated")).is_err() {
            return;
        }
        assert!(matches!(
            workspace.apply_write(&preview),
            Err(WorkspaceError::PathEscapesWorkspace)
        ));
        assert!(!outside.path().join("app.py").exists());
    }

    #[test]
    fn traversal_and_sensitive_files_are_rejected() {
        let directory = tempdir().expect("workspace should be created");
        fs::write(directory.path().join(".env"), "TOKEN=secret")
            .expect("fixture should be written");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        assert!(matches!(
            workspace.read_file("../outside.txt"),
            Err(WorkspaceError::InvalidRelativePath)
        ));
        assert!(matches!(
            workspace.read_file(".env"),
            Err(WorkspaceError::SensitivePath)
        ));
        assert!(
            !workspace
                .list_files(100)
                .expect("listing should work")
                .contains(&".env".to_owned())
        );
    }

    #[test]
    fn symlink_aliases_cannot_bypass_sensitive_path_checks() {
        let directory = tempdir().expect("workspace should be created");
        let sensitive_file = directory.path().join(".env");
        fs::write(&sensitive_file, "TOKEN=secret").expect("fixture should be written");
        if link_file(&sensitive_file, &directory.path().join("safe.txt")).is_err() {
            return;
        }
        let sensitive_directory = directory.path().join(".ssh");
        fs::create_dir(&sensitive_directory).expect("sensitive directory should be created");
        if link_directory(
            &sensitive_directory,
            &directory.path().join("safe-directory"),
        )
        .is_err()
        {
            return;
        }
        let workspace = Workspace::open(directory.path()).expect("workspace should open");

        assert!(matches!(
            workspace.read_file("safe.txt"),
            Err(WorkspaceError::SensitivePath)
        ));
        assert!(matches!(
            workspace.preview_write("safe-directory/new.txt", "data".to_owned(), None),
            Err(WorkspaceError::SensitivePath)
        ));
        assert!(matches!(
            workspace.resolve_directory("safe-directory"),
            Err(WorkspaceError::SensitivePath)
        ));
        let forged_preview = WritePreview {
            path: "safe-directory/new.txt".to_owned(),
            original_content: None,
            original_sha256: None,
            new_content: "data".to_owned(),
            new_sha256: hash_bytes(b"data"),
            unified_diff: String::new(),
        };
        assert!(matches!(
            workspace.apply_write(&forged_preview),
            Err(WorkspaceError::SensitivePath)
        ));
    }

    #[test]
    fn changed_file_cannot_apply_stale_preview() {
        let directory = tempdir().expect("workspace should be created");
        let path = directory.path().join("demo.txt");
        fs::write(&path, "one").expect("fixture should be written");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let original = workspace.read_file("demo.txt").expect("file should read");
        let preview = workspace
            .preview_write("demo.txt", "two".to_owned(), Some(&original.sha256))
            .expect("write should preview");
        fs::write(path, "changed elsewhere").expect("external edit should be written");
        assert!(matches!(
            workspace.apply_write(&preview),
            Err(WorkspaceError::FileChanged)
        ));
    }

    #[test]
    fn incremental_edits_support_utf8_and_multiple_precise_changes() {
        let directory = tempdir().expect("workspace should be created");
        fs::write(
            directory.path().join("问候.rs"),
            "fn main() {\n    let message = \"你好\";\n    println!(\"{message}\");\n}\n",
        )
        .expect("fixture should be written");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let original = workspace.read_file("问候.rs").expect("file should read");
        let preview = workspace
            .preview_edits(
                "问候.rs",
                &original.sha256,
                &[
                    unique_edit("let message = \"你好\"", "let message = \"你好，世界\""),
                    unique_edit("println!(\"{message}\")", "eprintln!(\"{message}\")"),
                ],
            )
            .expect("precise edits should preview");

        assert!(preview.new_content.contains("你好，世界"));
        assert!(preview.new_content.contains("eprintln!"));
        assert!(
            preview
                .unified_diff
                .contains("-    let message = \"你好\";")
        );
        workspace
            .apply_write(&preview)
            .expect("incremental preview should reuse normal apply");
        workspace
            .undo_write(&preview)
            .expect("incremental preview should reuse normal undo");
        assert_eq!(
            workspace
                .read_file("问候.rs")
                .expect("file should read")
                .content,
            original.content
        );
    }

    #[test]
    fn repeated_text_requires_an_explicit_one_based_occurrence() {
        let directory = tempdir().expect("workspace should be created");
        fs::write(directory.path().join("demo.txt"), "same\nmiddle\nsame\n")
            .expect("fixture should be written");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let original = workspace.read_file("demo.txt").expect("file should read");

        assert!(matches!(
            workspace.preview_edits(
                "demo.txt",
                &original.sha256,
                &[unique_edit("same", "changed")]
            ),
            Err(WorkspaceError::AmbiguousEditMatch {
                edit_index: 1,
                matches: 2
            })
        ));

        let preview = workspace
            .preview_edits(
                "demo.txt",
                &original.sha256,
                &[TextEdit {
                    old_text: "same".to_owned(),
                    new_text: "changed".to_owned(),
                    match_selection: EditMatch::Occurrence { one_based: 2 },
                }],
            )
            .expect("selected occurrence should preview");
        assert_eq!(preview.new_content, "same\nmiddle\nchanged\n");
    }

    #[test]
    fn empty_missing_and_overlapping_edits_are_rejected() {
        let directory = tempdir().expect("workspace should be created");
        fs::write(directory.path().join("demo.txt"), "abcdef").expect("fixture should be written");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let original = workspace.read_file("demo.txt").expect("file should read");

        assert!(matches!(
            workspace.preview_edits("demo.txt", &original.sha256, &[]),
            Err(WorkspaceError::EmptyEditSet)
        ));
        assert!(matches!(
            workspace.preview_edits("demo.txt", &original.sha256, &[unique_edit("", "content")]),
            Err(WorkspaceError::EmptyOldText { edit_index: 1 })
        ));
        assert!(matches!(
            workspace.preview_edits(
                "demo.txt",
                &original.sha256,
                &[unique_edit("missing", "content")]
            ),
            Err(WorkspaceError::EditMatchNotFound { edit_index: 1 })
        ));
        assert!(matches!(
            workspace.preview_edits(
                "demo.txt",
                &original.sha256,
                &[unique_edit("bcd", "first"), unique_edit("cde", "second")]
            ),
            Err(WorkspaceError::OverlappingEdits {
                first_edit: 1,
                second_edit: 2
            })
        ));
    }

    #[test]
    fn incremental_edits_reject_new_sensitive_and_out_of_bounds_paths() {
        let directory = tempdir().expect("workspace should be created");
        fs::write(directory.path().join(".env"), "TOKEN=secret")
            .expect("fixture should be written");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let edits = [unique_edit("old", "new")];

        assert!(matches!(
            workspace.preview_edits("new.txt", "unused", &edits),
            Err(WorkspaceError::IncrementalEditRequiresExistingFile)
        ));
        assert!(matches!(
            workspace.preview_edits(".env", "unused", &edits),
            Err(WorkspaceError::SensitivePath)
        ));
        assert!(matches!(
            workspace.preview_edits("../outside.txt", "unused", &edits),
            Err(WorkspaceError::InvalidRelativePath)
        ));
    }

    #[test]
    fn incremental_preview_keeps_stale_hash_protection() {
        let directory = tempdir().expect("workspace should be created");
        let path = directory.path().join("demo.txt");
        fs::write(&path, "before").expect("fixture should be written");
        let workspace = Workspace::open(directory.path()).expect("workspace should open");
        let original = workspace.read_file("demo.txt").expect("file should read");
        let preview = workspace
            .preview_edits(
                "demo.txt",
                &original.sha256,
                &[unique_edit("before", "after")],
            )
            .expect("edit should preview");
        fs::write(path, "user change").expect("external edit should be written");

        assert!(matches!(
            workspace.apply_write(&preview),
            Err(WorkspaceError::FileChanged)
        ));
    }
}
