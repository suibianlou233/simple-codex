use super::*;
use crate::secrets::MemorySecretStore;

#[path = "project_execution_native_tests.rs"]
mod native;

#[path = "codex_handoff_tests.rs"]
mod handoff;
#[path = "codex_submission_tests.rs"]
mod submission;

#[path = "legacy_undo_guard_tests.rs"]
mod legacy_undo;

#[path = "memory_notes_tests.rs"]
mod memory_notes;

#[path = "subagent_assignment_tests.rs"]
mod assignments;

fn fixture() -> (tempfile::TempDir, DesktopRuntime, PreparedCodexTurn) {
    let temp = tempfile::tempdir().expect("fixture");
    let (runtime, prepared) = fixture_at(temp.path());
    (temp, runtime, prepared)
}

fn fixture_at(directory: &Path) -> (DesktopRuntime, PreparedCodexTurn) {
    let root = directory.join("project");
    fs::create_dir(&root).expect("project");
    fs::write(root.join("file.txt"), "before\n").expect("file");
    fs::write(root.join("delete.txt"), "user original\n").expect("delete fixture");
    let (mut runtime, prepared) = prepare_at(directory, &root);
    runtime
        .acquire_project_execution(
            &prepared.task_id.to_string(),
            &prepared.turn_id.to_string(),
            &prepared.project_root,
        )
        .expect("project lease");
    (runtime, prepared)
}

fn prepare_at(directory: &Path, root: &Path) -> (DesktopRuntime, PreparedCodexTurn) {
    let mut runtime = DesktopRuntime::open(
        &directory.join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("runtime");
    let project_id = runtime
        .open_project(root.to_path_buf())
        .expect("open project")
        .projects[0]
        .id
        .clone();
    runtime
        .save_model_profile(SaveModelProfileInput {
            profile_id: None,
            name: "fixture".into(),
            base_url: "http://127.0.0.1:8000/v1".into(),
            model: "fixture".into(),
            dialect: "standard".into(),
            api_key: None,
            max_output_tokens: Some(1024),
            context_window_tokens: Some(32768),
            timeout_ms: 30000,
            is_default: true,
        })
        .expect("profile");
    let prepared = runtime
        .prepare_codex_new_chat(&StartChatInput {
            project_id,
            profile_id: None,
            content: "edit fixture".into(),
            permission_level: PermissionLevel::Approval,
        })
        .expect("prepare");
    (runtime, prepared)
}

fn finish(runtime: &mut DesktopRuntime, prepared: &PreparedCodexTurn, status: ProjectedTurnStatus) {
    runtime
        .finish_codex_projected_turn(&ProjectedTurnTerminal {
            task_id: prepared.task_id.to_string(),
            turn_id: prepared.turn_id.to_string(),
            status,
            error_message: None,
        })
        .expect("finish");
}


#[test]
fn native_turn_dispatch_and_completion_never_create_workspace_snapshots() {
    let (temp, mut runtime, prepared) = fixture();
    let task = prepared.task_id.to_string();
    let turn = prepared.turn_id.to_string();
    runtime.codex_turn_owners.insert(turn.clone(), CodexTurnOwner {
        kernel_key: "fixture".into(), instance_id: "fixture-instance".into()
    });
    runtime.begin_codex_submission(&prepared, "fixture-thread", "fixture-instance").expect("dispatch without baseline");
    fs::write(prepared.project_root.join("file.txt"), "native result").expect("simulated native write");
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt")).expect("file"), "native result");
    assert!(!temp.path().join("review-blobs").exists());
    assert!(!runtime.storage.load_events(&task).expect("events").iter().any(|e| e.event_type.starts_with("review_")));
    assert!(!runtime.project_leases.contains_key(&turn));
}

#[test]
fn lease_and_stop_still_guard_dispatch_without_snapshots() {
    let (_temp, mut runtime, prepared) = fixture();
    let turn = prepared.turn_id.to_string();
    runtime.codex_turn_owners.insert(turn.clone(), CodexTurnOwner {
        kernel_key: "fixture".into(), instance_id: "fixture-instance".into()
    });
    let token = CancellationToken::new();
    token.cancel();
    runtime.preparing_codex_turns.insert(turn.clone(), token);
    assert!(runtime.begin_codex_submission(&prepared, "thread", "fixture-instance").is_err());
    runtime.preparing_codex_turns.clear();
    runtime.project_leases.remove(&turn);
    assert!(runtime.begin_codex_submission(&prepared, "thread", "fixture-instance").is_err());
}

#[test]
fn old_snapshot_events_and_blobs_are_preserved_without_reactivation() {
    let (temp, mut runtime, prepared) = fixture();
    let task = prepared.task_id.to_string();
    let turn = prepared.turn_id.to_string();
    let blobs = temp.path().join("review-blobs");
    fs::create_dir(&blobs).expect("historical backup directory");
    fs::write(blobs.join("fixture"), "retained backup").expect("historical backup");
    runtime.storage.append_event(NewEvent {
        event_id: Uuid::new_v4().to_string(), task_id: task.clone(), turn_id: Some(turn),
        event_type: "review_baseline".into(), payload: json!({"historical_fixture":true}), created_at_ms: unix_time_ms().expect("clock")
    }).expect("historical record");
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    drop(runtime);
    let reopened = DesktopRuntime::open(&temp.path().join("data.db"), Arc::new(MemorySecretStore::new())).expect("reopen");
    assert_eq!(fs::read_to_string(blobs.join("fixture")).expect("preserved blob"), "retained backup");
    let events = reopened.storage.load_events(&task).expect("events");
    assert_eq!(events.iter().filter(|e| e.event_type.starts_with("review_")).count(), 1);
    assert!(events.iter().any(|e| e.event_type == "review_baseline"));
}
