use super::*;

fn applied_action(runtime: &mut DesktopRuntime, prepared: &PreparedCodexTurn) -> String {
    let workspace = Workspace::open(&prepared.project_root).expect("workspace");
    let original = workspace.read_file("file.txt").expect("original");
    let preview = workspace
        .preview_write("file.txt", "after\n".into(), Some(&original.sha256))
        .expect("preview");
    workspace.apply_write(&preview).expect("apply fixture edit");
    let id = Uuid::new_v4().to_string();
    runtime
        .save_action(DurableAction {
            id: id.clone(),
            task_id: prepared.task_id.to_string(),
            turn_id: prepared.turn_id.to_string(),
            tool_call_id: "legacy-write".into(),
            idempotency_key: format!("fixture/{id}"),
            payload: ActionPayload::WriteFile { preview },
            status: ActionStatus::Applied,
            operation: None,
            result: Some("applied".into()),
            created_at_ms: unix_time_ms().expect("time"),
        })
        .expect("persist legacy-compatible action");
    id
}

fn assert_busy_without_side_effects(
    runtime: &mut DesktopRuntime,
    prepared: &PreparedCodexTurn,
    id: &str,
) {
    let task = prepared.task_id.to_string();
    let event_count = runtime.storage.load_events(&task).expect("events").len();
    assert!(matches!(
        runtime.undo_action(id),
        Err(DesktopError::ProjectBusy)
    ));
    assert_eq!(
        fs::read(prepared.project_root.join("file.txt")).expect("preserved"),
        b"after\n"
    );
    assert_eq!(
        runtime
            .storage
            .load_events(&task)
            .expect("no undo intent")
            .len(),
        event_count
    );
    assert_eq!(runtime.actions[id].status, ActionStatus::Applied);
}

#[test]
fn legacy_undo_cannot_write_during_a_running_turn() {
    let (_temp, mut runtime, prepared) = fixture();
    let action = applied_action(&mut runtime, &prepared);
    assert_busy_without_side_effects(&mut runtime, &prepared, &action);
    assert!(!runtime.snapshot().expect("snapshot").actions[0].can_undo);
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    assert!(runtime.snapshot().expect("snapshot").actions[0].can_undo);
    runtime
        .undo_action(&action)
        .expect("undo after confirmed finish");
    assert_eq!(
        fs::read(prepared.project_root.join("file.txt")).expect("restored"),
        b"before\n"
    );
}

#[test]
fn legacy_undo_cannot_bypass_an_uncertain_submission_after_restart() {
    let (temp, mut runtime, prepared) = fixture();
    let action = applied_action(&mut runtime, &prepared);
    let turn = prepared.turn_id.to_string();
    runtime.codex_turn_owners.insert(
        turn.clone(),
        CodexTurnOwner {
            kernel_key: "fixture".into(),
            instance_id: "fixture-instance".into(),
        },
    );
    runtime
        .begin_codex_submission(&prepared, "fixture-thread", "fixture-instance")
        .expect("dispatch boundary");
    drop(runtime);
    let mut reopened = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("reopen");
    assert_eq!(
        reopened.snapshot().expect("state").turns[0].phase,
        "submission_recovery_required"
    );
    assert_busy_without_side_effects(&mut reopened, &prepared, &action);
    assert!(!reopened.snapshot().expect("snapshot").actions[0].can_undo);
    assert!(reopened.pending_codex_submissions.contains_key(&turn));
}

#[test]
fn legacy_undo_respects_other_installations_project_lease_and_can_retry() {
    let (temp, mut first, prepared) = fixture();
    let action = applied_action(&mut first, &prepared);
    finish(&mut first, &prepared, ProjectedTurnStatus::Completed);
    let other_data = temp.path().join("another-installation");
    fs::create_dir(&other_data).expect("data");
    let (mut other, active) = prepare_at(&other_data, &prepared.project_root);
    other
        .acquire_project_execution(
            &active.task_id.to_string(),
            &active.turn_id.to_string(),
            &active.project_root,
        )
        .expect("competing writer");
    assert_busy_without_side_effects(&mut first, &prepared, &action);
    finish(&mut other, &active, ProjectedTurnStatus::Completed);
    first
        .undo_action(&action)
        .expect("retry was not consumed while blocked");
    assert_eq!(
        fs::read(prepared.project_root.join("file.txt")).expect("restored"),
        b"before\n"
    );
}

#[test]
fn legacy_undo_still_preserves_user_edits_after_writer_releases_project() {
    let (_temp, mut runtime, prepared) = fixture();
    let action = applied_action(&mut runtime, &prepared);
    assert_busy_without_side_effects(&mut runtime, &prepared, &action);
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    fs::write(
        prepared.project_root.join("file.txt"),
        "user changed this\n",
    )
    .expect("manual edit");
    runtime
        .undo_action(&action)
        .expect("snapshot reports hash conflict");
    assert_eq!(runtime.actions[&action].status, ActionStatus::Failed);
    assert_eq!(
        fs::read(prepared.project_root.join("file.txt")).expect("user edit preserved"),
        b"user changed this\n"
    );
}

#[test]
fn legacy_undo_is_project_scoped_not_just_action_turn_or_global_activity() {
    for same_project in [true, false] {
        let (temp, mut runtime, prepared) = fixture();
        let action = applied_action(&mut runtime, &prepared);
        finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
        let root = if same_project {
            prepared.project_root.clone()
        } else {
            let root = temp.path().join("independent-project");
            fs::create_dir(&root).expect("independent root");
            root
        };
        runtime.open_project(root.clone()).expect("project");
        let root = fs::canonicalize(root).expect("canonical");
        let project_id = runtime
            .core
            .snapshot()
            .projects
            .into_iter()
            .find(|project| fs::canonicalize(&project.root).expect("project root") == root)
            .expect("matching project")
            .id
            .to_string();
        runtime
            .prepare_codex_new_chat(&StartChatInput {
                project_id,
                profile_id: None,
                content: "another task".into(),
                permission_level: PermissionLevel::Approval,
            })
            .expect("new task before acquiring native lease");
        assert_eq!(
            runtime
                .project_backend_action(&runtime.actions[&action])
                .expect("single action")
                .can_undo,
            !same_project
        );
        if same_project {
            assert_busy_without_side_effects(&mut runtime, &prepared, &action);
        } else {
            runtime
                .undo_action(&action)
                .expect("unrelated project must not block undo");
            assert_eq!(
                fs::read(prepared.project_root.join("file.txt")).expect("restored"),
                b"before\n"
            );
        }
    }
}
