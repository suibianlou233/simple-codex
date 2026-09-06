use super::*;

fn binding(prepared: &PreparedCodexTurn) -> CodexTurnBinding {
    CodexTurnBinding {
        task_id: prepared.task_id.to_string(),
        turn_id: prepared.turn_id.to_string(),
        codex_thread_id: "thread".into(),
        codex_turn_id: "native-turn".into(),
    }
}

fn completed(thread: &str, id: &str, kind: &str, value: Value) -> CodexKernelEvent {
    CodexKernelEvent::ItemCompleted {
        thread_id: thread.into(),
        turn_id: "native-turn".into(),
        completed_at_ms: 1,
        item: CodexThreadItem {
            id: id.into(),
            kind: kind.into(),
            value,
        },
    }
}

#[test]
fn early_tool_and_spawn_events_replay_durable_effects_not_only_text() {
    let (temp, mut runtime, prepared) = fixture();
    let command = completed(
        "thread",
        "command",
        "commandExecution",
        json!({"command":"fixture only","status":"failed","exitCode":1,"aggregatedOutput":"fixture failure"}),
    );
    let spawn = completed(
        "thread",
        "spawn",
        "collabAgentToolCall",
        json!({"tool":"spawnAgent","receiverThreadIds":["child"]}),
    );
    for event in [command.clone(), spawn.clone()] {
        assert!(
            runtime
                .project_codex_event(event)
                .expect("early event")
                .is_empty()
        );
    }
    assert!(runtime.actions.is_empty());
    let _effects = runtime
        .register_codex_turn(binding(&prepared))
        .expect("register");
    assert_eq!(
        runtime.actions.len(),
        1,
        "early failed command must be persisted"
    );
    assert_eq!(
        runtime.actions.values().next().expect("action").status,
        ActionStatus::Failed
    );
    let task = prepared.task_id.to_string();
    let turn = prepared.turn_id.to_string();
    assert!(
        runtime
            .spawned_children(&task, &turn)
            .expect("children")
            .contains("child")
    );
    for event in [command, spawn] {
        runtime.project_codex_event(event).expect("duplicate");
    }
    assert_eq!(runtime.actions.len(), 1);
    assert_eq!(
        runtime
            .storage
            .load_events(&task)
            .expect("events")
            .iter()
            .filter(|event| event.event_type == "codex_spawn_observed")
            .count(),
        1
    );
    drop(runtime);
    let reopened = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("reopen");
    assert_eq!(
        reopened
            .actions
            .values()
            .next()
            .expect("durable action")
            .status,
        ActionStatus::Failed
    );
    assert!(
        reopened
            .spawned_children(&task, &turn)
            .expect("durable children")
            .contains("child")
    );
}

#[test]
fn replay_matches_both_thread_and_turn_and_retains_unrelated_events() {
    let (_temp, mut runtime, prepared) = fixture();
    runtime
        .project_codex_event(completed(
            "unrelated",
            "foreign-spawn",
            "collabAgentToolCall",
            json!({"tool":"spawnAgent","receiverThreadIds":["foreign"]}),
        ))
        .expect("buffer");
    runtime
        .register_codex_turn(binding(&prepared))
        .expect("register");
    assert_eq!(runtime.pending_codex_events.len(), 1);
    assert!(
        runtime
            .spawned_children(&prepared.task_id.to_string(), &prepared.turn_id.to_string())
            .expect("children")
            .is_empty()
    );
}

#[test]
fn failed_replay_keeps_the_original_queue_instead_of_dropping_evidence() {
    let (_temp, mut runtime, prepared) = fixture();
    runtime
        .project_codex_event(completed(
            "thread",
            "bad-spawn",
            "collabAgentToolCall",
            json!({"tool":"spawnAgent","receiverThreadIds":[""]}),
        ))
        .expect("unbound buffer");
    assert!(runtime.register_codex_turn(binding(&prepared)).is_err());
    assert_eq!(runtime.pending_codex_events.len(), 1);
    assert!(
        runtime
            .project_leases
            .contains_key(&prepared.turn_id.to_string())
    );
}

#[tokio::test]
async fn handoff_is_instance_scoped_and_does_not_hold_the_runtime_mutex() {
    let (_temp, mut runtime, _prepared) = fixture();
    let gate = runtime.codex_event_gate("first");
    let other = runtime.codex_event_gate("second");
    let same = runtime.codex_event_gate("first");
    let guard = gate.lock().await;
    assert!(same.try_lock().is_err());
    assert!(other.try_lock().is_ok());
    drop(guard);
    assert!(same.try_lock().is_ok());
}
