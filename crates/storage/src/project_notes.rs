//! User-owned project memory, independent of generated native artifacts.
use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMemoryNotes {
    pub content: String,
    pub revision: i64,
}

impl Storage {
    pub fn load_project_memory_notes(
        &self,
        task: &str,
    ) -> Result<ProjectMemoryNotes, StorageError> {
        let task = self
            .get_task(task)?
            .ok_or_else(|| StorageError::TaskNotFound(task.into()))?;
        read_notes(&self.connection, &task.project_id)
    }

    /// Compare-and-save and content-free audit commit together. Empty content is
    /// a deletion tombstone, so an old editor cannot resurrect a deleted note.
    pub fn save_project_memory_notes(
        &mut self,
        task: &str,
        expected_revision: i64,
        content: String,
        event_id: String,
        created_at_ms: i64,
    ) -> Result<ProjectMemoryNotes, StorageError> {
        if content.len() > 8192 || expected_revision < 0 || expected_revision == i64::MAX {
            return Err(StorageError::InvalidMemoryNotes);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let project: String = transaction
            .query_row(
                "SELECT project_id FROM tasks WHERE task_id = ?1",
                [task],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::TaskNotFound(task.into()))?;
        let prior = read_notes(&transaction, &project)?;
        if prior.revision != expected_revision {
            return Err(StorageError::MemoryNotesConflict);
        }
        let notes = ProjectMemoryNotes {
            content,
            revision: prior.revision + 1,
        };
        transaction.execute(
            "INSERT INTO project_memory_notes(project_id, content, revision) VALUES (?1, ?2, ?3)
             ON CONFLICT(project_id) DO UPDATE SET content = excluded.content, revision = excluded.revision",
            (&project, &notes.content, notes.revision),
        )?;
        append_prepared_event(
            &transaction,
            prepare_event(NewEvent {
                event_id,
                task_id: task.into(),
                turn_id: None,
                event_type: "project_memory_notes_changed".into(),
                payload: serde_json::json!({"revision":notes.revision,"bytes":notes.content.len(),"cleared":notes.content.is_empty()}),
                created_at_ms,
            })?,
        )?;
        transaction.commit()?;
        Ok(notes)
    }
}

fn read_notes(connection: &Connection, project: &str) -> Result<ProjectMemoryNotes, StorageError> {
    Ok(connection
        .query_row(
            "SELECT content, revision FROM project_memory_notes WHERE project_id = ?1",
            [project],
            |row| {
                Ok(ProjectMemoryNotes {
                    content: row.get(0)?,
                    revision: row.get(1)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}
