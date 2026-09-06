use local_agent_storage::ActionClaimStatus;
use local_agent_storage::ActionExecutionResult;
use local_agent_storage::ActionRecoveryStatus;
use local_agent_storage::ApprovalDeclaration;
use local_agent_storage::BeginActionOutcome;
use local_agent_storage::NewActionIntent;
use local_agent_storage::NewJournalEvent;
use local_agent_storage::PrepareActionOutcome;
use local_agent_storage::Storage;
use local_agent_storage::StorageError;
use local_agent_storage::ThreadSnapshot;
use local_agent_storage::journal_event_types;
use serde_json::json;
use tempfile::tempdir;

fn write_intent(approval: bool) -> NewActionIntent {
    NewActionIntent {
        event_id: "intent-event".to_owned(),
        action_id: "action-1".to_owned(),
        idempotency_key: "write:README.md:old-hash:new-hash".to_owned(),
        thread_id: "thread-1".to_owned(),
        turn_id: Some("turn-1".to_owned()),
        step_id: Some("step-1".to_owned()),
        schema_version: 3,
        action_kind: "write_workspace".to_owned(),
        payload: json!({"path": "README.md", "expected_hash": "old-hash"}),
        approval: approval.then(|| ApprovalDeclaration {
            event_id: "approval-declared".to_owned(),
            approval_id: "approval-1".to_owned(),
            approval_kind: "file_change".to_owned(),
            payload: json!({"summary": "Update README"}),
        }),
        created_at_ms: 10,
    }
}

#[test]
fn intent_and_approval_declaration_are_one_atomic_unit() {
    let mut storage = Storage::open_in_memory().expect("open database");
    let prepared = storage
        .prepare_action_intent(write_intent(true))
        .expect("prepare action");
    let PrepareActionOutcome::Prepared(claim) = prepared else {
        panic!("first preparation must acquire the claim");
    };
    assert_eq!(claim.status, ActionClaimStatus::PendingApproval);
    assert_eq!(claim.schema_version, 3);

    let events = storage
        .read_journal_after("thread-1", 0)
        .expect("read journal");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, journal_event_types::ACTION_PROPOSED);
    assert_eq!(events[1].event_type, journal_event_types::APPROVAL_DECLARED);
    assert_eq!(events[0].sequence, 1);
    assert_eq!(events[1].sequence, 2);
    assert_eq!(events[0].schema_version, 3);
    assert_eq!(events[1].schema_version, 3);

    let recovery = storage
        .recovery_view("thread-1")
        .expect("rebuild approval state from journal");
    assert_eq!(recovery.actions[0].status, ActionRecoveryStatus::Pending);
}

#[test]
fn replay_distinguishes_ready_intents_from_pending_approval() {
    let mut storage = Storage::open_in_memory().expect("open database");
    storage
        .prepare_action_intent(write_intent(false))
        .expect("prepare action without approval");

    let recovery = storage
        .recovery_view("thread-1")
        .expect("rebuild ready action from journal");
    assert_eq!(recovery.actions.len(), 1);
    assert_eq!(recovery.actions[0].status, ActionRecoveryStatus::Ready);
}

#[test]
fn failure_to_persist_declaration_rolls_back_intent_and_claim() {
    let mut storage = Storage::open_in_memory().expect("open database");
    storage
        .append_journal_event(NewJournalEvent {
            event_id: "approval-declared".to_owned(),
            thread_id: "other-thread".to_owned(),
            turn_id: None,
            step_id: None,
            schema_version: 1,
            event_type: "existing".to_owned(),
            payload: json!({}),
            created_at_ms: 1,
        })
        .expect("seed duplicate event id");

    let result = storage.prepare_action_intent(write_intent(true));
    assert!(matches!(
        result,
        Err(StorageError::DuplicateJournalEvent(event_id))
            if event_id == "approval-declared"
    ));
    assert!(
        storage
            .read_journal_after("thread-1", 0)
            .expect("read rolled back stream")
            .is_empty()
    );
    assert!(
        storage
            .get_action_claim("write:README.md:old-hash:new-hash")
            .expect("read claim")
            .is_none()
    );
}

#[test]
fn preparation_is_idempotent_and_conflicting_reuse_is_rejected() {
    let mut storage = Storage::open_in_memory().expect("open database");
    storage
        .prepare_action_intent(write_intent(false))
        .expect("first preparation");
    let repeated = storage
        .prepare_action_intent(write_intent(false))
        .expect("repeat preparation");
    assert!(matches!(repeated, PrepareActionOutcome::Existing(_)));
    assert_eq!(
        storage
            .read_journal_after("thread-1", 0)
            .expect("read events")
            .len(),
        1
    );

    let mut conflict = write_intent(false);
    conflict.action_id = "different-action".to_owned();
    conflict.event_id = "different-event".to_owned();
    assert!(matches!(
        storage.prepare_action_intent(conflict),
        Err(StorageError::IdempotencyConflict { .. })
    ));

    let mut changed_payload = write_intent(false);
    changed_payload.event_id = "retry-with-different-payload".to_owned();
    changed_payload.payload = json!({"path": "README.md", "expected_hash": "other-hash"});
    assert!(matches!(
        storage.prepare_action_intent(changed_payload),
        Err(StorageError::IdempotencyDefinitionConflict { .. })
    ));

    let mut changed_approval_requirement = write_intent(true);
    changed_approval_requirement.event_id = "retry-with-approval".to_owned();
    assert!(matches!(
        storage.prepare_action_intent(changed_approval_requirement),
        Err(StorageError::IdempotencyDefinitionConflict { .. })
    ));
}

