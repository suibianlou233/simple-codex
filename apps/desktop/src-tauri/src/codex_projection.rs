//! Pure projection from slim Codex app-server events to the existing desktop
//! turn stream and durable conversation model.
//!
//! The module deliberately does not know about Tauri, SQLite, or the legacy
//! turn engine. The desktop runtime owns those side effects and applies the
//! returned effects in order. Keeping that boundary small also makes it
//! possible to replace the legacy engine without making Codex a second writer
//! of desktop state.

use std::collections::{HashMap, HashSet};

use local_agent_model::{CodexKernelEvent, CodexMessagePhase, CodexThreadItem, CodexTurnStatus};
use serde_json::Value;

/// Correlates one user-visible Simple turn with its Codex app-server ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexTurnBinding {
    pub(crate) task_id: String,
    pub(crate) turn_id: String,
    pub(crate) codex_thread_id: String,
    pub(crate) codex_turn_id: String,
}

/// An existing `turn-stream` event, before the runtime assigns its process-local
/// sequence and event id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedTurnStream {
    pub(crate) phase: Option<CodexMessagePhase>,
    pub(crate) task_id: String,
    pub(crate) turn_id: String,
    pub(crate) item_id: Option<String>,
    pub(crate) kind: &'static str,
    pub(crate) content: Option<String>,
    pub(crate) message: Option<String>,
}

/// A completed assistant item that must be appended to the Simple compatibility
/// journal using `item_id` as the durable event/message id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedAssistantMessage {
    pub(crate) phase: Option<CodexMessagePhase>,
    pub(crate) task_id: String,
    pub(crate) turn_id: String,
    pub(crate) item_id: String,
    pub(crate) content: String,
    /// `None` is possible only when the projector had to recover the final
    /// message from `turn/completed.items`. The runtime should use its local
    /// clock in that fallback case.
    pub(crate) created_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectedTurnStatus {
    Completed,
    Cancelled,
    Failed,
}

