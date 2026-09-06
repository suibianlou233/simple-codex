use std::collections::HashMap;

use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use rusqlite::TransactionBehavior;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::Storage;
use crate::StorageError;

/// Stable lifecycle event names understood by [`RecoveryView`].
///
/// Other event types remain valid journal facts; they simply do not affect the
/// generic recovery projection.
pub mod journal_event_types {
    pub const TURN_STARTED: &str = "turn.started";
    pub const TURN_COMPLETED: &str = "turn.completed";
    pub const TURN_FAILED: &str = "turn.failed";
    pub const TURN_CANCELLED: &str = "turn.cancelled";

    pub const ACTION_PROPOSED: &str = "action.proposed";
    pub const ACTION_APPROVED: &str = "action.approved";
    pub const ACTION_STARTED: &str = "action.started";
    pub const ACTION_COMPLETED: &str = "action.completed";
    pub const ACTION_REJECTED: &str = "action.rejected";
    pub const ACTION_FAILED: &str = "action.failed";
    pub const ACTION_UNDONE: &str = "action.undone";
    pub const APPROVAL_DECLARED: &str = "approval.declared";
}

/// An immutable event before the journal assigns its thread-local sequence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewJournalEvent {
    pub event_id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub step_id: Option<String>,
    pub schema_version: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_at_ms: i64,
}

/// A durable, versioned fact in one thread's append-only journal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JournalEvent {
    pub event_id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub step_id: Option<String>,
    pub sequence: i64,
    pub schema_version: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_at_ms: i64,
}

/// A replaceable projection cache through a known journal sequence.
///
/// Snapshots are not facts and are never used as a substitute for the event
/// stream. A caller may delete and rebuild them from journal events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadSnapshot {
    pub thread_id: String,
    pub through_sequence: i64,
    pub schema_version: i64,
    pub payload: Value,
    pub created_at_ms: i64,
}

/// The newest usable snapshot plus the immutable tail that must be replayed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadReplay {
    pub thread_id: String,
    pub snapshot: Option<ThreadSnapshot>,
    pub events: Vec<JournalEvent>,
    pub through_sequence: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRecoveryStatus {
    Pending,
    Ready,
    Approved,
    Running,
    Completed,
    Rejected,
    Failed,
    Undone,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveredTurn {
    pub turn_id: String,
    pub started_at_ms: i64,
    pub started_sequence: i64,
    pub last_sequence: i64,
    pub last_event_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveredAction {
    pub action_id: String,
    pub turn_id: Option<String>,
    pub step_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub status: ActionRecoveryStatus,
    pub last_sequence: i64,
}

/// A deterministic projection used to decide what remains after restart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryView {
    pub thread_id: String,
    pub through_sequence: i64,
    pub unfinished_turns: Vec<RecoveredTurn>,
    pub actions: Vec<RecoveredAction>,
}

impl RecoveryView {
    pub fn from_events(thread_id: &str, events: &[JournalEvent]) -> Result<Self, StorageError> {
        validate_non_empty("thread_id", thread_id)?;
        let mut turns = HashMap::<String, TurnAccumulator>::new();
        let mut actions = HashMap::<String, RecoveredAction>::new();
        let mut through_sequence = 0_i64;

        for event in events {
            if event.thread_id != thread_id {
                return Err(StorageError::JournalThreadMismatch {
                    event_id: event.event_id.clone(),
                    expected: thread_id.to_owned(),
                    found: event.thread_id.clone(),
                });
            }
            through_sequence = through_sequence.max(event.sequence);

            if let Some(turn_id) = event.turn_id.as_deref() {
                update_turn_projection(&mut turns, turn_id, event);
            }

            if let Some(status) = action_status_for_event_type(&event.event_type) {
                let action_id = event
                    .payload
                    .get("action_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| StorageError::InvalidRecoveryEvent {
                        event_id: event.event_id.clone(),
                        message: "action lifecycle payload requires a non-empty action_id",
                    })?;
                let idempotency_key = event
                    .payload
                    .get("idempotency_key")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        actions
                            .get(action_id)
                            .and_then(|action| action.idempotency_key.clone())
                    });
                actions.insert(
                    action_id.to_owned(),
                    RecoveredAction {
                        action_id: action_id.to_owned(),
                        turn_id: event.turn_id.clone(),
                        step_id: event.step_id.clone(),
                        idempotency_key,
                        status,
                        last_sequence: event.sequence,
                    },
                );
            }
        }

        let mut unfinished_turns = turns
            .into_values()
            .filter(|turn| !turn.terminal)
            .map(|turn| turn.recovered)
            .collect::<Vec<_>>();
        unfinished_turns.sort_by_key(|turn| turn.started_sequence);

        let mut actions = actions.into_values().collect::<Vec<_>>();
        actions.sort_by_key(|action| action.last_sequence);

        Ok(Self {
            thread_id: thread_id.to_owned(),
            through_sequence,
            unfinished_turns,
            actions,
        })
    }
}

