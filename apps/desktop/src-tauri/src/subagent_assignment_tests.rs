use super::*;
use crate::runtime::subagent_report::DispatchStatus;

fn item(id: &str, status: &str, receivers: &[&str]) -> CodexThreadItem {
    CodexThreadItem {
        id: id.into(),
        kind: "collabAgentToolCall".into(),
        value: json!({
            "tool":"spawnAgent", "senderThreadId":"thread", "receiverThreadIds":receivers,
            "status":status, "prompt":"检查输入法组合态，不修改其他模块。"
        }),
    }
}

fn register(runtime: &mut DesktopRuntime, prepared: &PreparedCodexTurn) {
    runtime
        .register_codex_turn(CodexTurnBinding {
            task_id: prepared.task_id.to_string(),
            turn_id: prepared.turn_id.to_string(),
            codex_thread_id: "thread".into(),
            codex_turn_id: "native-turn".into(),
        })
        .expect("register");
}

#[test]
fn assignment_receipts_do_not_claim_child_completion_and_survive_restart() {
    let (temp, mut runtime, prepared) = fixture();
    register(&mut runtime, &prepared);
    let turn = prepared.turn_id.to_string();
    let task = prepared.task_id.to_string();
    let begin = item("spawn", "inProgress", &[]);
    let end = item("spawn", "completed", &["child"]);
    assert!(
        runtime
            .record_assignment("thread", "native-turn", &begin, false)
            .expect("begin")
            .is_some()
    );
    assert!(
        runtime
            .record_assignment("thread", "native-turn", &end, true)
            .expect("end")
            .is_some()
    );
    assert!(
        runtime
            .record_assignment("thread", "native-turn", &begin, false)
            .expect("late begin")
            .is_none()
    );
    assert!(
        runtime
            .record_assignment("thread", "native-turn", &end, true)
            .expect("duplicate")
            .is_none()
    );
    let report = &runtime.child_result_reports[&turn];
    assert_eq!(report.assignments.len(), 1);
    assert_eq!(report.assignments[0].status, DispatchStatus::Dispatched);
    assert!(
        report.outcomes.is_empty(),
        "dispatch is not a completed child"
    );
    runtime
        .record_child_report(
            &task,
            &turn,
            vec![local_agent_model::CodexChildOutcome {
                thread_id: "child".into(),
                turn_id: Some("child-turn".into()),
                status: local_agent_model::CodexChildStatus::Failed,
            }],
        )
        .expect("final observation");
    let expected = runtime.child_result_reports[&turn].clone();
    assert_eq!(
        runtime
            .storage
            .load_events(&task)
            .expect("events")
            .iter()
            .filter(|event| event.event_type == "codex_child_report")
            .count(),
        3
    );
    drop(runtime);
    let reopened = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("reopen");
    assert_eq!(reopened.child_result_reports[&turn], expected);
    assert_eq!(
        reopened.snapshot().expect("snapshot").turns[0]
            .child_report
            .as_ref(),
        Some(&expected)
    );
}

#[test]
fn failed_and_missing_dispatch_receipts_are_not_successes() {
    let (_temp, mut runtime, prepared) = fixture();
    register(&mut runtime, &prepared);
    for (id, status, expected) in [
        ("failed", "failed", DispatchStatus::Failed),
        ("missing", "completed", DispatchStatus::Unknown),
        ("unknown", "newStatus", DispatchStatus::Unknown),
    ] {
        runtime
            .record_assignment("thread", "native-turn", &item(id, status, &[]), true)
            .expect("record");
        assert_eq!(
            runtime.child_result_reports[&prepared.turn_id.to_string()]
                .assignments
                .last()
                .expect("assignment")
                .status,
            expected
        );
    }
}

#[test]
fn assignment_source_and_receivers_are_validated_without_granting_ownership() {
    let (_temp, mut runtime, prepared) = fixture();
    register(&mut runtime, &prepared);
    assert!(
        runtime
            .record_assignment(
                "foreign",
                "native-turn",
                &item("x", "completed", &["child"]),
                true
            )
            .expect("foreign ignored")
            .is_none()
    );
    let mut bad = item("bad", "completed", &["child"]);
    bad.value["senderThreadId"] = json!("foreign");
    assert!(
        runtime
            .record_assignment("thread", "native-turn", &bad, true)
            .is_err()
    );
    bad.value["senderThreadId"] = json!("thread");
    bad.value["receiverThreadIds"] = json!([17]);
    assert!(
        runtime
            .record_assignment("thread", "native-turn", &bad, true)
            .is_err()
    );
    assert!(runtime.child_result_reports.is_empty());
    runtime
        .record_assignment(
            "thread",
            "native-turn",
            &item("ok", "completed", &["child"]),
            true,
        )
        .expect("record");
    assert!(
        runtime.codex_action_binding("child", "some-turn").is_none(),
        "display records cannot authorize child actions"
    );
}

#[test]
fn assignment_instructions_are_redacted_before_bounding_and_followups_stay_separate() {
    let (_temp, mut runtime, prepared) = fixture();
    register(&mut runtime, &prepared);
    let mut spawn = item("one", "completed", &["child"]);
    spawn.value["prompt"] = json!(format!(
        "Authorization: Bearer NEVER_STORE_THIS\n{}",
        "测".repeat(5000)
    ));
    runtime
        .record_assignment("thread", "native-turn", &spawn, true)
        .expect("spawn");
    let mut followup = item("two", "completed", &["child"]);
    followup.value["tool"] = json!("sendInput");
    runtime
        .record_assignment("thread", "native-turn", &followup, true)
        .expect("followup");
    let report = &runtime.child_result_reports[&prepared.turn_id.to_string()];
    assert_eq!(report.assignments.len(), 2);
    assert!(report.assignments[1].follow_up);
    assert!(report.assignments[0].instruction.chars().count() < 4200);
    assert!(report.assignments[0].instruction.contains("截短"));
    let events = serde_json::to_string(
        &runtime
            .storage
            .load_events(&prepared.task_id.to_string())
            .expect("events"),
    )
    .expect("json");
    assert!(!events.contains("NEVER_STORE_THIS"));
}

#[test]
fn early_assignment_replays_through_native_event_path_exactly_once() {
    let (_temp, mut runtime, prepared) = fixture();
    let event = CodexKernelEvent::ItemCompleted {
        thread_id: "thread".into(),
        turn_id: "native-turn".into(),
        completed_at_ms: 1,
        item: item("spawn", "completed", &["child"]),
    };
    assert!(
        runtime
            .project_codex_event(event.clone())
            .expect("early")
            .is_empty()
    );
    let effects = runtime
        .register_codex_turn(CodexTurnBinding {
            task_id: prepared.task_id.to_string(),
            turn_id: prepared.turn_id.to_string(),
            codex_thread_id: "thread".into(),
            codex_turn_id: "native-turn".into(),
        })
        .expect("replay");
    assert!(effects.iter().any(|effect| matches!(effect, CodexDesktopEffect::Stream(stream) if stream.kind == "delegations_updated")));
    runtime.project_codex_event(event).expect("duplicate");
    assert_eq!(
        runtime.child_result_reports[&prepared.turn_id.to_string()]
            .assignments
            .len(),
        1
    );
}
