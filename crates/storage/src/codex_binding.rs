use rusqlite::OptionalExtension;
use rusqlite::TransactionBehavior;
use serde::Deserialize;
use serde::Serialize;

use crate::Storage;
use crate::StorageError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewCodexThreadBinding {
    pub task_id: String,
    pub codex_thread_id: String,
    pub model_profile_id: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexThreadBinding {
    pub task_id: String,
    pub codex_thread_id: String,
    pub model_profile_id: String,
    pub created_at_ms: i64,
}

impl Storage {
    /// Creates an immutable task-to-Codex association.
    ///
    /// Repeating the exact same write is idempotent. Any attempt to move an
    /// existing task to another thread/profile, or reuse a Codex thread for a
    /// different task, fails instead of silently changing model history.
    pub fn bind_codex_thread(
        &mut self,
        binding: NewCodexThreadBinding,
    ) -> Result<CodexThreadBinding, StorageError> {
        validate_binding(&binding)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = insert_codex_thread_binding(&transaction, binding)?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn codex_thread_binding_for_task(
        &self,
        task_id: &str,
    ) -> Result<Option<CodexThreadBinding>, StorageError> {
        validate_non_empty("task_id", task_id)?;
        binding_for_task(&self.connection, task_id)
    }

    pub fn list_codex_thread_bindings(&self) -> Result<Vec<CodexThreadBinding>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT task_id, codex_thread_id, model_profile_id, created_at_ms
             FROM codex_thread_bindings
             ORDER BY created_at_ms ASC, task_id ASC",
        )?;
        statement
            .query_map([], binding_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

fn binding_for_task(
    connection: &rusqlite::Connection,
    task_id: &str,
) -> Result<Option<CodexThreadBinding>, StorageError> {
    connection
        .query_row(
            "SELECT task_id, codex_thread_id, model_profile_id, created_at_ms
             FROM codex_thread_bindings WHERE task_id = ?1",
            [task_id],
            binding_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn binding_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodexThreadBinding> {
    Ok(CodexThreadBinding {
        task_id: row.get(0)?,
        codex_thread_id: row.get(1)?,
        model_profile_id: row.get(2)?,
        created_at_ms: row.get(3)?,
    })
}

pub(crate) fn validate_binding(binding: &NewCodexThreadBinding) -> Result<(), StorageError> {
    validate_non_empty("task_id", &binding.task_id)?;
    validate_non_empty("codex_thread_id", &binding.codex_thread_id)?;
    validate_non_empty("model_profile_id", &binding.model_profile_id)?;
    if binding.created_at_ms < 0 {
        return Err(StorageError::InvalidField {
            field: "created_at_ms",
            message: "must not be negative",
        });
    }
    Ok(())
}

pub(crate) fn insert_codex_thread_binding(
    transaction: &rusqlite::Transaction<'_>,
    binding: NewCodexThreadBinding,
) -> Result<CodexThreadBinding, StorageError> {
    validate_binding(&binding)?;
    if let Some(existing) = binding_for_task(transaction, &binding.task_id)? {
        if existing.codex_thread_id == binding.codex_thread_id
            && existing.model_profile_id == binding.model_profile_id
        {
            return Ok(existing);
        }
        return Err(StorageError::CodexThreadBindingConflict {
            task_id: binding.task_id,
        });
    }
    if let Some(task_id) = transaction
        .query_row(
            "SELECT task_id FROM codex_thread_bindings WHERE codex_thread_id = ?1",
            [&binding.codex_thread_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return Err(StorageError::CodexThreadAlreadyBound {
            codex_thread_id: binding.codex_thread_id,
            task_id,
        });
    }
    transaction.execute(
        "INSERT INTO codex_thread_bindings(
            task_id, codex_thread_id, model_profile_id, created_at_ms
         ) VALUES (?1, ?2, ?3, ?4)",
        (
            &binding.task_id,
            &binding.codex_thread_id,
            &binding.model_profile_id,
            binding.created_at_ms,
        ),
    )?;
    Ok(CodexThreadBinding {
        task_id: binding.task_id,
        codex_thread_id: binding.codex_thread_id,
        model_profile_id: binding.model_profile_id,
        created_at_ms: binding.created_at_ms,
    })
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        Err(StorageError::EmptyField { field })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewModelProfile;
    use crate::NewProject;
    use crate::NewTask;

    fn storage_with_task_and_profiles() -> Storage {
        let mut storage = Storage::open_in_memory().expect("open storage");
        storage
            .create_project(NewProject {
                project_id: "project-1".to_owned(),
                root: "C:\\workspace".to_owned(),
                name: "Workspace".to_owned(),
            })
            .expect("create project");
        storage
            .create_task(NewTask {
                task_id: "task-1".to_owned(),
                project_id: "project-1".to_owned(),
                title: "Task".to_owned(),
                created_at_ms: 1,
            })
            .expect("create task");
        for profile_id in ["profile-1", "profile-2"] {
            storage
                .upsert_model_profile(NewModelProfile {
                    profile_id: profile_id.to_owned(),
                    name: profile_id.to_owned(),
                    base_url: "http://127.0.0.1:8000/v1".to_owned(),
                    model: "local-model".to_owned(),
                    dialect: "standard".to_owned(),
                    credential_ref: format!("credential:{profile_id}"),
                    max_output_tokens: None,
                    context_window_tokens: None,
                    timeout_ms: 120_000,
                    is_default: profile_id == "profile-1",
                    created_at_ms: 1,
                    updated_at_ms: 1,
                })
                .expect("create profile");
        }
        storage
    }

    #[test]
    fn exact_binding_retry_is_idempotent() {
        let mut storage = storage_with_task_and_profiles();
        let input = NewCodexThreadBinding {
            task_id: "task-1".to_owned(),
            codex_thread_id: "codex-thread-1".to_owned(),
            model_profile_id: "profile-1".to_owned(),
            created_at_ms: 2,
        };

        let first = storage
            .bind_codex_thread(input.clone())
            .expect("bind thread");
        let second = storage.bind_codex_thread(input).expect("repeat binding");

        assert_eq!(first, second);
        let bindings = storage.list_codex_thread_bindings().expect("list bindings");
        assert_eq!(bindings.len(), 1);
    }

    #[test]
    fn task_cannot_silently_switch_model_profile() {
        let mut storage = storage_with_task_and_profiles();
        storage
            .bind_codex_thread(NewCodexThreadBinding {
                task_id: "task-1".to_owned(),
                codex_thread_id: "codex-thread-1".to_owned(),
                model_profile_id: "profile-1".to_owned(),
                created_at_ms: 2,
            })
            .expect("bind thread");

        let error = storage
            .bind_codex_thread(NewCodexThreadBinding {
                task_id: "task-1".to_owned(),
                codex_thread_id: "codex-thread-1".to_owned(),
                model_profile_id: "profile-2".to_owned(),
                created_at_ms: 3,
            })
            .expect_err("profile switch must fail");

        assert!(matches!(
            error,
            StorageError::CodexThreadBindingConflict { .. }
        ));
    }

    #[test]
    fn codex_task_creation_rolls_back_when_thread_is_already_bound() {
        let mut storage = storage_with_task_and_profiles();
        storage
            .bind_codex_thread(NewCodexThreadBinding {
                task_id: "task-1".to_owned(),
                codex_thread_id: "codex-thread-1".to_owned(),
                model_profile_id: "profile-1".to_owned(),
                created_at_ms: 2,
            })
            .expect("bind source thread");
        let error = storage
            .create_task_with_events_and_codex_binding(
                NewTask {
                    task_id: "task-2".to_owned(),
                    project_id: "project-1".to_owned(),
                    title: "Branch".to_owned(),
                    created_at_ms: 3,
                },
                Vec::new(),
                NewCodexThreadBinding {
                    task_id: "task-2".to_owned(),
                    codex_thread_id: "codex-thread-1".to_owned(),
                    model_profile_id: "profile-1".to_owned(),
                    created_at_ms: 3,
                },
            )
            .expect_err("reusing a thread must fail atomically");
        assert!(matches!(
            error,
            StorageError::CodexThreadAlreadyBound { .. }
        ));
        assert!(
            storage
                .get_task("task-2")
                .expect("query rolled-back task")
                .is_none()
        );
    }
}
