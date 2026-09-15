use super::*;

impl DesktopRuntime {
    fn ensure_lifecycle_idle(&self, id: TaskId) -> Result<(), DesktopError> {
        if self.storage.get_task(&id.to_string())?.is_none() {
            return Err(local_agent_core::CoreError::TaskNotFound(id).into());
        }
        // Pending submissions/completions and descendant work own a project lease.
        if !self.project_leases.is_empty() || !self.preparing_codex_turns.is_empty()
            || self.core.snapshot().turns.iter().any(|turn| turn.status == TurnStatus::Running)
            || !self.lifecycle_pending.is_empty() {
            return Err(DesktopError::UnsupportedCodexOperation("归档或删除：请先等待所有任务停止并确认状态"));
        }
        Ok(())
    }

    fn forget_deleted_task(&mut self, id: TaskId) {
        let turns: std::collections::HashSet<String> = self.core.snapshot().turns.iter()
            .filter(|turn| turn.task_id == id).map(|turn| turn.id.to_string()).collect();
        self.core.forget_deleted_task(id);
        self.messages.remove(&id);
        self.tool_exchanges.remove(&id);
        self.task_goals.remove(&id);
        self.task_permissions.remove(&id);
        self.task_backends.remove(&id);
        self.task_memory_enabled.remove(&id);
        self.actions.retain(|_, action| action.task_id != id.to_string());
        self.codex_turn_links.retain(|key, _| !turns.contains(key));
        self.codex_turn_projectors.retain(|key, _| !turns.contains(key));
        self.codex_turn_owners.retain(|key, _| !turns.contains(key));
        self.turn_profiles.retain(|key, _| !turns.contains(key));
    }
}

#[tauri::command]
pub fn set_task_archived(task_id: String, archived: bool, state: State<'_, DesktopState>) -> Result<BackendSnapshot, String> {
    let mut runtime = state.lock().map_err(command_error)?;
    let id = parse_task_id(&task_id).map_err(command_error)?;
    runtime.ensure_lifecycle_idle(id).map_err(command_error)?;
    runtime.storage.set_task_archived(&task_id, archived, Uuid::new_v4().to_string()).map_err(command_error)?;
    runtime.snapshot().map_err(command_error)
}

#[tauri::command]
pub async fn delete_task(task_id: String, app: AppHandle, state: State<'_, DesktopState>) -> Result<BackendSnapshot, String> {
    let started = Instant::now();
    let id = parse_task_id(&task_id).map_err(command_error)?;
    let control = {
        let mut runtime = state.lock().map_err(command_error)?;
        runtime.ensure_lifecycle_idle(id).map_err(command_error)?;
        let control = if runtime.storage.codex_thread_binding_for_task(&task_id).map_err(command_error)?.is_some() {
            Some(runtime.prepare_codex_control(&task_id).map_err(command_error)?)
        } else { None };
        runtime.lifecycle_pending.insert(id);
        control
    };
    let result: Result<(), DesktopError> = async {
        if let Some(control) = control {
            let prepare_started = Instant::now();
            let key = codex_kernel_key(&control.profile);
            let (client, _) = ensure_codex_kernel(app, Arc::clone(&state.runtime),
                Arc::clone(&state.codex_kernels), Arc::clone(&state.codex_loaded_threads),
                Arc::clone(&state.codex_pending_approvals), state.selected_kernel()?, state.codex_home.clone(),
                &key, &control.profile.profile_id, control.gateway, id).await?;
            crate::logging::info("delete_kernel_ready", json!({"duration_ms":prepare_started.elapsed().as_millis()}));
            let native_started = Instant::now();
            CodexSessionBridge::new(client).delete_thread(&control.codex_thread_id).await?;
            crate::logging::info("delete_native_completed", json!({"duration_ms":native_started.elapsed().as_millis()}));
        }
        let mut runtime = state.lock()?;
        runtime.storage.delete_task_records(&task_id)?;
        runtime.forget_deleted_task(id);
        Ok(())
    }.await;
    let mut runtime = state.lock().map_err(command_error)?;
    runtime.lifecycle_pending.remove(&id);
    result.map_err(command_error)?;
    crate::logging::info("delete_completed", json!({"duration_ms":started.elapsed().as_millis()}));
    runtime.snapshot().map_err(command_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_and_deletion_project_correctly_after_restart_and_active_work_is_rejected() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let project = directory.path().join("project");
        fs::create_dir(&project).expect("fixture project");
        let path = directory.path().join("test.db");
        let open = || DesktopRuntime::open(&path, Arc::new(crate::secrets::MemorySecretStore::new())).expect("runtime");
        let mut runtime = open();
        let project_id = runtime.open_project(project.clone()).expect("open project").projects[0].id.clone();
        let snapshot = runtime.create_task(CreateTaskInput { project_id, title: "fixture".into(), goal: "test".into(), permission_level: PermissionLevel::Approval }).expect("create task");
        let id = parse_task_id(&snapshot.tasks[0].id).expect("id");
        runtime.ensure_lifecycle_idle(id).expect("idle");
        runtime.storage.set_task_archived(&id.to_string(), true, Uuid::new_v4().to_string()).expect("archive");
        drop(runtime);
        let mut runtime = open();
        assert!(runtime.snapshot().expect("snapshot").tasks[0].archived);
        runtime.lifecycle_pending.insert(id);
        assert!(runtime.ensure_lifecycle_idle(id).is_err());
        runtime.lifecycle_pending.clear();
        let started = runtime.core.decide(AppCommand::StartTurn { task_id: id }).expect("start");
        runtime.core.apply(&started).expect("project start");
        assert!(runtime.ensure_lifecycle_idle(id).is_err());
        // Reopen the fixture's idle durable state; no real task was dispatched.
        drop(runtime);
        let mut runtime = open();
        runtime.storage.delete_task_records(&id.to_string()).expect("delete records");
        runtime.forget_deleted_task(id);
        let snapshot = runtime.snapshot().expect("snapshot");
        assert!(snapshot.tasks.is_empty());
        assert!(snapshot.messages.is_empty());
        drop(runtime);
        assert!(open().snapshot().expect("reopened snapshot").tasks.is_empty());
        assert!(project.is_dir());
    }
}
