use crate::{Storage, StorageError, NewEvent};
use std::collections::HashSet;

impl Storage {
    pub fn archived_task_ids(&self) -> Result<HashSet<String>, StorageError> {
        let mut query = self.connection.prepare(
            "SELECT task_id FROM events e WHERE event_type = 'task_archive_changed'
             AND sequence = (SELECT MAX(sequence) FROM events WHERE task_id=e.task_id AND event_type='task_archive_changed')
             AND json_extract(payload_json, '$.archived') = 1")?;
        Ok(query.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<_, _>>()?)
    }

    pub fn set_task_archived(&mut self, task_id: &str, archived: bool, event_id: String) -> Result<(), StorageError> {
        self.append_event(NewEvent { event_id, task_id: task_id.into(), turn_id: None,
            event_type: "task_archive_changed".into(), payload: serde_json::json!({"archived": archived}),
            created_at_ms: crate::repository::unix_time_ms()? })?;
        Ok(())
    }

    /// Delete only the product records for this task, after native deletion succeeds.
    pub fn delete_task_records(&mut self, task_id: &str) -> Result<(), StorageError> {
        if self.get_task(task_id)?.is_none() { return Err(StorageError::TaskNotFound(task_id.into())); }
        let tx = self.connection.transaction()?;
        for sql in [
            "DELETE FROM action_claims WHERE thread_id=?1",
            "DELETE FROM thread_snapshots WHERE thread_id=?1",
            "DELETE FROM journal_events WHERE thread_id=?1",
            "DELETE FROM journal_streams WHERE thread_id=?1",
            "DELETE FROM codex_history_locations WHERE task_id=?1",
            "DELETE FROM codex_thread_bindings WHERE task_id=?1",
            "DELETE FROM events WHERE task_id=?1",
            "DELETE FROM tasks WHERE task_id=?1",
        ] { tx.execute(sql, [task_id])?; }
        tx.commit()?;
        Ok(())
    }
}
