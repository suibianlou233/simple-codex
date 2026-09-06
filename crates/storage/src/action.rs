use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use rusqlite::TransactionBehavior;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::JournalEvent;
use crate::NewJournalEvent;
use crate::Storage;
use crate::StorageError;
use crate::journal::append_prepared_journal_event;
use crate::journal::prepare_journal_event;
use crate::journal_event_types;

/// Approval information persisted together with an action intent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalDeclaration {
    pub event_id: String,
    pub approval_id: String,
    pub approval_kind: String,
    pub payload: Value,
}

/// A dangerous operation before any external side effect is allowed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewActionIntent {
    pub event_id: String,
    pub action_id: String,
    pub idempotency_key: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub step_id: Option<String>,
    pub schema_version: i64,
    pub action_kind: String,
    pub payload: Value,
    pub approval: Option<ApprovalDeclaration>,
    pub created_at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClaimStatus {
    PendingApproval,
    Ready,
    Executing,
    Completed,
    Rejected,
    Failed,
}

impl ActionClaimStatus {
    fn as_sql(self) -> &'static str {
        match self {
            Self::PendingApproval => "pending_approval",
            Self::Ready => "ready",
            Self::Executing => "executing",
            Self::Completed => "completed",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
        }
    }

    fn from_sql(value: &str) -> Result<Self, StorageError> {
        match value {
            "pending_approval" => Ok(Self::PendingApproval),
            "ready" => Ok(Self::Ready),
            "executing" => Ok(Self::Executing),
            "completed" => Ok(Self::Completed),
            "rejected" => Ok(Self::Rejected),
            "failed" => Ok(Self::Failed),
            _ => Err(StorageError::CorruptActionClaim {
                message: format!("unknown status `{value}`"),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionClaim {
    pub action_id: String,
    pub idempotency_key: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub step_id: Option<String>,
    pub action_kind: String,
    pub schema_version: i64,
    pub status: ActionClaimStatus,
    pub intent_event_id: String,
    pub approval_event_id: Option<String>,
    pub last_event_id: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "claim")]
pub enum PrepareActionOutcome {
    Prepared(ActionClaim),
    Existing(ActionClaim),
}

/// `DoNotExecute` includes a prior `executing` claim after restart. The
/// external effect is then uncertain and must be reconciled, never repeated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "claim")]
pub enum BeginActionOutcome {
    Execute(ActionClaim),
    DoNotExecute(ActionClaim),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionExecutionResult {
    Completed,
    Failed,
}

impl Storage {
    /// Atomically records the action intent, optional approval declaration, and
    /// unique idempotency claim. Returning `Existing` never appends new facts.
    pub fn prepare_action_intent(
        &mut self,
        intent: NewActionIntent,
    ) -> Result<PrepareActionOutcome, StorageError> {
        validate_action_intent(&intent)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        if let Some(existing) = action_claim_by_key(&transaction, &intent.idempotency_key)? {
            if existing.action_id != intent.action_id
                || existing.thread_id != intent.thread_id
                || existing.action_kind != intent.action_kind
            {
                return Err(StorageError::IdempotencyConflict {
                    key: intent.idempotency_key,
                    existing_action_id: existing.action_id,
                    attempted_action_id: intent.action_id,
                });
            }
            ensure_existing_intent_matches(&transaction, &existing, &intent)?;
            transaction.commit()?;
            return Ok(PrepareActionOutcome::Existing(existing));
        }

        let proposed = prepare_journal_event(NewJournalEvent {
            event_id: intent.event_id.clone(),
            thread_id: intent.thread_id.clone(),
            turn_id: intent.turn_id.clone(),
            step_id: intent.step_id.clone(),
            schema_version: intent.schema_version,
            event_type: journal_event_types::ACTION_PROPOSED.to_owned(),
            payload: action_intent_payload(&intent)?,
            created_at_ms: intent.created_at_ms,
        })?;
        let proposed = append_prepared_journal_event(&transaction, proposed)?;

        let approval_event = intent
            .approval
            .as_ref()
            .map(|approval| approval_event(&intent, approval))
            .transpose()?
            .map(|event| append_prepared_journal_event(&transaction, event))
            .transpose()?;
        let status = if approval_event.is_some() {
            ActionClaimStatus::PendingApproval
        } else {
            ActionClaimStatus::Ready
        };
        let last_event_id = approval_event.as_ref().map_or_else(
            || proposed.event_id.as_str(),
            |event| event.event_id.as_str(),
        );
        transaction.execute(
            "INSERT INTO action_claims(
                idempotency_key, action_id, thread_id, turn_id, step_id, action_kind,
                schema_version, status, intent_event_id, approval_event_id, last_event_id,
                created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            (
                &intent.idempotency_key,
                &intent.action_id,
                &intent.thread_id,
                &intent.turn_id,
                &intent.step_id,
                &intent.action_kind,
                intent.schema_version,
                status.as_sql(),
                &proposed.event_id,
                approval_event.as_ref().map(|event| &event.event_id),
                last_event_id,
                intent.created_at_ms,
            ),
        )?;
        let claim = action_claim_by_key(&transaction, &intent.idempotency_key)?.ok_or(
            StorageError::CorruptActionClaim {
                message: "inserted claim could not be read".to_owned(),
            },
        )?;
        transaction.commit()?;
        Ok(PrepareActionOutcome::Prepared(claim))
    }

    /// Records an approval decision and advances the claim in one transaction.
    pub fn resolve_action_approval(
        &mut self,
        idempotency_key: &str,
        event_id: &str,
        approved: bool,
        payload: Value,
        created_at_ms: i64,
    ) -> Result<ActionClaim, StorageError> {
        validate_non_empty("idempotency_key", idempotency_key)?;
        validate_non_empty("event_id", event_id)?;
        validate_timestamp(created_at_ms)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let claim = required_claim(&transaction, idempotency_key)?;
        let expected_type = if approved {
            journal_event_types::ACTION_APPROVED
        } else {
            journal_event_types::ACTION_REJECTED
        };
        if claim.status != ActionClaimStatus::PendingApproval
            && lifecycle_event_already_recorded(
                &transaction,
                &claim,
                event_id,
                expected_type,
                &payload,
            )?
        {
            transaction.commit()?;
            return Ok(claim);
        }
        require_status(&claim, ActionClaimStatus::PendingApproval)?;
        let event = lifecycle_event(&claim, event_id, expected_type, payload, created_at_ms)?;
        let event = append_prepared_journal_event(&transaction, event)?;
        let status = if approved {
            ActionClaimStatus::Ready
        } else {
            ActionClaimStatus::Rejected
        };
        update_claim(&transaction, idempotency_key, status, &event, created_at_ms)?;
        let claim = required_claim(&transaction, idempotency_key)?;
        transaction.commit()?;
        Ok(claim)
    }

    /// Acquires the one and only execution lease for an idempotency key.
    pub fn begin_action_execution(
        &mut self,
        idempotency_key: &str,
        event_id: &str,
        payload: Value,
        created_at_ms: i64,
    ) -> Result<BeginActionOutcome, StorageError> {
        validate_non_empty("idempotency_key", idempotency_key)?;
        validate_non_empty("event_id", event_id)?;
        validate_timestamp(created_at_ms)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let claim = required_claim(&transaction, idempotency_key)?;
        if claim.status != ActionClaimStatus::Ready {
            transaction.commit()?;
            return Ok(BeginActionOutcome::DoNotExecute(claim));
        }
        let event = lifecycle_event(
            &claim,
            event_id,
            journal_event_types::ACTION_STARTED,
            payload,
            created_at_ms,
        )?;
        let event = append_prepared_journal_event(&transaction, event)?;
        update_claim(
            &transaction,
            idempotency_key,
            ActionClaimStatus::Executing,
            &event,
            created_at_ms,
        )?;
        let claim = required_claim(&transaction, idempotency_key)?;
        transaction.commit()?;
        Ok(BeginActionOutcome::Execute(claim))
    }

    /// Persists the observed outcome after the external operation returns.
    pub fn finish_action_execution(
        &mut self,
        idempotency_key: &str,
        event_id: &str,
        result: ActionExecutionResult,
        payload: Value,
        created_at_ms: i64,
    ) -> Result<ActionClaim, StorageError> {
        validate_non_empty("idempotency_key", idempotency_key)?;
        validate_non_empty("event_id", event_id)?;
        validate_timestamp(created_at_ms)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let claim = required_claim(&transaction, idempotency_key)?;
        let (event_type, status) = match result {
            ActionExecutionResult::Completed => (
                journal_event_types::ACTION_COMPLETED,
                ActionClaimStatus::Completed,
            ),
            ActionExecutionResult::Failed => (
                journal_event_types::ACTION_FAILED,
                ActionClaimStatus::Failed,
            ),
        };
        if claim.status != ActionClaimStatus::Executing
            && lifecycle_event_already_recorded(
                &transaction,
                &claim,
                event_id,
                event_type,
                &payload,
            )?
        {
            transaction.commit()?;
            return Ok(claim);
        }
        require_status(&claim, ActionClaimStatus::Executing)?;
        let event = lifecycle_event(&claim, event_id, event_type, payload, created_at_ms)?;
        let event = append_prepared_journal_event(&transaction, event)?;
        update_claim(&transaction, idempotency_key, status, &event, created_at_ms)?;
        let claim = required_claim(&transaction, idempotency_key)?;
        transaction.commit()?;
        Ok(claim)
    }

    pub fn get_action_claim(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ActionClaim>, StorageError> {
        validate_non_empty("idempotency_key", idempotency_key)?;
        action_claim_by_key(&self.connection, idempotency_key)
    }

    pub fn list_action_claims(&self, thread_id: &str) -> Result<Vec<ActionClaim>, StorageError> {
        validate_non_empty("thread_id", thread_id)?;
        let mut statement = self.connection.prepare(
            "SELECT action_id, idempotency_key, thread_id, turn_id, step_id,
                    action_kind, schema_version, status, intent_event_id, approval_event_id,
                    last_event_id, created_at_ms, updated_at_ms
             FROM action_claims WHERE thread_id = ?1
             ORDER BY created_at_ms ASC, action_id ASC",
        )?;
        let rows = statement.query_map([thread_id], action_claim_from_row)?;
        rows.map(|row| row?.try_into()).collect()
    }
}

fn approval_event(
    intent: &NewActionIntent,
    approval: &ApprovalDeclaration,
) -> Result<crate::journal::PreparedJournalEvent, StorageError> {
    validate_non_empty("approval.event_id", &approval.event_id)?;
    validate_non_empty("approval.approval_id", &approval.approval_id)?;
    validate_non_empty("approval.approval_kind", &approval.approval_kind)?;
    let payload = approval_declaration_payload(intent, approval)?;
    prepare_journal_event(NewJournalEvent {
        event_id: approval.event_id.clone(),
        thread_id: intent.thread_id.clone(),
        turn_id: intent.turn_id.clone(),
        step_id: intent.step_id.clone(),
        schema_version: intent.schema_version,
        event_type: journal_event_types::APPROVAL_DECLARED.to_owned(),
        payload,
        created_at_ms: intent.created_at_ms,
    })
}

fn approval_declaration_payload(
    intent: &NewActionIntent,
    approval: &ApprovalDeclaration,
) -> Result<Value, StorageError> {
    let mut payload = object_payload(approval.payload.clone(), "approval payload")?;
    payload.insert(
        "action_id".to_owned(),
        Value::String(intent.action_id.clone()),
    );
    payload.insert(
        "idempotency_key".to_owned(),
        Value::String(intent.idempotency_key.clone()),
    );
    payload.insert(
        "approval_id".to_owned(),
        Value::String(approval.approval_id.clone()),
    );
    payload.insert(
        "approval_kind".to_owned(),
        Value::String(approval.approval_kind.clone()),
    );
    Ok(Value::Object(payload))
}

fn lifecycle_event(
    claim: &ActionClaim,
    event_id: &str,
    event_type: &str,
    payload: Value,
    created_at_ms: i64,
) -> Result<crate::journal::PreparedJournalEvent, StorageError> {
    let payload = lifecycle_payload(claim, payload)?;
    prepare_journal_event(NewJournalEvent {
        event_id: event_id.to_owned(),
        thread_id: claim.thread_id.clone(),
        turn_id: claim.turn_id.clone(),
        step_id: claim.step_id.clone(),
        schema_version: claim.schema_version,
        event_type: event_type.to_owned(),
        payload,
        created_at_ms,
    })
}

fn lifecycle_payload(claim: &ActionClaim, payload: Value) -> Result<Value, StorageError> {
    let mut payload = object_payload(payload, "lifecycle payload")?;
    payload.insert(
        "action_id".to_owned(),
        Value::String(claim.action_id.clone()),
    );
    payload.insert(
        "idempotency_key".to_owned(),
        Value::String(claim.idempotency_key.clone()),
    );
    Ok(Value::Object(payload))
}

fn validate_action_intent(intent: &NewActionIntent) -> Result<(), StorageError> {
    validate_non_empty("event_id", &intent.event_id)?;
    validate_non_empty("action_id", &intent.action_id)?;
    validate_non_empty("idempotency_key", &intent.idempotency_key)?;
    validate_non_empty("thread_id", &intent.thread_id)?;
    validate_optional_id("turn_id", intent.turn_id.as_deref())?;
    validate_optional_id("step_id", intent.step_id.as_deref())?;
    validate_non_empty("action_kind", &intent.action_kind)?;
    if intent.schema_version <= 0 {
        return Err(StorageError::InvalidField {
            field: "schema_version",
            message: "must be positive",
        });
    }
    validate_timestamp(intent.created_at_ms)?;
    let _ = object_payload(intent.payload.clone(), "intent payload")?;
    if let Some(approval) = &intent.approval {
        validate_non_empty("approval.event_id", &approval.event_id)?;
        validate_non_empty("approval.approval_id", &approval.approval_id)?;
        validate_non_empty("approval.approval_kind", &approval.approval_kind)?;
        let _ = object_payload(approval.payload.clone(), "approval payload")?;
    }
    Ok(())
}

fn action_intent_payload(intent: &NewActionIntent) -> Result<Value, StorageError> {
    let mut payload = object_payload(intent.payload.clone(), "intent payload")?;
    payload.insert(
        "action_id".to_owned(),
        Value::String(intent.action_id.clone()),
    );
    payload.insert(
        "idempotency_key".to_owned(),
        Value::String(intent.idempotency_key.clone()),
    );
    payload.insert(
        "action_kind".to_owned(),
        Value::String(intent.action_kind.clone()),
    );
    Ok(Value::Object(payload))
}

fn ensure_existing_intent_matches(
    connection: &Connection,
    existing: &ActionClaim,
    intent: &NewActionIntent,
) -> Result<(), StorageError> {
    let definition_matches = existing.turn_id == intent.turn_id
        && existing.step_id == intent.step_id
        && existing.schema_version == intent.schema_version;
    if !definition_matches {
        return Err(definition_conflict(
            intent,
            "turn、step 或 schema version 不一致",
        ));
    }

    let persisted_payload = journal_payload_by_event_id(connection, &existing.intent_event_id)?;
    if persisted_payload.as_ref() != Some(&action_intent_payload(intent)?) {
        return Err(definition_conflict(intent, "intent payload 不一致"));
    }

    match (&existing.approval_event_id, &intent.approval) {
        (None, None) => Ok(()),
        (Some(event_id), Some(approval)) => {
            let persisted_payload = journal_payload_by_event_id(connection, event_id)?;
            if persisted_payload.as_ref() == Some(&approval_declaration_payload(intent, approval)?)
            {
                Ok(())
            } else {
                Err(definition_conflict(intent, "approval declaration 不一致"))
            }
        }
        _ => Err(definition_conflict(
            intent,
            "是否需要 approval 的定义不一致",
        )),
    }
}

fn definition_conflict(intent: &NewActionIntent, message: &'static str) -> StorageError {
    StorageError::IdempotencyDefinitionConflict {
        key: intent.idempotency_key.clone(),
        action_id: intent.action_id.clone(),
        message,
    }
}

fn journal_payload_by_event_id(
    connection: &Connection,
    event_id: &str,
) -> Result<Option<Value>, StorageError> {
    let payload_json = connection
        .query_row(
            "SELECT payload_json FROM journal_events WHERE event_id = ?1",
            [event_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    payload_json
        .map(|payload| serde_json::from_str(&payload))
        .transpose()
        .map_err(Into::into)
}

fn lifecycle_event_already_recorded(
    connection: &Connection,
    claim: &ActionClaim,
    event_id: &str,
    event_type: &str,
    payload: &Value,
) -> Result<bool, StorageError> {
    let stored = connection
        .query_row(
            "SELECT payload_json
             FROM journal_events
             WHERE event_id = ?1 AND thread_id = ?2 AND event_type = ?3",
            (event_id, &claim.thread_id, event_type),
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(false);
    };
    let stored: Value = serde_json::from_str(&stored)?;
    Ok(stored == lifecycle_payload(claim, payload.clone())?)
}

fn object_payload(payload: Value, label: &'static str) -> Result<Map<String, Value>, StorageError> {
    payload
        .as_object()
        .cloned()
        .ok_or(StorageError::InvalidField {
            field: "payload",
            message: label,
        })
}

fn required_claim(
    connection: &Connection,
    idempotency_key: &str,
) -> Result<ActionClaim, StorageError> {
    action_claim_by_key(connection, idempotency_key)?
        .ok_or_else(|| StorageError::ActionClaimNotFound(idempotency_key.to_owned()))
}

fn require_status(claim: &ActionClaim, expected: ActionClaimStatus) -> Result<(), StorageError> {
    if claim.status == expected {
        Ok(())
    } else {
        Err(StorageError::InvalidActionTransition {
            key: claim.idempotency_key.clone(),
            found: claim.status.as_sql(),
            expected: expected.as_sql(),
        })
    }
}

fn update_claim(
    transaction: &Transaction<'_>,
    idempotency_key: &str,
    status: ActionClaimStatus,
    event: &JournalEvent,
    updated_at_ms: i64,
) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE action_claims
         SET status = ?2, last_event_id = ?3, updated_at_ms = ?4
         WHERE idempotency_key = ?1",
        (
            idempotency_key,
            status.as_sql(),
            &event.event_id,
            updated_at_ms,
        ),
    )?;
    Ok(())
}

type RawActionClaim = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    String,
    String,
    Option<String>,
    String,
    i64,
    i64,
);

impl TryFrom<RawActionClaim> for ActionClaim {
    type Error = StorageError;

    fn try_from(raw: RawActionClaim) -> Result<Self, Self::Error> {
        Ok(Self {
            action_id: raw.0,
            idempotency_key: raw.1,
            thread_id: raw.2,
            turn_id: raw.3,
            step_id: raw.4,
            action_kind: raw.5,
            schema_version: raw.6,
            status: ActionClaimStatus::from_sql(&raw.7)?,
            intent_event_id: raw.8,
            approval_event_id: raw.9,
            last_event_id: raw.10,
            created_at_ms: raw.11,
            updated_at_ms: raw.12,
        })
    }
}

fn action_claim_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawActionClaim> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
    ))
}

fn action_claim_by_key(
    connection: &Connection,
    idempotency_key: &str,
) -> Result<Option<ActionClaim>, StorageError> {
    let raw = connection
        .query_row(
            "SELECT action_id, idempotency_key, thread_id, turn_id, step_id,
                    action_kind, schema_version, status, intent_event_id, approval_event_id,
                    last_event_id, created_at_ms, updated_at_ms
             FROM action_claims WHERE idempotency_key = ?1",
            [idempotency_key],
            action_claim_from_row,
        )
        .optional()?;
    raw.map(TryInto::try_into).transpose()
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        Err(StorageError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn validate_optional_id(field: &'static str, value: Option<&str>) -> Result<(), StorageError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
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
