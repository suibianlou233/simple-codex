use super::*;
use crate::secrets::MemorySecretStore;

fn fixture() -> (
    tempfile::TempDir,
    DesktopRuntime,
    CodexTurnBinding,
    CodexApprovalRequest,
) {
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("project");
    fs::create_dir(&root).expect("project");
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
    let binding = CodexTurnBinding {
        task_id: prepared.task_id.to_string(),
        turn_id: prepared.turn_id.to_string(),
        codex_thread_id: "root".into(),
        codex_turn_id: "root-turn".into(),
    };
    runtime
        .acquire_project_execution(&binding.task_id, &binding.turn_id, &prepared.project_root)
        .expect("baseline");
    runtime
        .register_codex_turn(binding.clone())
        .expect("register");
    runtime.codex_turn_owners.insert(
        binding.turn_id.clone(),
        CodexTurnOwner {
            kernel_key: "fixture".into(),
            instance_id: "instance".into(),
        },
    );
    let request = CodexApprovalRequest {
        request_id: json!(7),
        kind: CodexApprovalKind::CommandExecution,
        thread_id: "child".into(),
        turn_id: "child-turn".into(),
        item_id: "item".into(),
        approval_id: None,
        started_at_ms: 42,
        reason: None,
        command: Some("echo fixture".into()),
        cwd: None,
        params: json!({}),
    };
    (temp, runtime, binding, request)
}

#[test]
fn child_approval_stays_pending_and_child_completion_does_not_finish_root() {
    let (temp, mut runtime, root, request) = fixture();
    assert!(
        runtime
            .prepare_codex_approval(&request)
            .expect("unbound")
            .is_none()
    );
    assert!(
        runtime
            .bind_descendant(&root, "instance", &request)
            .expect("bind")
    );
    assert!(
        runtime
            .bind_descendant(&root, "instance", &request)
            .expect("duplicate")
    );
    let action = runtime
        .prepare_codex_approval(&request)
        .expect("prepare")
        .expect("visible approval");
    assert_eq!(runtime.actions[&action].task_id, root.task_id);
    assert_eq!(runtime.actions[&action].turn_id, root.turn_id);
    assert_eq!(runtime.actions[&action].status, ActionStatus::Pending);
    assert!(BackendAction::from(&runtime.actions[&action]).is_subagent);
    assert_eq!(
        runtime
            .storage
            .load_events(&root.task_id)
            .expect("events")
            .iter()
            .filter(|event| event.event_type == "codex_descendant_bound")
            .count(),
        1
    );
    runtime
        .resolve_codex_approval_action(&action, false)
        .expect("reject");
    let effects = runtime
        .project_codex_event(CodexKernelEvent::TurnCompleted {
            thread_id: request.thread_id,
            turn_id: request.turn_id,
            status: CodexTurnStatus::Completed,
            error_message: None,
            turn: json!({}),
        })
        .expect("child done");
    assert!(effects.is_empty());
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].status,
        "running"
    );
    assert!(runtime.project_leases.contains_key(&root.turn_id));
    drop(runtime);
    let reopened = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("reopen");
    assert_eq!(reopened.actions[&action].status, ActionStatus::Rejected);
    assert!(BackendAction::from(&reopened.actions[&action]).is_subagent);
    assert!(
        reopened.codex_descendants.is_empty(),
        "persisted evidence does not revive stale live authority"
    );
}

#[test]
fn ownership_changes_and_permission_modes_never_widen_child_authority() {
    let (_temp, mut runtime, root, request) = fixture();
    assert!(
        !runtime
            .bind_descendant(&root, "old-instance", &request)
            .expect("wrong instance")
    );
    assert!(
        runtime
            .bind_descendant(&root, "instance", &request)
            .expect("bind")
    );
    runtime.task_permissions.insert(
        parse_task_id(&root.task_id).expect("task"),
        PermissionLevel::ProjectFullAccess,
    );
    assert!(
        runtime
            .prepare_codex_approval(&request)
            .expect("policy")
            .is_none()
    );
    runtime
        .codex_turn_owners
        .get_mut(&root.turn_id)
        .expect("owner")
        .instance_id = "replacement".into();
    assert!(
        runtime
            .codex_action_binding(&request.thread_id, &request.turn_id)
            .is_none()
    );
    assert!(
        !runtime
            .bind_descendant(&root, "instance", &request)
            .expect("stale result")
    );
}