#[test]
fn approval_and_execution_follow_durable_state_transitions() {
    let mut storage = Storage::open_in_memory().expect("open database");
    storage
        .prepare_action_intent(write_intent(true))
        .expect("prepare action");
    let approved = storage
        .resolve_action_approval(
            "write:README.md:old-hash:new-hash",
            "approval-granted",
            true,
            json!({"decided_by": "user"}),
            11,
        )
        .expect("approve action");
    assert_eq!(approved.status, ActionClaimStatus::Ready);

    let approval_retry = storage
        .resolve_action_approval(
            "write:README.md:old-hash:new-hash",
            "approval-granted",
            true,
            json!({"decided_by": "user"}),
            11,
        )
        .expect("retry committed approval");
    assert_eq!(approval_retry.status, ActionClaimStatus::Ready);

    let begun = storage
        .begin_action_execution(
            "write:README.md:old-hash:new-hash",
            "action-started",
            json!({}),
            12,
        )
        .expect("begin execution");
    assert!(matches!(
        begun,
        BeginActionOutcome::Execute(ref claim)
            if claim.status == ActionClaimStatus::Executing
    ));

    let repeated = storage
        .begin_action_execution(
            "write:README.md:old-hash:new-hash",
            "must-not-be-appended",
            json!({}),
            13,
        )
        .expect("repeat begin");
    assert!(matches!(
        repeated,
        BeginActionOutcome::DoNotExecute(ref claim)
            if claim.status == ActionClaimStatus::Executing
    ));

    let finished = storage
        .finish_action_execution(
            "write:README.md:old-hash:new-hash",
            "action-completed",
            ActionExecutionResult::Completed,
            json!({"new_hash": "new-hash"}),
            14,
        )
        .expect("finish action");
    assert_eq!(finished.status, ActionClaimStatus::Completed);
    let finish_retry = storage
        .finish_action_execution(
            "write:README.md:old-hash:new-hash",
            "action-completed",
            ActionExecutionResult::Completed,
            json!({"new_hash": "new-hash"}),
            14,
        )
        .expect("retry committed completion");
    assert_eq!(finish_retry.status, ActionClaimStatus::Completed);
    assert_eq!(
        storage
            .read_journal_after("thread-1", 0)
            .expect("read lifecycle")
            .len(),
        5
    );
}

#[test]
fn restart_never_reissues_an_execution_with_uncertain_outcome() {
    let directory = tempdir().expect("temporary directory");
    let database = directory.path().join("state.sqlite3");
    {
        let mut storage = Storage::open(&database).expect("open storage");
        storage
            .prepare_action_intent(write_intent(false))
            .expect("prepare action");
        let begun = storage
            .begin_action_execution(
                "write:README.md:old-hash:new-hash",
                "action-started",
                json!({}),
                11,
            )
            .expect("begin action");
        assert!(matches!(begun, BeginActionOutcome::Execute(_)));
        // Simulate a crash after the external effect may have started but
        // before its completion fact could be committed.
    }

    let mut reopened = Storage::open(&database).expect("reopen storage");
    let decision = reopened
        .begin_action_execution(
            "write:README.md:old-hash:new-hash",
            "action-started-again",
            json!({}),
            12,
        )
        .expect("recover execution claim");
    assert!(matches!(
        decision,
        BeginActionOutcome::DoNotExecute(ref claim)
            if claim.status == ActionClaimStatus::Executing
    ));
    assert_eq!(
        reopened
            .read_journal_after("thread-1", 0)
            .expect("read events")
            .len(),
        2
    );
}

#[test]
fn concurrent_preparation_acquires_one_idempotency_claim() {
    let directory = tempdir().expect("temporary directory");
    let database = directory.path().join("state.sqlite3");
    drop(Storage::open(&database).expect("initialize database"));

    let barrier = Arc::new(Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let database = database.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut storage = Storage::open(database).expect("open writer");
                barrier.wait();
                storage
                    .prepare_action_intent(write_intent(false))
                    .expect("prepare concurrent intent")
            })
        })
        .collect::<Vec<_>>();

    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().expect("join writer"))
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, PrepareActionOutcome::Prepared(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, PrepareActionOutcome::Existing(_)))
            .count(),
        1
    );

    let storage = Storage::open(&database).expect("reopen database");
    assert_eq!(
        storage
            .read_journal_after("thread-1", 0)
            .expect("read journal")
            .len(),
        1
    );
}

#[test]
fn replay_starts_strictly_after_the_latest_snapshot() {
    let mut storage = Storage::open_in_memory().expect("open database");
    let mut first = write_intent(false);
    first.approval = None;
    storage
        .prepare_action_intent(first)
        .expect("append first event");
    storage
        .save_thread_snapshot(ThreadSnapshot {
            thread_id: "thread-1".to_owned(),
            through_sequence: 1,
            schema_version: 1,
            payload: json!({"projection": "at-one"}),
            created_at_ms: 11,
        })
        .expect("save snapshot");
    storage
        .begin_action_execution(
            "write:README.md:old-hash:new-hash",
            "action-started",
            json!({}),
            12,
        )
        .expect("append tail event");

    let replay = storage.load_thread_replay("thread-1").expect("load replay");
    assert_eq!(replay.snapshot.expect("snapshot").through_sequence, 1);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].sequence, 2);
    assert_eq!(replay.through_sequence, 2);
}
use std::sync::Arc;
use std::sync::Barrier;
use std::thread;
