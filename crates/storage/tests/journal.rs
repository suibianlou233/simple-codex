use std::sync::Arc;
use std::sync::Barrier;
use std::thread;

use local_agent_storage::ActionRecoveryStatus;
use local_agent_storage::NewJournalEvent;
use local_agent_storage::Storage;
use local_agent_storage::StorageError;
use local_agent_storage::ThreadSnapshot;
use local_agent_storage::journal_event_types;
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

fn event(
    event_id: &str,
    thread_id: &str,
    turn_id: Option<&str>,
    step_id: Option<&str>,
    event_type: &str,
    payload: serde_json::Value,
    created_at_ms: i64,
) -> NewJournalEvent {
    NewJournalEvent {
        event_id: event_id.to_owned(),
        thread_id: thread_id.to_owned(),
        turn_id: turn_id.map(str::to_owned),
        step_id: step_id.map(str::to_owned),
        schema_version: 1,
        event_type: event_type.to_owned(),
        payload,
        created_at_ms,
    }
}

#[test]
fn batch_append_is_atomic_and_read_after_is_incremental() {
    let mut storage = Storage::open_in_memory().expect("open database");
    let records = storage
        .append_journal_events(vec![
            event(
                "event-1",
                "thread-1",
                Some("turn-1"),
                None,
                journal_event_types::TURN_STARTED,
                json!({}),
                10,
            ),
            event(
                "event-2",
                "thread-1",
                Some("turn-1"),
                Some("step-1"),
                "model.requested",
                json!({"model": "configured-model"}),
                11,
            ),
        ])
        .expect("append batch");
    assert_eq!(
        records
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        [1, 2]
    );

    let duplicate = storage.append_journal_events(vec![
        event(
            "event-3",
            "thread-1",
            Some("turn-1"),
            None,
            "message.delta",
            json!({}),
            12,
        ),
        event(
            "event-1",
            "thread-1",
            Some("turn-1"),
            None,
            "message.completed",
            json!({}),
            13,
        ),
    ]);
    assert!(matches!(
        duplicate,
        Err(StorageError::DuplicateJournalEvent(id)) if id == "event-1"
    ));

    let after_first = storage
        .read_journal_after("thread-1", 1)
        .expect("read incrementally");
    assert_eq!(after_first.len(), 1);
    assert_eq!(after_first[0].event_id, "event-2");
    assert_eq!(after_first[0].schema_version, 1);
    assert_eq!(after_first[0].step_id.as_deref(), Some("step-1"));
}

#[test]
fn thread_sequences_are_independent_inside_one_atomic_batch() {
    let mut storage = Storage::open_in_memory().expect("open database");
    let records = storage
        .append_journal_events(vec![
            event("a-1", "a", None, None, "custom", json!({}), 1),
            event("b-1", "b", None, None, "custom", json!({}), 2),
            event("a-2", "a", None, None, "custom", json!({}), 3),
        ])
        .expect("append mixed batch");

    assert_eq!(
        records
            .iter()
            .map(|event| (event.thread_id.as_str(), event.sequence))
            .collect::<Vec<_>>(),
        [("a", 1), ("b", 1), ("a", 2)]
    );
}

#[test]
fn snapshots_cannot_run_ahead_or_regress() {
    let mut storage = Storage::open_in_memory().expect("open database");
    storage
        .append_journal_event(event(
            "event-1",
            "thread-1",
            None,
            None,
            "custom",
            json!({}),
            1,
        ))
        .expect("append event");

    let ahead = storage.save_thread_snapshot(ThreadSnapshot {
        thread_id: "thread-1".to_owned(),
        through_sequence: 2,
        schema_version: 1,
        payload: json!({"state": "future"}),
        created_at_ms: 2,
    });
    assert!(matches!(
        ahead,
        Err(StorageError::SnapshotAheadOfJournal { .. })
    ));

    storage
        .save_thread_snapshot(ThreadSnapshot {
            thread_id: "thread-1".to_owned(),
            through_sequence: 1,
            schema_version: 1,
            payload: json!({"state": "current"}),
            created_at_ms: 3,
        })
        .expect("save current snapshot");
    let stale = storage.save_thread_snapshot(ThreadSnapshot {
        thread_id: "thread-1".to_owned(),
        through_sequence: 0,
        schema_version: 1,
        payload: json!({"state": "stale"}),
        created_at_ms: 4,
    });
    assert!(matches!(
        stale,
        Err(StorageError::StaleThreadSnapshot { .. })
    ));

    let loaded = storage
        .load_thread_snapshot("thread-1")
        .expect("load snapshot")
        .expect("snapshot exists");
    assert_eq!(loaded.through_sequence, 1);
    assert_eq!(loaded.payload, json!({"state": "current"}));
}

