//! Durable, non-semantic child results. Native root status is not rewritten.
use super::*;
use local_agent_model::CodexChildOutcome;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChildReport {
    #[serde(default)]
    pub(super) assignments: Vec<Assignment>,
    pub(super) outcomes: Vec<CodexChildOutcome>,
    pub(super) rejected_operations: usize,
}

/// A dispatch receipt, deliberately not a claim about child completion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Assignment {
    pub(super) sender_thread_id: String,
    pub(super) sender_turn_id: String,
    pub(super) item_id: String,
    pub(super) receivers: Vec<String>,
    pub(super) instruction: String,
    pub(super) follow_up: bool,
    pub(super) status: DispatchStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DispatchStatus {
    Pending,
    Dispatched,
    Failed,
    Unknown,
}

impl DesktopRuntime {
    pub(super) fn record_assignment(
        &mut self,
        thread: &str,
        turn: &str,
        item: &CodexThreadItem,
        completed: bool,
    ) -> Result<Option<CodexDesktopEffect>, DesktopError> {
        if item.kind != "collabAgentToolCall"
            || !matches!(
                item.value["tool"].as_str(),
                Some("spawnAgent" | "sendInput")
            )
            || item.id.is_empty()
        {
            return Ok(None);
        }
        let Some(root) = self.codex_action_binding(thread, turn) else {
            return Ok(None);
        };
        // Never accept a payload claiming a different sender as ownership evidence.
        if item
            .value
            .get("senderThreadId")
            .is_some_and(|sender| sender.as_str() != Some(thread))
        {
            return Err(DesktopError::InvalidCodexResponse("子任务派发来源不一致"));
        }
        let receivers = item.value["receiverThreadIds"]
            .as_array()
            .map(|ids| {
                ids.iter()
                    .map(|id| {
                        id.as_str()
                            .filter(|id| !id.is_empty())
                            .map(str::to_owned)
                            .ok_or(DesktopError::InvalidCodexResponse("子任务派发目标无效"))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let status = if !completed {
            DispatchStatus::Pending
        } else {
            match item.value["status"].as_str() {
                Some("completed") if !receivers.is_empty() => DispatchStatus::Dispatched,
                Some("failed") => DispatchStatus::Failed,
                _ => DispatchStatus::Unknown,
            }
        };
        // Redact before bounding, so truncation cannot reveal a credential prefix.
        let raw = item.value["prompt"].as_str().unwrap_or_default();
        let safe = redact_sensitive_output_with_secrets(raw, &self.known_secret_values());
        let mut instruction: String = safe.chars().take(4096).collect();
        if safe.chars().count() > 4096 {
            instruction.push_str("\n（分工说明已截短）");
        }
        let assignment = Assignment {
            sender_thread_id: thread.into(),
            sender_turn_id: turn.into(),
            item_id: item.id.clone(),
            receivers,
            instruction,
            follow_up: item.value["tool"] == "sendInput",
            status,
        };
        let mut report = self
            .child_result_reports
            .get(&root.turn_id)
            .cloned()
            .unwrap_or_default();
        if let Some(old) = report.assignments.iter_mut().find(|old| {
            old.sender_thread_id == thread && old.sender_turn_id == turn && old.item_id == item.id
        }) {
            // End receipts are immutable. Duplicate replay/late begins cannot regress them.
            if old.status != DispatchStatus::Pending || *old == assignment {
                return Ok(None);
            }
            *old = assignment;
        } else {
            report.assignments.push(assignment);
        }
        self.persist_child_report(&root.task_id, &root.turn_id, report)?;
        Ok(Some(CodexDesktopEffect::Stream(
            crate::codex_projection::ProjectedTurnStream {
                phase: None,
                task_id: root.task_id,
                turn_id: root.turn_id,
                item_id: None,
                kind: "delegations_updated",
                content: None,
                message: None,
            },
        )))
    }

    pub(super) fn record_spawned_children(
        &mut self,
        thread: &str,
        turn: &str,
        item: &CodexThreadItem,
    ) -> Result<(), DesktopError> {
        if item.kind != "collabAgentToolCall" || item.value["tool"] != "spawnAgent" {
            return Ok(());
        }
        let Some(root) = self
            .codex_turn_links
            .get(turn)
            .filter(|root| root.codex_thread_id == thread)
            .cloned()
        else {
            return Ok(());
        };
        let Some(children) = item.value["receiverThreadIds"].as_array() else {
            return Ok(());
        };
        if children.is_empty() {
            return Ok(());
        }
        let ids = children
            .iter()
            .map(|id| {
                id.as_str()
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .ok_or(DesktopError::InvalidCodexResponse("派生子任务标识无效"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let events = self.storage.load_events(&root.task_id)?;
        if events.iter().any(|event| {
            event.turn_id.as_deref() == Some(&root.turn_id)
                && event.event_type == "codex_spawn_observed"
                && event.payload["item_id"] == item.id
        }) {
            return Ok(());
        }
        self.storage.append_event(NewEvent {
            event_id: Uuid::new_v4().to_string(),
            task_id: root.task_id,
            turn_id: Some(root.turn_id),
            event_type: "codex_spawn_observed".into(),
            payload: json!({"item_id":item.id,"children":ids}),
            created_at_ms: unix_time_ms()?,
        })?;
        Ok(())
    }

    pub(super) fn spawned_children(
        &self,
        task: &str,
        turn: &str,
    ) -> Result<HashSet<String>, DesktopError> {
        let mut ids = HashSet::new();
        for event in self.storage.load_events(task)? {
            if event.turn_id.as_deref() == Some(turn) && event.event_type == "codex_spawn_observed"
            {
                let children: Vec<String> =
                    serde_json::from_value(event.payload["children"].clone())?;
                ids.extend(children);
            }
        }
        Ok(ids)
    }

    pub(super) fn record_child_report(
        &mut self,
        task: &str,
        turn: &str,
        outcomes: Vec<CodexChildOutcome>,
    ) -> Result<(), DesktopError> {
        let rejected_operations = self.actions.values().filter(|action| action.turn_id == turn
            && action.status == ActionStatus::Rejected && matches!(&action.payload,ActionPayload::CodexApproval{request} if request.is_subagent)).count();
        let report = ChildReport {
            assignments: self
                .child_result_reports
                .get(turn)
                .map(|report| report.assignments.clone())
                .unwrap_or_default(),
            outcomes,
            rejected_operations,
        };
        self.persist_child_report(task, turn, report)
    }

    fn persist_child_report(
        &mut self,
        task: &str,
        turn: &str,
        report: ChildReport,
    ) -> Result<(), DesktopError> {
        if self.child_result_reports.get(turn) == Some(&report) {
            return Ok(());
        }
        let payload = serde_json::to_value(&report)?;
        self.storage.append_event(NewEvent {
            event_id: Uuid::new_v4().to_string(),
            task_id: task.into(),
            turn_id: Some(turn.into()),
            event_type: "codex_child_report".into(),
            payload,
            created_at_ms: unix_time_ms()?,
        })?;
        self.child_result_reports.insert(turn.into(), report);
        Ok(())
    }
}

pub(super) fn load_child_reports(
    storage: &Storage,
) -> Result<HashMap<String, ChildReport>, DesktopError> {
    let mut reports = HashMap::new();
    for task in storage.list_tasks()? {
        for event in storage.load_events(&task.task_id)? {
            if event.event_type == "codex_child_report"
                && let Some(turn) = event.turn_id
            {
                reports.insert(turn, serde_json::from_value(event.payload)?);
            }
        }
    }
    Ok(reports)
}
