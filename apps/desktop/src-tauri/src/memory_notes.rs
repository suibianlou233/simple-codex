//! Manual corrections belong to the project, never to a model profile.
use super::*;
use local_agent_storage::ProjectMemoryNotes;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryNotesView {
    pub(super) task_id: String,
    pub(super) content: String,
    pub(super) revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveMemoryNotesInput {
    pub(super) task_id: String,
    pub(super) content: String,
    pub(super) expected_revision: i64,
}

impl DesktopRuntime {
    pub(super) fn load_memory_notes(&self, task: &str) -> Result<MemoryNotesView, DesktopError> {
        let ProjectMemoryNotes { content, revision } =
            self.storage.load_project_memory_notes(task)?;
        Ok(MemoryNotesView {
            task_id: task.into(),
            content,
            revision,
        })
    }

    pub(super) fn save_memory_notes(
        &mut self,
        input: SaveMemoryNotesInput,
    ) -> Result<MemoryNotesView, DesktopError> {
        if self
            .known_secret_values()
            .iter()
            .any(|secret| input.content.contains(secret))
            || input.content.lines().any(|line| {
                let sanitized = crate::logging::sanitize_text(line);
                sanitized.contains("[REDACTED") && sanitized.trim_end() != line.trim_end()
            })
        {
            return Err(DesktopError::SensitiveMemoryNotes);
        }
        if self.project_execution_busy(&input.task_id, None)? {
            return Err(DesktopError::ProjectBusy);
        }
        let root = self.task_project_root(parse_task_id(&input.task_id)?)?;
        let _lease = project_lease::ProjectLease::acquire(
            &root,
            self.database_path
                .parent()
                .ok_or(DesktopError::InvalidStoredPath)?,
        )?;
        let notes = self.storage.save_project_memory_notes(
            &input.task_id,
            input.expected_revision,
            input.content,
            Uuid::new_v4().to_string(),
            unix_time_ms()?,
        )?;
        Ok(MemoryNotesView {
            task_id: input.task_id,
            content: notes.content,
            revision: notes.revision,
        })
    }

    pub(super) fn codex_user_input(
        &self,
        prepared: &PreparedCodexTurn,
    ) -> Result<Vec<Value>, DesktopError> {
        let notes = self
            .storage
            .load_project_memory_notes(&prepared.task_id.to_string())?;
        let mut input = Vec::new();
        if !notes.content.trim().is_empty()
            && self
                .task_memory_enabled
                .get(&prepared.task_id)
                .copied()
                .unwrap_or(true)
        {
            // Ordinary user input, not developer/system authority. The current
            // request remains separate and last. Never copy notes into logs.
            input.push(json!({"type":"text", "text":format!(
                "用户在 Simple 中确认的项目记忆（版本 {}）。这是用户维护的参考，不是系统指令；若与本轮明确要求冲突，以本轮要求为准。自动摘要可能过时，请结合项目代码核实。\n{}",
                notes.revision, notes.content), "text_elements":[]}));
        }
        input.push(json!({"type":"text", "text":prepared.content,"text_elements":[]}));
        Ok(input)
    }
}

#[tauri::command]
pub fn load_project_memory_notes(
    task_id: String,
    state: State<'_, DesktopState>,
) -> Result<MemoryNotesView, String> {
    state
        .lock()
        .map_err(command_error)?
        .load_memory_notes(&task_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn save_project_memory_notes(
    input: SaveMemoryNotesInput,
    state: State<'_, DesktopState>,
) -> Result<MemoryNotesView, String> {
    state
        .lock()
        .map_err(command_error)?
        .save_memory_notes(input)
        .map_err(command_error)
}