#[test]
fn recovery_view_finds_unfinished_turns_and_latest_action_state() {
    let mut storage = Storage::open_in_memory().expect("open database");
    storage
        .append_journal_events(vec![
            event(
                "turn-1-start",
                "thread-1",
                Some("turn-1"),
                None,
                journal_event_types::TURN_STARTED,
                json!({}),
                1,
            ),
            event(
                "turn-1-end",
                "thread-1",
                Some("turn-1"),
                None,
                journal_event_types::TURN_COMPLETED,
                json!({}),
                2,
            ),
            event(
                "turn-2-start",
                "thread-1",
                Some("turn-2"),
                None,
                journal_event_types::TURN_STARTED,
                json!({}),
                3,
            ),
            event(
                "action-proposed",
                "thread-1",
                Some("turn-2"),
                Some("step-1"),
                journal_event_types::ACTION_PROPOSED,
                json!({"action_id": "action-1", "idempotency_key": "write:one"}),
                4,
            ),
            event(
                "action-started",
                "thread-1",
                Some("turn-2"),
                Some("step-1"),
                journal_event_types::ACTION_STARTED,
                json!({"action_id": "action-1"}),
                5,
            ),
        ])
        .expect("append recovery history");

    let view = storage.recovery_view("thread-1").expect("recover view");
    assert_eq!(view.through_sequence, 5);
    assert_eq!(view.unfinished_turns.len(), 1);
    assert_eq!(view.unfinished_turns[0].turn_id, "turn-2");
    assert_eq!(view.actions.len(), 1);
    assert_eq!(view.actions[0].action_id, "action-1");
    assert_eq!(view.actions[0].status, ActionRecoveryStatus::Running);
    assert_eq!(
        view.actions[0].idempotency_key.as_deref(),
        Some("write:one")
    );
}

#[test]
fn known_action_event_requires_an_action_id() {
    let mut storage = Storage::open_in_memory().expect("open database");
    storage
        .append_journal_event(event(
            "broken-action",
            "thread-1",
            Some("turn-1"),
            None,
            journal_event_types::ACTION_STARTED,
            json!({}),
            1,
        ))
        .expect("journal accepts generic JSON facts");

    let recovered = storage.recovery_view("thread-1");
    assert!(matches!(
        recovered,
        Err(StorageError::InvalidRecoveryEvent { event_id, .. })
            if event_id == "broken-action"
    ));
}

#[test]
fn journal_facts_survive_restart_and_reject_sql_mutation() {
    let directory = tempdir().expect("temporary directory");
    let database = directory.path().join("state.sqlite3");
    {
        let mut storage = Storage::open(&database).expect("open storage");
        storage
            .append_journal_event(event(
                "event-1",
                "thread-1",
                None,
                None,
                "custom",
                json!({"durable": true}),
                1,
            ))
            .expect("append event");
    }

    let storage = Storage::open(&database).expect("reopen storage");
    let facts = storage
        .read_journal_after("thread-1", 0)
        .expect("read durable events");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].payload, json!({"durable": true}));
    drop(storage);

    let direct = Connection::open(&database).expect("open raw database");
    assert!(
        direct
            .execute(
                "UPDATE journal_events SET event_type = 'rewritten' WHERE event_id = 'event-1'",
                [],
            )
            .is_err()
    );
    assert!(
        direct
            .execute("DELETE FROM journal_events WHERE event_id = 'event-1'", [])
            .is_err()
    );
}

#[test]
fn concurrent_writers_allocate_contiguous_thread_sequences() {
    const WRITERS: usize = 6;

    let directory = tempdir().expect("temporary directory");
    let database = directory.path().join("state.sqlite3");
    drop(Storage::open(&database).expect("initialize database"));

    let barrier = Arc::new(Barrier::new(WRITERS));
    let handles = (0..WRITERS)
        .map(|index| {
            let barrier = Arc::clone(&barrier);
            let database = database.clone();
            thread::spawn(move || {
                let mut storage = Storage::open(database).expect("open writer database");
                barrier.wait();
                storage
                    .append_journal_event(event(
                        &format!("event-{index}"),
                        "thread-1",
                        None,
                        None,
                        "custom",
                        json!({}),
                        index as i64,
                    ))
                    .expect("append concurrent event")
                    .sequence
            })
        })
        .collect::<Vec<_>>();

    let mut sequences = handles
        .into_iter()
        .map(|handle| handle.join().expect("join writer"))
        .collect::<Vec<_>>();
    sequences.sort_unstable();
    assert_eq!(sequences, (1..=WRITERS as i64).collect::<Vec<_>>());
}
