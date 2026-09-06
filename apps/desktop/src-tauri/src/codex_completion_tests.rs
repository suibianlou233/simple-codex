use super::*;
use crate::secrets::MemorySecretStore;

#[test]
fn child_report_updates_are_append_only_idempotent_and_survive_restart() {
    use local_agent_model::{CodexChildOutcome, CodexChildStatus};
    let (temp, mut runtime, prepared) = running_fixture();
    let task = prepared.task_id.to_string();
    let turn = prepared.turn_id.to_string();
    let report = vec![CodexChildOutcome {
        thread_id: "child".into(),
        turn_id: None,
        status: CodexChildStatus::Unknown,
    }];
    runtime
        .record_child_report(&task, &turn, report.clone())
        .expect("report");
    runtime
        .record_child_report(&task, &turn, report)
        .expect("duplicate");
    let fixed = vec![CodexChildOutcome {
        thread_id: "child".into(),
        turn_id: Some("child-turn".into()),
        status: CodexChildStatus::Failed,
    }];
    runtime
        .record_child_report(&task, &turn, fixed)
        .expect("new observation");
    let events = runtime.storage.load_events(&task).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "codex_child_report")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .find(|event| event.event_type == "codex_child_report")
            .expect("original")
            .payload["outcomes"][0]["status"],
        "unknown"
    );
    drop(runtime);
    let reopened = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("reopen");
    assert_eq!(
        reopened.snapshot().expect("snapshot").turns[0]
            .child_report
            .as_ref()
            .expect("report")
            .outcomes[0]
            .status,
        CodexChildStatus::Failed
    );
}

fn running_fixture() -> (tempfile::TempDir, DesktopRuntime, PreparedCodexTurn) {
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("project");
    fs::create_dir(&root).expect("root");
    fs::write(root.join("file.txt"), "before\n").expect("initial");
    let mut runtime = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("runtime");
    let project_id = runtime.open_project(root).expect("project").projects[0]
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
            content: "fixture".into(),
            permission_level: PermissionLevel::Approval,
        })
        .expect("prepare");
    let task = prepared.task_id.to_string();
    let turn = prepared.turn_id.to_string();
    runtime
        .acquire_project_execution(&task, &turn, &prepared.project_root)
        .expect("baseline");
    (temp, runtime, prepared)
}

fn pending_fixture() -> (tempfile::TempDir, DesktopRuntime, PendingCompletion) {
    let (temp, mut runtime, prepared) = running_fixture();
    let task = prepared.task_id.to_string();
    let turn = prepared.turn_id.to_string();
    runtime
        .register_codex_turn(CodexTurnBinding {
            task_id: task.clone(),
            turn_id: turn.clone(),
            codex_thread_id: "root".into(),
            codex_turn_id: "native-turn".into(),
        })
        .expect("register");
    runtime.codex_turn_owners.insert(
        turn.clone(),
        CodexTurnOwner {
            kernel_key: "key".into(),
            instance_id: "instance".into(),
        },
    );
    let terminal = ProjectedTurnTerminal {
        task_id: task,
        turn_id: turn,
        status: ProjectedTurnStatus::Completed,
        error_message: None,
    };
    let Dispatch::Watch(pending) = runtime
        .defer_completion(&terminal, "instance")
        .expect("defer")
    else {
        panic!("must defer live completion");
    };
    (temp, runtime, *pending)
}

#[path = "codex_completion_native_tests.rs"]
mod native;

