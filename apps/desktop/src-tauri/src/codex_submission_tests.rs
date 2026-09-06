use super::*;
use crate::runtime::codex_submission;

#[test]
fn cancelled_preparation_cannot_cross_dispatch_boundary() {
    let (_temp, mut runtime, prepared) = fixture();
    let turn = prepared.turn_id.to_string();
    let token = CancellationToken::new();
    token.cancel();
    runtime.preparing_codex_turns.insert(turn, token);
    assert!(
        runtime
            .begin_codex_submission(&prepared, "thread", "instance")
            .is_err()
    );
    assert!(runtime.pending_codex_submissions.is_empty());
    assert!(
        !runtime
            .storage
            .load_events(&prepared.task_id.to_string())
            .expect("events")
            .iter()
            .any(|event| event.event_type == "codex_submission_dispatching")
    );
}

fn dispatch(
    runtime: &mut DesktopRuntime,
    prepared: &PreparedCodexTurn,
) -> codex_submission::PendingSubmission {
    runtime.codex_turn_owners.insert(
        prepared.turn_id.to_string(),
        CodexTurnOwner {
            kernel_key: "kernel".into(),
            instance_id: "instance".into(),
        },
    );
    runtime
        .begin_codex_submission(prepared, "thread", "instance")
        .expect("durable dispatch")
}

#[test]
fn uncertain_submission_retains_lease_and_stop_intent_across_restart() {
    let (temp, mut runtime, prepared) = fixture();
    runtime.preparing_codex_turns.insert(prepared.turn_id.to_string(), CancellationToken::new());
    let pending = dispatch(&mut runtime, &prepared);
    assert!(runtime.preparing_codex_turns.is_empty(), "dispatch atomically hands cancellation over to native recovery");
    let turn = prepared.turn_id.to_string();
    let task = prepared.task_id.to_string();
    assert!(matches!(
        runtime.mark_codex_submission_failed(prepared.task_id, prepared.turn_id, "fixture failure"),
        Err(DesktopError::SubmissionPending)
    ));
    assert!(runtime.project_leases.contains_key(&turn));
    assert!(runtime.project_execution_busy(&task, None).expect("busy"));
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].phase,
        "checking_submission"
    );
    assert!(runtime.request_submission_stop(&turn).expect("record stop"));
    assert!(
        runtime
            .request_submission_stop(&turn)
            .expect("idempotent stop")
    );
    assert!(
        matches!(
            runtime.complete_submission_handoff(&turn, false),
            Err(DesktopError::SubmissionPending)
        ),
        "handoff cannot race past a stop request"
    );
    let events = runtime.storage.load_events(&task).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "codex_submission_stop_requested")
            .count(),
        1
    );
    assert!(!events.iter().any(|event| matches!(
        event.event_type.as_str(),
        "turn_finished" | "review_after" | "codex_submission_failed"
    )));
    assert!(
        runtime
            .begin_codex_submission(&prepared, "thread", "instance")
            .is_err()
    );
    drop(runtime);
    let mut reopened = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("reopen");
    assert_eq!(
        reopened.snapshot().expect("snapshot").turns[0].status,
        "running"
    );
    assert_eq!(
        reopened.snapshot().expect("snapshot").turns[0].phase,
        "submission_recovery_required"
    );
    assert!(reopened.project_leases.contains_key(&turn));
    assert!(reopened.pending_codex_submissions[&turn].cancel_requested);
    assert!(
        !reopened.owns_submission(&pending),
        "saved instance id cannot become live ownership"
    );
    assert!(matches!(
        reopened.request_submission_stop(&turn),
        Err(DesktopError::SubmissionRecoveryUnavailable)
    ));
    assert!(
        reopened
            .project_execution_busy(&task, None)
            .expect("still busy")
    );
}

#[test]
fn failure_before_dispatch_still_finishes_without_creating_an_uncertain_task() {
    let (_temp, mut runtime, prepared) = fixture();
    runtime
        .mark_codex_submission_failed(prepared.task_id, prepared.turn_id, "fixture setup failure")
        .expect("failed before dispatch");
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].status,
        "failed"
    );
    assert!(runtime.pending_codex_submissions.is_empty());
    assert!(runtime.project_leases.is_empty());
}

#[test]
fn another_instance_cannot_claim_an_uncertain_submission() {
    let (_temp, mut runtime, prepared) = fixture();
    let pending = dispatch(&mut runtime, &prepared);
    runtime
        .codex_turn_owners
        .get_mut(&prepared.turn_id.to_string())
        .expect("owner")
        .instance_id = "replacement".into();
    assert!(!runtime.owns_submission(&pending));
    assert!(runtime.link_submission(&pending, "native", true).is_err());
    assert!(
        runtime
            .request_submission_stop(&prepared.turn_id.to_string())
            .is_err()
    );
    assert!(
        runtime
            .project_leases
            .contains_key(&prepared.turn_id.to_string())
    );
    assert!(
        !runtime
            .storage
            .load_events(&prepared.task_id.to_string())
            .expect("events")
            .iter()
            .any(|event| event.event_type == "codex_turn_linked")
    );
}