impl ProjectedTurnStatus {
    pub(crate) const fn stream_kind(self) -> &'static str {
        match self {
            Self::Completed => "finished",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// Authoritative terminal state. The runtime should persist this first and emit
/// its matching `turn-stream` event only after persistence succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedTurnTerminal {
    pub(crate) task_id: String,
    pub(crate) turn_id: String,
    pub(crate) status: ProjectedTurnStatus,
    pub(crate) error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexDesktopEffect {
    Stream(ProjectedTurnStream),
    PersistAssistant(ProjectedAssistantMessage),
    Finish(ProjectedTurnTerminal),
}

/// Stateful, per-turn projector.
///
/// Codex may repeat notifications around reconnect/replay boundaries. Stable
/// item ids and the terminal latch make applying the resulting effects
/// idempotent before they reach SQLite or React.
pub(crate) struct CodexTurnProjector {
    binding: CodexTurnBinding,
    started: bool,
    completed_assistant_items: HashSet<String>,
    message_phases: HashMap<String, CodexMessagePhase>,
    last_non_retryable_error: Option<String>,
    terminal: bool,
}

impl CodexTurnProjector {
    pub(crate) fn new(binding: CodexTurnBinding) -> Self {
        Self {
            binding,
            started: false,
            completed_assistant_items: HashSet::new(),
            message_phases: HashMap::new(),
            last_non_retryable_error: None,
            terminal: false,
        }
    }

    pub(crate) fn binding(&self) -> &CodexTurnBinding {
        &self.binding
    }

    /// Projects one typed kernel event. Unrelated threads/turns and non-text
    /// items intentionally produce no effects during the pure-text migration
    /// stage.
    pub(crate) fn project(&mut self, event: &CodexKernelEvent) -> Vec<CodexDesktopEffect> {
        if self.terminal {
            return Vec::new();
        }

        match event {
            CodexKernelEvent::ItemStarted {
                thread_id,
                turn_id,
                item,
                ..
            } if self.matches_codex_turn(thread_id, turn_id) => {
                if let Some(phase) = item.agent_phase() {
                    self.message_phases.insert(item.id.clone(), phase);
                }
                Vec::new()
            }
            CodexKernelEvent::TurnStarted {
                thread_id, turn_id, ..
            } if self.matches_codex_turn(thread_id, turn_id) => {
                if self.started {
                    return Vec::new();
                }
                self.started = true;
                vec![CodexDesktopEffect::Stream(
                    self.stream(None, "started", None, None),
                )]
            }
            CodexKernelEvent::AgentMessageDelta {
                thread_id,
                turn_id,
                item_id,
                delta,
            } if self.matches_codex_turn(thread_id, turn_id)
                && !self.completed_assistant_items.contains(item_id) =>
            {
                vec![CodexDesktopEffect::Stream(self.stream(
                    Some(item_id.clone()),
                    "delta",
                    Some(delta.clone()),
                    None,
                ))]
            }
            CodexKernelEvent::ItemCompleted {
                thread_id,
                turn_id,
                completed_at_ms,
                item,
            } if self.matches_codex_turn(thread_id, turn_id) => self
                .complete_assistant_item(item, Some(*completed_at_ms))
                .into_iter()
                .collect(),
            CodexKernelEvent::Error {
                thread_id,
                turn_id,
                will_retry: false,
                error,
            } if self.matches_codex_turn(thread_id, turn_id) => {
                self.last_non_retryable_error = error
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                Vec::new()
            }
            CodexKernelEvent::TurnCompleted {
                thread_id,
                turn_id,
                status,
                error_message,
                turn,
            } if self.matches_codex_turn(thread_id, turn_id) => {
                let mut effects = self.completed_turn_fallback_items(turn);
                let error_message = error_message
                    .clone()
                    .or_else(|| self.last_non_retryable_error.take());
                let (status, error_message) = terminal_projection(*status, error_message);
                self.terminal = true;
                effects.push(CodexDesktopEffect::Finish(ProjectedTurnTerminal {
                    task_id: self.binding.task_id.clone(),
                    turn_id: self.binding.turn_id.clone(),
                    status,
                    error_message,
                }));
                effects
            }
            _ => Vec::new(),
        }
    }

    fn matches_codex_turn(&self, thread_id: &str, turn_id: &str) -> bool {
        self.binding.codex_thread_id == thread_id && self.binding.codex_turn_id == turn_id
    }

    fn stream(
        &self,
        item_id: Option<String>,
        kind: &'static str,
        content: Option<String>,
        message: Option<String>,
    ) -> ProjectedTurnStream {
        ProjectedTurnStream {
            phase: item_id
                .as_ref()
                .and_then(|id| self.message_phases.get(id))
                .copied(),
            task_id: self.binding.task_id.clone(),
            turn_id: self.binding.turn_id.clone(),
            item_id,
            kind,
            content,
            message,
        }
    }

    fn complete_assistant_item(
        &mut self,
        item: &CodexThreadItem,
        completed_at_ms: Option<i64>,
    ) -> Option<CodexDesktopEffect> {
        let content = item.agent_text()?;
        if !self.completed_assistant_items.insert(item.id.clone()) {
            return None;
        }
        if content.is_empty() {
            return None;
        }
        Some(CodexDesktopEffect::PersistAssistant(
            ProjectedAssistantMessage {
                phase: item
                    .agent_phase()
                    .or_else(|| self.message_phases.get(&item.id).copied()),
                task_id: self.binding.task_id.clone(),
                turn_id: self.binding.turn_id.clone(),
                item_id: item.id.clone(),
                content: content.to_owned(),
                created_at_ms: completed_at_ms,
            },
        ))
    }

    /// `turn/completed.items` is only a final-message fallback. Canonical
    /// `item/completed` notifications win and stable ids suppress duplicates.
    fn completed_turn_fallback_items(&mut self, turn: &Value) -> Vec<CodexDesktopEffect> {
        let completed_at_ms = turn
            .get("completedAt")
            .and_then(Value::as_i64)
            .and_then(|seconds| seconds.checked_mul(1_000));
        turn.get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_agent_item)
            .filter_map(|item| self.complete_assistant_item(&item, completed_at_ms))
            .collect()
    }
}

fn parse_agent_item(value: &Value) -> Option<CodexThreadItem> {
    if value.get("type").and_then(Value::as_str) != Some("agentMessage") {
        return None;
    }
    Some(CodexThreadItem {
        id: value.get("id")?.as_str()?.to_owned(),
        kind: "agentMessage".to_owned(),
        value: value.clone(),
    })
}

fn terminal_projection(
    status: CodexTurnStatus,
    error_message: Option<String>,
) -> (ProjectedTurnStatus, Option<String>) {
    match status {
        CodexTurnStatus::Completed => (ProjectedTurnStatus::Completed, None),
        CodexTurnStatus::Interrupted => (ProjectedTurnStatus::Cancelled, None),
        CodexTurnStatus::Failed => (
            ProjectedTurnStatus::Failed,
            Some(error_message.unwrap_or_else(|| "Codex 回合执行失败".to_owned())),
        ),
        CodexTurnStatus::InProgress => (
            ProjectedTurnStatus::Failed,
            Some("Codex 返回了非终态的 turn/completed".to_owned()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn projector() -> CodexTurnProjector {
        CodexTurnProjector::new(CodexTurnBinding {
            task_id: "simple-task".to_owned(),
            turn_id: "simple-turn".to_owned(),
            codex_thread_id: "codex-thread".to_owned(),
            codex_turn_id: "codex-turn".to_owned(),
        })
    }

    #[test]
    fn native_phase_survives_deltas_completion_and_duplicate_events() {
        for (wire_phase, expected) in [
            ("commentary", CodexMessagePhase::Commentary),
            ("final_answer", CodexMessagePhase::FinalAnswer),
        ] {
            let mut projector = projector();
            let item = CodexThreadItem {
                id: "phase-item".into(),
                kind: "agentMessage".into(),
                value: json!({"type":"agentMessage","id":"phase-item","text":"正文","phase":wire_phase}),
            };
            let started = CodexKernelEvent::ItemStarted {
                thread_id: "codex-thread".into(),
                turn_id: "codex-turn".into(),
                started_at_ms: 1,
                item: item.clone(),
            };
            assert!(projector.project(&started).is_empty());
            let delta = projector.project(&CodexKernelEvent::AgentMessageDelta {
                thread_id: "codex-thread".into(),
                turn_id: "codex-turn".into(),
                item_id: "phase-item".into(),
                delta: "正文".into(),
            });
            assert!(
                matches!(delta.as_slice(), [CodexDesktopEffect::Stream(stream)] if stream.phase == Some(expected))
            );
            let done = CodexKernelEvent::ItemCompleted {
                thread_id: "codex-thread".into(),
                turn_id: "codex-turn".into(),
                completed_at_ms: 2,
                item,
            };
            assert!(
                matches!(projector.project(&done).as_slice(), [CodexDesktopEffect::PersistAssistant(message)] if message.phase == Some(expected))
            );
            assert!(projector.project(&done).is_empty());
        }
    }

    #[test]
    fn completed_item_uses_stable_item_id_and_ignores_late_delta() {
        let mut projector = projector();
        let item = CodexThreadItem {
            id: "agent-item".to_owned(),
            kind: "agentMessage".to_owned(),
            value: json!({
                "id": "agent-item",
                "type": "agentMessage",
                "text": "完成"
            }),
        };

        let completed = projector.project(&CodexKernelEvent::ItemCompleted {
            thread_id: "codex-thread".to_owned(),
            turn_id: "codex-turn".to_owned(),
            completed_at_ms: 42,
            item,
        });
        assert!(matches!(
            completed.as_slice(),
            [CodexDesktopEffect::PersistAssistant(message)]
                if message.item_id == "agent-item" && message.content == "完成"
        ));

        let late = projector.project(&CodexKernelEvent::AgentMessageDelta {
            thread_id: "codex-thread".to_owned(),
            turn_id: "codex-turn".to_owned(),
            item_id: "agent-item".to_owned(),
            delta: "迟到".to_owned(),
        });
        assert!(late.is_empty());
    }

    #[test]
    fn failed_turn_uses_error_notification_when_terminal_omits_message() {
        let mut projector = projector();
        assert!(
            projector
                .project(&CodexKernelEvent::Error {
                    thread_id: "codex-thread".to_owned(),
                    turn_id: "codex-turn".to_owned(),
                    will_retry: false,
                    error: json!({ "message": "上游失败" }),
                })
                .is_empty()
        );

        let effects = projector.project(&CodexKernelEvent::TurnCompleted {
            thread_id: "codex-thread".to_owned(),
            turn_id: "codex-turn".to_owned(),
            status: CodexTurnStatus::Failed,
            error_message: None,
            turn: json!({ "items": [] }),
        });
        assert!(matches!(
            effects.as_slice(),
            [CodexDesktopEffect::Finish(ProjectedTurnTerminal {
                status: ProjectedTurnStatus::Failed,
                error_message: Some(message),
                ..
            })] if message == "上游失败"
        ));
    }
}
