//! Persist product history associations. The version adapter supplies the native
//! database filename and read-only existence query; never copy or write rollouts.
use crate::{Storage, StorageError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::Duration;

impl Storage {
    /// Select a durable managed home for a task. Existing native threads must be
    /// found in exactly one complete history store; missing/ambiguous data is an
    /// error, never permission to start an empty replacement conversation.
    pub fn resolve_codex_history_home(
        &mut self,
        task_id: &str,
        managed_root: &Path,
        new_home_name: &str,
        layout: (&str, &str),
    ) -> Result<String, StorageError> {
        let (database_name, _) = layout;
        if database_name.is_empty()
            || database_name == "."
            || database_name == ".."
            || database_name.contains(['/', '\\', ':'])
        {
            return Err(StorageError::NativeHistory("历史数据库文件名无效"));
        }
        validate_home_name(new_home_name)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Read both associations under the same write reservation, so a
        // concurrent native-thread binding cannot race a "new task" choice.
        let binding: Option<String> = transaction
            .query_row(
                "SELECT codex_thread_id FROM codex_thread_bindings WHERE task_id = ?1",
                [task_id],
                |row| row.get(0),
            )
            .optional()?;
        let stored: Option<String> = transaction
            .query_row(
                "SELECT home_name FROM codex_history_locations WHERE task_id = ?1",
                [task_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(home) = stored {
            validate_home_name(&home)?;
            // Newly started native threads can still be unmaterialized. Our
            // prior home binding is authoritative; cold resume validates the
            // native ID through the native API, not a premature SQL-row check.
            if binding.is_some()
                && checked_database_path(managed_root, &home, database_name)?.is_none()
            {
                return Err(StorageError::NativeHistory(
                    "已绑定的历史目录或数据库缺失；未新建替代会话",
                ));
            }
            transaction.commit()?;
            return Ok(home);
        }
        let selected = if let Some(thread_id) = binding {
            find_thread_home(managed_root, &thread_id, layout)?.ok_or(
                StorageError::NativeHistory("未找到原任务的历史数据库；请恢复原数据目录"),
            )?
        } else {
            new_home_name.to_owned()
        };
        transaction.execute(
            "INSERT INTO codex_history_locations(task_id, home_name) VALUES (?1, ?2)",
            (task_id, &selected),
        )?;
        transaction.commit()?;
        Ok(selected)
    }
}

fn validate_home_name(home: &str) -> Result<(), StorageError> {
    if home.len() != 64
        || !home
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StorageError::NativeHistory("历史目录标识无效"));
    }
    Ok(())
}

fn find_thread_home(
    root: &Path,
    thread_id: &str,
    layout: (&str, &str),
) -> Result<Option<String>, StorageError> {
    if !root.try_exists()? {
        return Ok(None);
    }
    let mut found = None;
    let mut count = 0;
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let Some(home) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if validate_home_name(&home).is_err() {
            continue;
        }
        count += 1;
        if count > 512 {
            return Err(StorageError::NativeHistory(
                "历史目录过多，需整理后再定位；未选择任意副本",
            ));
        }
        if contains_thread(root, &home, thread_id, layout)? {
            if found.is_some() {
                return Err(StorageError::NativeHistory(
                    "多个历史目录包含同一任务；请确认使用哪份原始数据",
                ));
            }
            found = Some(home);
        }
    }
    Ok(found)
}

fn contains_thread(
    root: &Path,
    home: &str,
    thread_id: &str,
    layout: (&str, &str),
) -> Result<bool, StorageError> {
    let Some(database) = checked_database_path(root, home, layout.0)? else {
        return Ok(false);
    };
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(2))?;
    connection
        .query_row(layout.1, [thread_id], |row| row.get(0))
        .map_err(Into::into)
}

fn checked_database_path(
    root: &Path,
    home: &str,
    database_name: &str,
) -> Result<Option<PathBuf>, StorageError> {
    validate_home_name(home)?;
    let path = root.join(home);
    if !path.try_exists()? {
        return Ok(None);
    }
    reject_link(&path)?;
    let canonical_root = std::fs::canonicalize(root)?;
    let canonical_home = std::fs::canonicalize(&path)?;
    if canonical_home.parent() != Some(canonical_root.as_path()) || !canonical_home.is_dir() {
        return Err(StorageError::NativeHistory("历史目录越界或不是文件夹"));
    }
    let database = canonical_home.join(database_name);
    if !database.try_exists()? {
        return Ok(None);
    }
    reject_link(&database)?;
    Ok(Some(database))
}

fn reject_link(path: &Path) -> Result<(), StorageError> {
    let metadata = std::fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(StorageError::NativeHistory(
                "历史目录或数据库不能是重解析链接",
            ));
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(StorageError::NativeHistory(
            "历史目录或数据库不能是符号链接",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_history_tests.rs"]
mod tests;