#[test]
fn link_retry_is_idempotent_and_cannot_rebind_the_submission() {
    let (temp, mut runtime, prepared) = fixture();
    let pending = dispatch(&mut runtime, &prepared);
    runtime
        .link_submission(&pending, "native", true)
        .expect("link");
    runtime
        .link_submission(&pending, "native", true)
        .expect("retry");
    assert!(runtime.link_submission(&pending, "other", true).is_err());
    let task = prepared.task_id.to_string();
    assert_eq!(
        runtime
            .storage
            .load_events(&task)
            .expect("events")
            .iter()
            .filter(|event| event.event_type == "codex_turn_linked")
            .count(),
        1
    );
    runtime
        .complete_submission_handoff(&prepared.turn_id.to_string(), false)
        .expect("handoff");
    assert!(
        runtime
            .project_leases
            .contains_key(&prepared.turn_id.to_string()),
        "registration isn't completion"
    );
    drop(runtime);
    let reopened = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("repeated link remains readable");
    assert!(reopened.pending_codex_submissions.is_empty());
}

#[derive(Clone)]
struct ReadOnlyRpc {
    responses: Arc<Mutex<VecDeque<Value>>>,
    methods: Arc<Mutex<Vec<String>>>,
}
#[async_trait::async_trait]
impl local_agent_model::CodexRpc for ReadOnlyRpc {
    async fn request(&self, method: &str, _params: Value) -> Result<Value, CodexKernelError> {
        assert!(
            matches!(
                method,
                "thread/read" | "thread/items/list" | "thread/turns/list"
            ),
            "recovery must remain read only"
        );
        self.methods.lock().expect("calls").push(method.into());
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .ok_or(CodexKernelError::Unavailable)
    }
}

#[tokio::test]
async fn delayed_recovery_hydrates_real_status_without_resubmitting() {
    let (_temp, mut runtime, prepared) = fixture();
    let pending = dispatch(&mut runtime, &prepared);
    runtime
        .storage
        .bind_codex_thread(NewCodexThreadBinding {
            task_id: prepared.task_id.to_string(),
            codex_thread_id: "thread".into(),
            model_profile_id: prepared.profile.profile_id.clone(),
            created_at_ms: 1,
        })
        .expect("thread binding");
    let user = json!({"turnId":"native","item":{"id":"user","type":"userMessage","clientId":prepared.user_message_id,"content":[]}});
    let rpc = ReadOnlyRpc {
        responses: Arc::new(Mutex::new(
            vec![
                json!({"thread":{"id":"thread"}}),
                json!({"data":[user.clone()],"nextCursor":null}),
                json!({"thread":{"id":"thread"}}),
                json!({"data":[{"id":"native","status":"completed"}],"nextCursor":null}),
                json!({"data":[user],"nextCursor":null}),
            ]
            .into(),
        )),
        methods: Arc::new(Mutex::new(Vec::new())),
    };
    let runtime = Arc::new(Mutex::new(runtime));
    let effects = codex_submission::recover_once(&runtime, &rpc, &pending)
        .await
        .expect("reconcile")
        .expect("found");
    assert!(effects.iter().any(|effect|matches!(effect,CodexDesktopEffect::Finish(terminal) if terminal.status==ProjectedTurnStatus::Completed)));
    let runtime = runtime.lock().expect("runtime");
    assert!(
        runtime
            .project_leases
            .contains_key(&prepared.turn_id.to_string())
    );
    assert!(
        runtime
            .pending_codex_submissions
            .contains_key(&prepared.turn_id.to_string()),
        "must retain until effects commit"
    );
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].status,
        "running"
    );
    assert_eq!(rpc.methods.lock().expect("methods").len(), 5);
}

#[test]
fn explicit_native_terminal_clears_unknown_marker_but_does_not_hide_stop_intent() {
    let (temp, mut runtime, prepared) = fixture();
    let pending = dispatch(&mut runtime, &prepared);
    runtime
        .link_submission(&pending, "native", true)
        .expect("link");
    runtime
        .request_submission_stop(&prepared.turn_id.to_string())
        .expect("stop");
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    assert_eq!(
        runtime.snapshot().expect("snapshot").turns[0].status,
        "cancelled"
    );
    assert!(runtime.project_leases.is_empty());
    assert!(runtime.pending_codex_submissions.is_empty());
    drop(runtime);
    let reopened = DesktopRuntime::open(
        &temp.path().join("data.db"),
        Arc::new(MemorySecretStore::new()),
    )
    .expect("reopen");
    assert!(reopened.pending_codex_submissions.is_empty());
}