#[test]
fn pending_completion_keeps_lease_stop_target_and_running_state_until_final_observation() {
    let (temp, mut runtime, pending) = pending_fixture();
    let task = &pending.terminal.task_id;
    let turn = &pending.terminal.turn_id;
    let snapshot = runtime.snapshot().expect("snapshot");
    assert_eq!(snapshot.turns[0].status, "running");
    assert_eq!(snapshot.turns[0].phase, "waiting_children");
    assert!(
        runtime
            .codex_interrupt_target(turn)
            .expect("target")
            .is_some()
    );
    assert!(runtime.project_leases.contains_key(turn));
    assert!(runtime.project_execution_busy(task, None).expect("project remains busy"));
    assert!(
        runtime
            .storage
            .load_events(task)
            .expect("events")
            .iter()
            .all(|event| event.event_type != "review_after" && event.event_type != "turn_finished")
    );
    assert!(matches!(
        runtime
            .defer_completion(&pending.terminal, "instance")
            .expect("duplicate"),
        Dispatch::Ignore
    ));
    fs::write(
        temp.path().join("project/file.txt"),
        "child wrote after root finished\n",
    )
    .expect("late child write");
    assert!(
        runtime
            .seal_completion(&pending)
            .expect("quiet completion")
            .is_some()
    );
    assert!(
        runtime
            .seal_completion(&pending)
            .expect("duplicate completion")
            .is_none()
    );
    assert!(!runtime.project_leases.contains_key(turn));
    assert!(!runtime.storage.load_events(task).expect("events").iter().any(|event| event.event_type.starts_with("review_")));
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].status,
        "completed"
    );
}

#[test]
fn uncertain_check_never_releases_files_and_stale_instance_cannot_finalize_new_owner() {
    let (_temp, mut runtime, pending) = pending_fixture();
    runtime
        .completion_check_state(&pending, true)
        .expect("uncertain");
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].phase,
        "checking_completion"
    );
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].status,
        "running"
    );
    runtime
        .codex_turn_owners
        .get_mut(&pending.terminal.turn_id)
        .expect("owner")
        .instance_id = "replacement".into();
    assert!(
        runtime
            .seal_completion(&pending)
            .expect("stale check")
            .is_none()
    );
    assert!(
        runtime
            .project_leases
            .contains_key(&pending.terminal.turn_id)
    );
    assert!(
        !runtime
            .completion_check_state(&pending, false)
            .expect("stale update")
    );
}

#[test]
fn stop_while_waiting_is_cancelled_not_success_when_tree_finally_quiets() {
    let (_temp, mut runtime, pending) = pending_fixture();
    let target = runtime
        .codex_interrupt_target(&pending.terminal.turn_id)
        .expect("target")
        .expect("native target");
    runtime
        .note_completion_cancel_requested(&target)
        .expect("cancel intent");
    assert!(
        runtime
            .project_leases
            .contains_key(&pending.terminal.turn_id)
    );
    let terminal = runtime
        .seal_completion(&pending)
        .expect("quiet")
        .expect("sealed");
    assert_eq!(terminal.status, ProjectedTurnStatus::Cancelled);
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].status,
        "cancelled"
    );
}

#[test]
fn finalization_error_remains_uncertain_and_retry_commits_only_once() {
    let (_temp, mut runtime, pending) = pending_fixture();
    let turn = &pending.terminal.turn_id;
    // Invalid terminal identity injects a real finalization error, rather than
    // teaching a mock to return the desired state. Restore it for the retry.
    runtime
        .pending_codex_finishes
        .get_mut(turn)
        .expect("pending")
        .terminal
        .turn_id = "invalid".into();
    assert!(matches!(
        runtime
            .advance_completion(&pending, Ok(true))
            .expect("advance"),
        CompletionUpdate::Waiting { changed: true }
    ));
    assert!(runtime.project_leases.contains_key(turn));
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].phase,
        "checking_completion"
    );
    assert!(matches!(
        runtime
            .advance_completion(&pending, Ok(true))
            .expect("retry"),
        CompletionUpdate::Waiting { changed: false }
    ));
    assert!(
        !runtime
            .storage
            .load_events(&pending.terminal.task_id)
            .expect("events")
            .iter()
            .any(|event| event.event_type == "turn_finished")
    );
    runtime
        .pending_codex_finishes
        .get_mut(turn)
        .expect("pending")
        .terminal
        .turn_id = turn.clone();
    assert!(matches!(
        runtime
            .advance_completion(&pending, Ok(true))
            .expect("retry success"),
        CompletionUpdate::Sealed(_)
    ));
    assert!(matches!(
        runtime
            .advance_completion(&pending, Ok(true))
            .expect("duplicate"),
        CompletionUpdate::Obsolete
    ));
    assert_eq!(
        runtime
            .storage
            .load_events(&pending.terminal.task_id)
            .expect("events")
            .iter()
            .filter(|event| event.event_type == "turn_finished")
            .count(),
        1
    );
}
