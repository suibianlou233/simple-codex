use std::fs;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use rusqlite::TransactionBehavior;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::codex_binding::{NewCodexThreadBinding, insert_codex_thread_binding, validate_binding};
use crate::migrations;

#[path = "project_notes.rs"]
mod project_notes;
pub use project_notes::ProjectMemoryNotes;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("项目记忆已被其他窗口修改，请刷新后重新编辑")]
    MemoryNotesConflict,
    #[error("项目记忆最多允许 8192 字节，且版本必须有效")]
    InvalidMemoryNotes,
    #[error("SQLite 操作失败：{0}")]
    Database(#[from] rusqlite::Error),
    #[error("本地存储路径操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("未找到任务 `{0}`")]
    TaskNotFound(String),
    #[error("未找到模型配置 `{0}`")]
    ModelProfileNotFound(String),
    #[error("未找到项目 `{0}`")]
    ProjectNotFound(String),
    #[error("项目 `{0}` 已存在")]
    DuplicateProject(String),
    #[error("项目根目录 `{0}` 已经登记")]
    DuplicateProjectRoot(String),
    #[error("任务 `{0}` 已存在")]
    DuplicateTask(String),
    #[error("任务 `{task_id}` 已绑定到另一个 Codex Thread 或模型配置")]
    CodexThreadBindingConflict { task_id: String },
    #[error("无法安全定位任务历史：{0}")]
    NativeHistory(&'static str),
    #[error("Codex Thread `{codex_thread_id}` 已绑定到任务 `{task_id}`")]
    CodexThreadAlreadyBound {
        codex_thread_id: String,
        task_id: String,
    },
    #[error("事件 `{0}` 已存在")]
    DuplicateEvent(String),
    #[error("Agent Journal 事件 `{0}` 已存在")]
    DuplicateJournalEvent(String),
    #[error("事件 `{event_id}` 属于任务 `{found}`，预期属于任务 `{expected}`")]
    EventTaskMismatch {
        event_id: String,
        expected: String,
        found: String,
    },
    #[error("Journal 事件 `{event_id}` 属于 Thread `{found}`，预期属于 `{expected}`")]
    JournalThreadMismatch {
        event_id: String,
        expected: String,
        found: String,
    },
    #[error(
        "Thread `{thread_id}` 的快照序号 {through_sequence} 超过当前 Journal 序号 {last_sequence}"
    )]
    SnapshotAheadOfJournal {
        thread_id: String,
        through_sequence: i64,
        last_sequence: i64,
    },
    #[error("Thread `{thread_id}` 的快照不能从序号 {current_sequence} 回退到 {attempted_sequence}")]
    StaleThreadSnapshot {
        thread_id: String,
        current_sequence: i64,
        attempted_sequence: i64,
    },
    #[error("Journal 事件 `{event_id}` 无法用于恢复：{message}")]
    InvalidRecoveryEvent {
        event_id: String,
        message: &'static str,
    },
    #[error("未找到幂等操作占用 `{0}`")]
    ActionClaimNotFound(String),
    #[error("幂等键 `{key}` 已属于操作 `{existing_action_id}`，不能改用于 `{attempted_action_id}`")]
    IdempotencyConflict {
        key: String,
        existing_action_id: String,
        attempted_action_id: String,
    },
    #[error("幂等键 `{key}` 对操作 `{action_id}` 的定义与已持久化 intent 不一致：{message}")]
    IdempotencyDefinitionConflict {
        key: String,
        action_id: String,
        message: &'static str,
    },
    #[error("幂等操作 `{key}` 状态为 `{found}`，要求 `{expected}`")]
    InvalidActionTransition {
        key: String,
        found: &'static str,
        expected: &'static str,
    },
    #[error("幂等操作索引损坏：{message}")]
    CorruptActionClaim { message: String },
    #[error("{field} 不能为空")]
    EmptyField { field: &'static str },
    #[error("{field} 无效：{message}")]
    InvalidField {
        field: &'static str,
        message: &'static str,
    },
    #[error("时间戳不能早于 Unix 纪元")]
    ClockBeforeUnixEpoch,
    #[error("数据库结构版本 {found} 高于当前支持的版本 {supported}")]
    UnsupportedSchema { found: i64, supported: i64 },
    #[error("事件内容不是有效的 JSON：{0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewProject {
    pub project_id: String,
    pub root: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub project_id: String,
    pub root: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTask {
    pub task_id: String,
    pub project_id: String,
    pub title: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub task_id: String,
    pub project_id: String,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_sequence: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewEvent {
    pub event_id: String,
    pub task_id: String,
    pub turn_id: Option<String>,
    pub event_type: String,
    pub payload: Value,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub event_id: String,
    pub task_id: String,
    pub turn_id: Option<String>,
    pub sequence: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskSnapshot {
    pub task: TaskRecord,
    pub events: Vec<EventRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewModelProfile {
    pub profile_id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub dialect: String,
    pub credential_ref: String,
    pub max_output_tokens: Option<i64>,
    pub context_window_tokens: Option<i64>,
    pub timeout_ms: i64,
    pub is_default: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfileRecord {
    pub profile_id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub dialect: String,
    pub credential_ref: String,
    pub max_output_tokens: Option<i64>,
    pub context_window_tokens: Option<i64>,
    pub timeout_ms: i64,
    pub is_default: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// A single SQLite connection used for short local persistence operations.
///
/// Open another `Storage` for another thread. SQLite WAL and immediate write
/// transactions serialize event sequence allocation across those connections.
pub struct Storage {
    pub(crate) connection: Connection,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        Self::configure(connection, true)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let connection = Connection::open_in_memory()?;
        Self::configure(connection, false)
    }

    fn configure(mut connection: Connection, use_wal: bool) -> Result<Self, StorageError> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        if use_wal {
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        migrations::migrate(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn upsert_model_profile(
        &mut self,
        profile: NewModelProfile,
    ) -> Result<ModelProfileRecord, StorageError> {
        validate_model_profile(&profile)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if profile.is_default {
            transaction.execute(
                "UPDATE model_profiles SET is_default = 0 WHERE is_default = 1",
                [],
            )?;
        }
        transaction.execute(
            "INSERT INTO model_profiles(
                profile_id, name, base_url, model, dialect, credential_ref,
                max_output_tokens, context_window_tokens, timeout_ms, is_default,
                created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(profile_id) DO UPDATE SET
                name = excluded.name,
                base_url = excluded.base_url,
                model = excluded.model,
                dialect = excluded.dialect,
                credential_ref = excluded.credential_ref,
                max_output_tokens = excluded.max_output_tokens,
                context_window_tokens = excluded.context_window_tokens,
                timeout_ms = excluded.timeout_ms,
                is_default = excluded.is_default,
                updated_at_ms = excluded.updated_at_ms",
            (
                &profile.profile_id,
                &profile.name,
                &profile.base_url,
                &profile.model,
                &profile.dialect,
                &profile.credential_ref,
                profile.max_output_tokens,
                profile.context_window_tokens,
                profile.timeout_ms,
                i64::from(profile.is_default),
                profile.created_at_ms,
                profile.updated_at_ms,
            ),
        )?;
        let record = model_profile_by_id(&transaction, &profile.profile_id)?
            .ok_or_else(|| StorageError::Database(rusqlite::Error::QueryReturnedNoRows))?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn list_model_profiles(&self) -> Result<Vec<ModelProfileRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT profile_id, name, base_url, model, dialect, credential_ref,
                    max_output_tokens, context_window_tokens, timeout_ms, is_default,
                    created_at_ms, updated_at_ms
             FROM model_profiles
             ORDER BY is_default DESC, updated_at_ms DESC, name ASC",
        )?;
        let rows = statement.query_map([], model_profile_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Read settings for a separately owned desktop slot without migrating or
    /// opening the source database for writes. Credentials are only references.
    pub fn read_model_profiles_read_only(
        path: &Path,
    ) -> Result<Vec<ModelProfileRecord>, StorageError> {
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Self { connection }.list_model_profiles()
    }

    pub fn get_model_profile(
        &self,
        profile_id: &str,
    ) -> Result<Option<ModelProfileRecord>, StorageError> {
        model_profile_by_id(&self.connection, profile_id)
    }

    pub fn set_default_model_profile(
        &mut self,
        profile_id: &str,
        updated_at_ms: i64,
    ) -> Result<ModelProfileRecord, StorageError> {
        validate_non_empty("profile_id", profile_id)?;
        validate_timestamp(updated_at_ms)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if model_profile_by_id(&transaction, profile_id)?.is_none() {
            return Err(StorageError::ModelProfileNotFound(profile_id.to_owned()));
        }
        transaction.execute(
            "UPDATE model_profiles SET is_default = 0 WHERE is_default = 1",
            [],
        )?;
        transaction.execute(
            "UPDATE model_profiles SET is_default = 1, updated_at_ms = ?2 WHERE profile_id = ?1",
            (profile_id, updated_at_ms),
        )?;
        let record = model_profile_by_id(&transaction, profile_id)?
            .ok_or_else(|| StorageError::ModelProfileNotFound(profile_id.to_owned()))?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn create_project(&mut self, project: NewProject) -> Result<ProjectRecord, StorageError> {
        validate_new_project(&project)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if project_exists(&transaction, &project.project_id)? {
            return Err(StorageError::DuplicateProject(project.project_id.clone()));
        }
        if project_by_root(&transaction, &project.root)?.is_some() {
            return Err(StorageError::DuplicateProjectRoot(project.root.clone()));
        }
        insert_project(&transaction, &project)?;
        transaction.commit()?;

        Ok(new_project_record(project))
    }

    /// Returns the existing project for `root`, or atomically registers it.
    ///
    /// The caller must provide a canonical, reversible root identity. Concurrent
    /// calls for that root are serialized by an immediate transaction, so only
    /// one project row is ever created.
    pub fn ensure_project(&mut self, project: NewProject) -> Result<ProjectRecord, StorageError> {
        validate_new_project(&project)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = project_by_root(&transaction, &project.root)? {
            transaction.commit()?;
            return Ok(existing);
        }
        if project_exists(&transaction, &project.project_id)? {
            return Err(StorageError::DuplicateProject(project.project_id));
        }
        insert_project(&transaction, &project)?;
        transaction.commit()?;
        Ok(new_project_record(project))
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectRecord>, StorageError> {
        let mut statement = self
            .connection
            .prepare("SELECT project_id, root, name FROM projects ORDER BY name, project_id")?;
        let rows = statement.query_map([], project_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_project(&self, project_id: &str) -> Result<Option<ProjectRecord>, StorageError> {
        let project = self
            .connection
            .query_row(
                "SELECT project_id, root, name FROM projects WHERE project_id = ?1",
                [project_id],
                project_from_row,
            )
            .optional()?;
        Ok(project)
    }

    pub fn get_project_by_root(&self, root: &str) -> Result<Option<ProjectRecord>, StorageError> {
        let project = self
            .connection
            .query_row(
                "SELECT project_id, root, name FROM projects WHERE root = ?1",
                [root],
                project_from_row,
            )
            .optional()?;
        Ok(project)
    }

    pub fn create_task(&mut self, task: NewTask) -> Result<TaskRecord, StorageError> {
        validate_new_task(&task)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_task(&transaction, &task)?;
        transaction.commit()?;

        Ok(new_task_record(task))
    }

    /// Atomically creates a task and its initial durable events.
    ///
    /// All rows are written in one immediate transaction. Events receive
    /// sequences `1..=n` in input order. Any failure leaves no task or events.
    pub fn create_task_with_events(
        &mut self,
        task: NewTask,
        events: Vec<NewEvent>,
    ) -> Result<TaskSnapshot, StorageError> {
        validate_new_task(&task)?;
        let expected_task_id = task.task_id.clone();
        let prepared_events = events
            .into_iter()
            .map(|event| {
                if event.task_id != expected_task_id {
                    return Err(StorageError::EventTaskMismatch {
                        event_id: event.event_id,
                        expected: expected_task_id.clone(),
                        found: event.task_id,
                    });
                }
                prepare_event(event)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_task(&transaction, &task)?;

        let mut records = Vec::with_capacity(prepared_events.len());
        for event in prepared_events {
            records.push(append_prepared_event(&transaction, event)?);
        }
        let task = transaction.query_row(
            "SELECT task_id, project_id, title, created_at_ms, updated_at_ms, last_sequence \
             FROM tasks WHERE task_id = ?1",
            [&expected_task_id],
            task_from_row,
        )?;
        transaction.commit()?;

        Ok(TaskSnapshot {
            task,
            events: records,
        })
    }

    /// Atomically creates a Codex-backed task, its initial events, and its
    /// immutable local-to-Codex thread binding.
    pub fn create_task_with_events_and_codex_binding(
        &mut self,
        task: NewTask,
        events: Vec<NewEvent>,
        binding: NewCodexThreadBinding,
    ) -> Result<TaskSnapshot, StorageError> {
        validate_new_task(&task)?;
        validate_binding(&binding)?;
        let expected_task_id = task.task_id.clone();
        if binding.task_id != expected_task_id {
            return Err(StorageError::InvalidField {
                field: "codex_binding.task_id",
                message: "must match the new task",
            });
        }
        let prepared_events = events
            .into_iter()
            .map(|event| {
                if event.task_id != expected_task_id {
                    return Err(StorageError::EventTaskMismatch {
                        event_id: event.event_id,
                        expected: expected_task_id.clone(),
                        found: event.task_id,
                    });
                }
                prepare_event(event)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_task(&transaction, &task)?;
        let mut records = Vec::with_capacity(prepared_events.len());
        for event in prepared_events {
            records.push(append_prepared_event(&transaction, event)?);
        }
        insert_codex_thread_binding(&transaction, binding)?;
        let task = transaction.query_row(
            "SELECT task_id, project_id, title, created_at_ms, updated_at_ms, last_sequence \
             FROM tasks WHERE task_id = ?1",
            [&expected_task_id],
            task_from_row,
        )?;
        transaction.commit()?;
        Ok(TaskSnapshot {
            task,
            events: records,
        })
    }

    /// Lists locally persisted tasks in most-recently-updated order.
    pub fn list_tasks(&self) -> Result<Vec<TaskRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT task_id, project_id, title, created_at_ms, updated_at_ms, last_sequence \
             FROM tasks \
             ORDER BY updated_at_ms DESC, created_at_ms DESC, task_id ASC",
        )?;
        let rows = statement.query_map([], task_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<TaskRecord>, StorageError> {
        let task = self
            .connection
            .query_row(
                "SELECT task_id, project_id, title, created_at_ms, updated_at_ms, last_sequence \
                 FROM tasks WHERE task_id = ?1",
                [task_id],
                task_from_row,
            )
            .optional()?;
        Ok(task)
    }

    /// Appends one immutable event and atomically assigns its task-local sequence.
    pub fn append_event(&mut self, event: NewEvent) -> Result<EventRecord, StorageError> {
        let event = prepare_event(event)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = append_prepared_event(&transaction, event)?;
        transaction.commit()?;
        Ok(record)
    }

    /// Appends a group of events atomically in input order.
    pub fn append_events(
        &mut self,
        events: Vec<NewEvent>,
    ) -> Result<Vec<EventRecord>, StorageError> {
        let prepared = events
            .into_iter()
            .map(prepare_event)
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let records = prepared
            .into_iter()
            .map(|event| append_prepared_event(&transaction, event))
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;
        Ok(records)
    }

    pub fn load_events(&self, task_id: &str) -> Result<Vec<EventRecord>, StorageError> {
        if self.get_task(task_id)?.is_none() {
            return Err(StorageError::TaskNotFound(task_id.to_owned()));
        }

        let mut statement = self.connection.prepare(
            "SELECT event_id, task_id, turn_id, sequence, event_type, payload_json, created_at_ms \
             FROM events WHERE task_id = ?1 ORDER BY sequence ASC",
        )?;
        let rows = statement.query_map([task_id], |row| {
            let payload_json: String = row.get(5)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                payload_json,
                row.get::<_, i64>(6)?,
            ))
        })?;

        rows.map(|row| {
            let (event_id, task_id, turn_id, sequence, event_type, payload_json, created_at_ms) =
                row?;
            Ok(EventRecord {
                event_id,
                task_id,
                turn_id,
                sequence,
                event_type,
                payload: serde_json::from_str(&payload_json)?,
                created_at_ms,
            })
        })
        .collect()
    }

    /// Loads all durable information needed to restore one task after restart.
    pub fn load_task(&self, task_id: &str) -> Result<Option<TaskSnapshot>, StorageError> {
        let Some(task) = self.get_task(task_id)? else {
            return Ok(None);
        };
        let events = self.load_events(task_id)?;
        Ok(Some(TaskSnapshot { task, events }))
    }
}

struct PreparedEvent {
    event: NewEvent,
    payload_json: String,
}

fn validate_new_project(project: &NewProject) -> Result<(), StorageError> {
    validate_non_empty("project_id", &project.project_id)?;
    validate_non_empty("root", &project.root)?;
    validate_non_empty("name", &project.name)
}

fn insert_project(transaction: &Transaction<'_>, project: &NewProject) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO projects(project_id, root, name) VALUES (?1, ?2, ?3)",
        (&project.project_id, &project.root, &project.name),
    )?;
    Ok(())
}

fn new_project_record(project: NewProject) -> ProjectRecord {
    ProjectRecord {
        project_id: project.project_id,
        root: project.root,
        name: project.name,
    }
}

fn validate_new_task(task: &NewTask) -> Result<(), StorageError> {
    validate_non_empty("task_id", &task.task_id)?;
    validate_non_empty("project_id", &task.project_id)?;
    validate_non_empty("title", &task.title)?;
    validate_timestamp(task.created_at_ms)
}

fn prepare_event(event: NewEvent) -> Result<PreparedEvent, StorageError> {
    validate_non_empty("event_id", &event.event_id)?;
    validate_non_empty("task_id", &event.task_id)?;
    validate_non_empty("event_type", &event.event_type)?;
    if let Some(turn_id) = &event.turn_id {
        validate_non_empty("turn_id", turn_id)?;
    }
    validate_timestamp(event.created_at_ms)?;
    let payload_json = serde_json::to_string(&event.payload)?;
    Ok(PreparedEvent {
        event,
        payload_json,
    })
}

fn insert_task(transaction: &Transaction<'_>, task: &NewTask) -> Result<(), StorageError> {
    if task_exists(transaction, &task.task_id)? {
        return Err(StorageError::DuplicateTask(task.task_id.clone()));
    }
    if !project_exists(transaction, &task.project_id)? {
        return Err(StorageError::ProjectNotFound(task.project_id.clone()));
    }

    transaction.execute(
        "INSERT INTO tasks(\
            task_id, project_id, title, created_at_ms, updated_at_ms, last_sequence\
         ) VALUES (?1, ?2, ?3, ?4, ?4, 0)",
        (
            &task.task_id,
            &task.project_id,
            &task.title,
            task.created_at_ms,
        ),
    )?;
    Ok(())
}

fn append_prepared_event(
    transaction: &Transaction<'_>,
    prepared: PreparedEvent,
) -> Result<EventRecord, StorageError> {
    let event = prepared.event;
    if !task_exists(transaction, &event.task_id)? {
        return Err(StorageError::TaskNotFound(event.task_id));
    }
    if event_exists(transaction, &event.event_id)? {
        return Err(StorageError::DuplicateEvent(event.event_id));
    }

    let sequence = transaction.query_row(
        "UPDATE tasks \
         SET last_sequence = last_sequence + 1, \
             updated_at_ms = MAX(updated_at_ms, ?2) \
         WHERE task_id = ?1 \
         RETURNING last_sequence",
        (&event.task_id, event.created_at_ms),
        |row| row.get::<_, i64>(0),
    )?;

    transaction.execute(
        "INSERT INTO events(\
            event_id, task_id, turn_id, sequence, event_type, payload_json, created_at_ms\
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        (
            &event.event_id,
            &event.task_id,
            &event.turn_id,
            sequence,
            &event.event_type,
            &prepared.payload_json,
            event.created_at_ms,
        ),
    )?;

    Ok(EventRecord {
        event_id: event.event_id,
        task_id: event.task_id,
        turn_id: event.turn_id,
        sequence,
        event_type: event.event_type,
        payload: event.payload,
        created_at_ms: event.created_at_ms,
    })
}

fn new_task_record(task: NewTask) -> TaskRecord {
    TaskRecord {
        task_id: task.task_id,
        project_id: task.project_id,
        title: task.title,
        created_at_ms: task.created_at_ms,
        updated_at_ms: task.created_at_ms,
        last_sequence: 0,
    }
}

fn task_exists(transaction: &Transaction<'_>, task_id: &str) -> rusqlite::Result<bool> {
    transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE task_id = ?1)",
        [task_id],
        |row| row.get(0),
    )
}

fn project_exists(transaction: &Transaction<'_>, project_id: &str) -> rusqlite::Result<bool> {
    transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE project_id = ?1)",
        [project_id],
        |row| row.get(0),
    )
}

fn project_by_root(
    transaction: &Transaction<'_>,
    root: &str,
) -> rusqlite::Result<Option<ProjectRecord>> {
    transaction
        .query_row(
            "SELECT project_id, root, name FROM projects WHERE root = ?1",
            [root],
            project_from_row,
        )
        .optional()
}

fn event_exists(transaction: &Transaction<'_>, event_id: &str) -> rusqlite::Result<bool> {
    transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM events WHERE event_id = ?1)",
        [event_id],
        |row| row.get(0),
    )
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
    Ok(TaskRecord {
        task_id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        created_at_ms: row.get(3)?,
        updated_at_ms: row.get(4)?,
        last_sequence: row.get(5)?,
    })
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRecord> {
    Ok(ProjectRecord {
        project_id: row.get(0)?,
        root: row.get(1)?,
        name: row.get(2)?,
    })
}

fn model_profile_by_id(
    connection: &Connection,
    profile_id: &str,
) -> Result<Option<ModelProfileRecord>, StorageError> {
    connection
        .query_row(
            "SELECT profile_id, name, base_url, model, dialect, credential_ref,
                    max_output_tokens, context_window_tokens, timeout_ms, is_default,
                    created_at_ms, updated_at_ms
             FROM model_profiles WHERE profile_id = ?1",
            [profile_id],
            model_profile_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn model_profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelProfileRecord> {
    Ok(ModelProfileRecord {
        profile_id: row.get(0)?,
        name: row.get(1)?,
        base_url: row.get(2)?,
        model: row.get(3)?,
        dialect: row.get(4)?,
        credential_ref: row.get(5)?,
        max_output_tokens: row.get(6)?,
        context_window_tokens: row.get(7)?,
        timeout_ms: row.get(8)?,
        is_default: row.get::<_, i64>(9)? == 1,
        created_at_ms: row.get(10)?,
        updated_at_ms: row.get(11)?,
    })
}

fn validate_model_profile(profile: &NewModelProfile) -> Result<(), StorageError> {
    validate_non_empty("profile_id", &profile.profile_id)?;
    validate_non_empty("name", &profile.name)?;
    validate_non_empty("base_url", &profile.base_url)?;
    validate_non_empty("model", &profile.model)?;
    validate_non_empty("credential_ref", &profile.credential_ref)?;
    if !matches!(profile.dialect.as_str(), "standard" | "deep_seek" | "qwen") {
        return Err(StorageError::InvalidField {
            field: "dialect",
            message: "must be standard, deep_seek, or qwen",
        });
    }
    if profile.name.chars().count() > 80 {
        return Err(StorageError::InvalidField {
            field: "name",
            message: "must not exceed 80 characters",
        });
    }
    if profile.base_url.len() > 2_048 {
        return Err(StorageError::InvalidField {
            field: "base_url",
            message: "must not exceed 2048 bytes",
        });
    }
    if profile.model.chars().count() > 256 {
        return Err(StorageError::InvalidField {
            field: "model",
            message: "must not exceed 256 characters",
        });
    }
    if profile.max_output_tokens.is_some_and(|value| value < 1_024) {
        return Err(StorageError::InvalidField {
            field: "max_output_tokens",
            message: "must be at least 1024",
        });
    }
    if profile
        .context_window_tokens
        .is_some_and(|value| !(16_384..=1_048_576).contains(&value))
    {
        return Err(StorageError::InvalidField {
            field: "context_window_tokens",
            message: "must be between 16384 and 1048576",
        });
    }
    if let (Some(output), Some(context)) =
        (profile.max_output_tokens, profile.context_window_tokens)
        && output.saturating_add(8_192) > context
    {
        return Err(StorageError::InvalidField {
            field: "max_output_tokens",
            message: "must leave at least 8192 tokens for input",
        });
    }
    if !(1_000..=600_000).contains(&profile.timeout_ms) {
        return Err(StorageError::InvalidField {
            field: "timeout_ms",
            message: "must be between 1000 and 600000",
        });
    }
    validate_timestamp(profile.created_at_ms)?;
    validate_timestamp(profile.updated_at_ms)
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        Err(StorageError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn validate_timestamp(timestamp_ms: i64) -> Result<(), StorageError> {
    if timestamp_ms < 0 {
        Err(StorageError::ClockBeforeUnixEpoch)
    } else {
        Ok(())
    }
}

pub(crate) fn unix_time_ms() -> Result<i64, StorageError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::ClockBeforeUnixEpoch)?;
    i64::try_from(elapsed.as_millis()).map_err(|_| StorageError::ClockBeforeUnixEpoch)
}