struct TurnAccumulator {
    recovered: RecoveredTurn,
    terminal: bool,
}

pub(crate) struct PreparedJournalEvent {
    event: NewJournalEvent,
    payload_json: String,
}

impl Storage {
    /// Appends one immutable fact and atomically allocates its thread sequence.
    pub fn append_journal_event(
        &mut self,
        event: NewJournalEvent,
    ) -> Result<JournalEvent, StorageError> {
        let prepared = prepare_journal_event(event)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = append_prepared_journal_event(&transaction, prepared)?;
        transaction.commit()?;
        Ok(record)
    }

    /// Appends all provided facts atomically in input order.
    ///
    /// A batch may contain multiple threads. Each thread receives its own
    /// contiguous sequence numbers, and any failure rolls back the full batch.
    pub fn append_journal_events(
        &mut self,
        events: Vec<NewJournalEvent>,
    ) -> Result<Vec<JournalEvent>, StorageError> {
        let prepared = events
            .into_iter()
            .map(prepare_journal_event)
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let records = prepared
            .into_iter()
            .map(|event| append_prepared_journal_event(&transaction, event))
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;
        Ok(records)
    }

    /// Reads journal facts strictly after `sequence`, oldest first.
    pub fn read_journal_after(
        &self,
        thread_id: &str,
        sequence: i64,
    ) -> Result<Vec<JournalEvent>, StorageError> {
        validate_non_empty("thread_id", thread_id)?;
        if sequence < 0 {
            return Err(StorageError::InvalidField {
                field: "sequence",
                message: "must not be negative",
            });
        }

        let mut statement = self.connection.prepare(
            "SELECT event_id, thread_id, turn_id, step_id, sequence, schema_version,
                    event_type, payload_json, created_at_ms
             FROM journal_events
             WHERE thread_id = ?1 AND sequence > ?2
             ORDER BY sequence ASC",
        )?;
        let rows = statement.query_map((thread_id, sequence), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?;

        rows.map(|row| {
            let (
                event_id,
                thread_id,
                turn_id,
                step_id,
                sequence,
                schema_version,
                event_type,
                payload_json,
                created_at_ms,
            ) = row?;
            Ok(JournalEvent {
                event_id,
                thread_id,
                turn_id,
                step_id,
                sequence,
                schema_version,
                event_type,
                payload: serde_json::from_str(&payload_json)?,
                created_at_ms,
            })
        })
        .collect()
    }

    /// Stores a rebuildable projection cache without permitting it to move
    /// behind a snapshot already accepted for the same thread.
    pub fn save_thread_snapshot(
        &mut self,
        snapshot: ThreadSnapshot,
    ) -> Result<ThreadSnapshot, StorageError> {
        validate_snapshot(&snapshot)?;
        let payload_json = serde_json::to_string(&snapshot.payload)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_journal_stream(&transaction, &snapshot.thread_id)?;
        let last_sequence = journal_last_sequence(&transaction, &snapshot.thread_id)?;
        if snapshot.through_sequence > last_sequence {
            return Err(StorageError::SnapshotAheadOfJournal {
                thread_id: snapshot.thread_id,
                through_sequence: snapshot.through_sequence,
                last_sequence,
            });
        }

        let current_sequence = transaction
            .query_row(
                "SELECT through_sequence FROM thread_snapshots WHERE thread_id = ?1",
                [&snapshot.thread_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if current_sequence.is_some_and(|current| snapshot.through_sequence < current) {
            return Err(StorageError::StaleThreadSnapshot {
                thread_id: snapshot.thread_id,
                current_sequence: current_sequence.unwrap_or_default(),
                attempted_sequence: snapshot.through_sequence,
            });
        }

        transaction.execute(
            "INSERT INTO thread_snapshots(
                thread_id, through_sequence, schema_version, payload_json, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(thread_id) DO UPDATE SET
                through_sequence = excluded.through_sequence,
                schema_version = excluded.schema_version,
                payload_json = excluded.payload_json,
                created_at_ms = excluded.created_at_ms",
            (
                &snapshot.thread_id,
                snapshot.through_sequence,
                snapshot.schema_version,
                &payload_json,
                snapshot.created_at_ms,
            ),
        )?;
        transaction.commit()?;
        Ok(snapshot)
    }

    pub fn load_thread_snapshot(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadSnapshot>, StorageError> {
        validate_non_empty("thread_id", thread_id)?;
        let raw = self
            .connection
            .query_row(
                "SELECT thread_id, through_sequence, schema_version, payload_json, created_at_ms
                 FROM thread_snapshots WHERE thread_id = ?1",
                [thread_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?;

        raw.map(
            |(thread_id, through_sequence, schema_version, payload_json, created_at_ms)| {
                Ok(ThreadSnapshot {
                    thread_id,
                    through_sequence,
                    schema_version,
                    payload: serde_json::from_str(&payload_json)?,
                    created_at_ms,
                })
            },
        )
        .transpose()
    }

    /// Loads the minimal deterministic input needed to rebuild a thread.
    ///
    /// The snapshot is only a cache. Events after its sequence are always
    /// returned in order, so a caller can replay the authoritative tail.
    pub fn load_thread_replay(&self, thread_id: &str) -> Result<ThreadReplay, StorageError> {
        validate_non_empty("thread_id", thread_id)?;
        let snapshot = self.load_thread_snapshot(thread_id)?;
        let snapshot_sequence = snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.through_sequence);
        let events = self.read_journal_after(thread_id, snapshot_sequence)?;
        let through_sequence = events
            .last()
            .map_or(snapshot_sequence, |event| event.sequence);
        Ok(ThreadReplay {
            thread_id: thread_id.to_owned(),
            snapshot,
            events,
            through_sequence,
        })
    }

    /// Replays durable facts into a minimal restart-oriented projection.
    pub fn recovery_view(&self, thread_id: &str) -> Result<RecoveryView, StorageError> {
        let events = self.read_journal_after(thread_id, 0)?;
        RecoveryView::from_events(thread_id, &events)
    }
}

fn update_turn_projection(
    turns: &mut HashMap<String, TurnAccumulator>,
    turn_id: &str,
    event: &JournalEvent,
) {
    use journal_event_types::{TURN_CANCELLED, TURN_COMPLETED, TURN_FAILED, TURN_STARTED};

    if event.event_type == TURN_STARTED {
        turns
            .entry(turn_id.to_owned())
            .or_insert_with(|| TurnAccumulator {
                recovered: RecoveredTurn {
                    turn_id: turn_id.to_owned(),
                    started_at_ms: event.created_at_ms,
                    started_sequence: event.sequence,
                    last_sequence: event.sequence,
                    last_event_type: event.event_type.clone(),
                },
                terminal: false,
            });
    }

    if let Some(turn) = turns.get_mut(turn_id) {
        turn.recovered.last_sequence = event.sequence;
        turn.recovered.last_event_type.clone_from(&event.event_type);
        if matches!(
            event.event_type.as_str(),
            TURN_COMPLETED | TURN_FAILED | TURN_CANCELLED
        ) {
            turn.terminal = true;
        }
    }
}

fn action_status_for_event_type(event_type: &str) -> Option<ActionRecoveryStatus> {
    use journal_event_types::{
        ACTION_APPROVED, ACTION_COMPLETED, ACTION_FAILED, ACTION_PROPOSED, ACTION_REJECTED,
        ACTION_STARTED, ACTION_UNDONE, APPROVAL_DECLARED,
    };

    match event_type {
        // A proposal is immediately runnable unless the same atomic intent
        // transaction also appends an approval declaration after it.
        ACTION_PROPOSED => Some(ActionRecoveryStatus::Ready),
        APPROVAL_DECLARED => Some(ActionRecoveryStatus::Pending),
        ACTION_APPROVED => Some(ActionRecoveryStatus::Approved),
        ACTION_STARTED => Some(ActionRecoveryStatus::Running),
        ACTION_COMPLETED => Some(ActionRecoveryStatus::Completed),
        ACTION_REJECTED => Some(ActionRecoveryStatus::Rejected),
        ACTION_FAILED => Some(ActionRecoveryStatus::Failed),
        ACTION_UNDONE => Some(ActionRecoveryStatus::Undone),
        _ => None,
    }
}

pub(crate) fn prepare_journal_event(
    event: NewJournalEvent,
) -> Result<PreparedJournalEvent, StorageError> {
    validate_non_empty("event_id", &event.event_id)?;
    validate_non_empty("thread_id", &event.thread_id)?;
    validate_optional_id("turn_id", event.turn_id.as_deref())?;
    validate_optional_id("step_id", event.step_id.as_deref())?;
    validate_non_empty("event_type", &event.event_type)?;
    if event.schema_version <= 0 {
        return Err(StorageError::InvalidField {
            field: "schema_version",
            message: "must be positive",
        });
    }
    validate_timestamp(event.created_at_ms)?;
    let payload_json = serde_json::to_string(&event.payload)?;
    Ok(PreparedJournalEvent {
        event,
        payload_json,
    })
}

pub(crate) fn append_prepared_journal_event(
    transaction: &Transaction<'_>,
    prepared: PreparedJournalEvent,
) -> Result<JournalEvent, StorageError> {
    let event = prepared.event;
    if journal_event_exists(transaction, &event.event_id)? {
        return Err(StorageError::DuplicateJournalEvent(event.event_id));
    }
    ensure_journal_stream(transaction, &event.thread_id)?;
    let sequence = transaction.query_row(
        "UPDATE journal_streams
         SET last_sequence = last_sequence + 1
         WHERE thread_id = ?1
         RETURNING last_sequence",
        [&event.thread_id],
        |row| row.get::<_, i64>(0),
    )?;
    transaction.execute(
        "INSERT INTO journal_events(
            event_id, thread_id, turn_id, step_id, sequence, schema_version,
            event_type, payload_json, created_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        (
            &event.event_id,
            &event.thread_id,
            &event.turn_id,
            &event.step_id,
            sequence,
            event.schema_version,
            &event.event_type,
            &prepared.payload_json,
            event.created_at_ms,
        ),
    )?;

    Ok(JournalEvent {
        event_id: event.event_id,
        thread_id: event.thread_id,
        turn_id: event.turn_id,
        step_id: event.step_id,
        sequence,
        schema_version: event.schema_version,
        event_type: event.event_type,
        payload: event.payload,
        created_at_ms: event.created_at_ms,
    })
}

fn ensure_journal_stream(
    transaction: &Transaction<'_>,
    thread_id: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO journal_streams(thread_id, last_sequence) VALUES (?1, 0)
         ON CONFLICT(thread_id) DO NOTHING",
        [thread_id],
    )?;
    Ok(())
}

fn journal_last_sequence(
    transaction: &Transaction<'_>,
    thread_id: &str,
) -> Result<i64, StorageError> {
    transaction
        .query_row(
            "SELECT last_sequence FROM journal_streams WHERE thread_id = ?1",
            [thread_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn journal_event_exists(
    transaction: &Transaction<'_>,
    event_id: &str,
) -> Result<bool, StorageError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM journal_events WHERE event_id = ?1)",
            [event_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn validate_snapshot(snapshot: &ThreadSnapshot) -> Result<(), StorageError> {
    validate_non_empty("thread_id", &snapshot.thread_id)?;
    if snapshot.through_sequence < 0 {
        return Err(StorageError::InvalidField {
            field: "through_sequence",
            message: "must not be negative",
        });
    }
    if snapshot.schema_version <= 0 {
        return Err(StorageError::InvalidField {
            field: "schema_version",
            message: "must be positive",
        });
    }
    validate_timestamp(snapshot.created_at_ms)
}

fn validate_optional_id(field: &'static str, value: Option<&str>) -> Result<(), StorageError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        Err(StorageError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        Err(StorageError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn validate_timestamp(timestamp_ms: i64) -> Result<(), StorageError> {
    if timestamp_ms < 0 {
        Err(StorageError::ClockBeforeUnixEpoch)
    } else {
        Ok(())
    }
}
