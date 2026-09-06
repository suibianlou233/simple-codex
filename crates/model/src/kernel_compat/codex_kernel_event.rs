//! Simple-owned Codex compatibility layer.
use serde_json::Value;

use crate::CodexKernelError;
use crate::CodexKernelWireMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexMessagePhase {
    Commentary,
    FinalAnswer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexTurnStatus {
    InProgress,
    Completed,
    Interrupted,
    Failed,
}

impl CodexTurnStatus {
    fn parse(value: &str) -> Result<Self, CodexKernelError> {
        match value {
            "inProgress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "interrupted" => Ok(Self::Interrupted),
            "failed" => Ok(Self::Failed),
            _ => Err(CodexKernelError::InvalidWireMessage(
                "未知的 Codex turn 状态",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexThreadItem {
    pub id: String,
    pub kind: String,
    pub value: Value,
}

impl CodexThreadItem {
    pub(crate) fn parse(value: &Value) -> Result<Self, CodexKernelError> {
        Ok(Self {
            id: required_string(value, "id", "item 缺少 id")?,
            kind: required_string(value, "type", "item 缺少 type")?,
            value: value.clone(),
        })
    }

    pub fn agent_text(&self) -> Option<&str> {
        (self.kind == "agentMessage")
            .then(|| self.value.get("text").and_then(Value::as_str))
            .flatten()
    }

    /// Transport completion is not proof that a command succeeded. Missing or
    /// conflicting exit information must not become an applied desktop action.
    pub fn execution_succeeded(&self) -> bool {
        self.value.get("status").and_then(Value::as_str) == Some("completed")
            && match self.kind.as_str() {
                "commandExecution" => self.value.get("exitCode").and_then(Value::as_i64) == Some(0),
                "fileChange" => true,
                _ => false,
            }
    }

    pub fn agent_phase(&self) -> Option<CodexMessagePhase> {
        if self.kind != "agentMessage" {
            return None;
        }
        self.value
            .get("phase")
            .and_then(|phase| serde_json::from_value(phase.clone()).ok())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexApprovalKind {
    CommandExecution,
    FileChange,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexApprovalRequest {
    pub request_id: Value,
    pub kind: CodexApprovalKind,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub approval_id: Option<String>,
    pub started_at_ms: i64,
    pub reason: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub params: Value,
}

impl CodexApprovalRequest {
    /// Stable desktop action id. Bridge sub-approvals have their own callback id;
    /// ordinary command and file approvals use the item id.
    pub fn action_id(&self) -> &str {
        self.approval_id.as_deref().unwrap_or(&self.item_id)
    }
}

/// Typed lifecycle facts consumed by the desktop projection.
///
/// This is intentionally separate from `ModelEvent`: these messages describe
/// Codex thread/item/approval state, not provider streaming protocol details.
#[derive(Debug, Clone, PartialEq)]
pub enum CodexKernelEvent {
    ThreadStarted {
        thread_id: String,
        thread: Value,
    },
    TurnStarted {
        thread_id: String,
        turn_id: String,
        turn: Value,
    },
    ItemStarted {
        thread_id: String,
        turn_id: String,
        started_at_ms: i64,
        item: CodexThreadItem,
    },
    AgentMessageDelta {
        thread_id: String,
        turn_id: String,
        item_id: String,
        delta: String,
    },
    ItemCompleted {
        thread_id: String,
        turn_id: String,
        completed_at_ms: i64,
        item: CodexThreadItem,
    },
    TurnCompleted {
        thread_id: String,
        turn_id: String,
        status: CodexTurnStatus,
        error_message: Option<String>,
        turn: Value,
    },
    Error {
        thread_id: String,
        turn_id: String,
        will_retry: bool,
        error: Value,
    },
    ApprovalRequested(CodexApprovalRequest),
    ServerRequestResolved {
        thread_id: String,
        request_id: Value,
    },
    OtherNotification {
        method: String,
        params: Value,
    },
    OtherRequest {
        id: Value,
        method: String,
        params: Value,
    },
    OrphanResponse {
        id: Value,
        result: Option<Value>,
        error: Option<Value>,
    },
}

impl CodexKernelEvent {
    pub fn project(message: CodexKernelWireMessage) -> Result<Self, CodexKernelError> {
        match message {
            CodexKernelWireMessage::Notification { method, params } => {
                project_notification(method, params)
            }
            CodexKernelWireMessage::Request { id, method, params } => {
                project_request(id, method, params)
            }
            CodexKernelWireMessage::Response { id, result, error } => {
                Ok(Self::OrphanResponse { id, result, error })
            }
        }
    }
}

fn project_notification(
    method: String,
    params: Value,
) -> Result<CodexKernelEvent, CodexKernelError> {
    match method.as_str() {
        "thread/started" => {
            let thread = required_value(&params, "thread", "thread/started 缺少 thread")?;
            Ok(CodexKernelEvent::ThreadStarted {
                thread_id: required_string(thread, "id", "thread 缺少 id")?,
                thread: thread.clone(),
            })
        }
        "turn/started" => {
            let turn = required_value(&params, "turn", "turn/started 缺少 turn")?;
            Ok(CodexKernelEvent::TurnStarted {
                thread_id: required_string(&params, "threadId", "turn/started 缺少 threadId")?,
                turn_id: required_string(turn, "id", "turn 缺少 id")?,
                turn: turn.clone(),
            })
        }
        "item/started" => Ok(CodexKernelEvent::ItemStarted {
            thread_id: required_string(&params, "threadId", "item/started 缺少 threadId")?,
            turn_id: required_string(&params, "turnId", "item/started 缺少 turnId")?,
            started_at_ms: required_i64(&params, "startedAtMs", "item/started 缺少 startedAtMs")?,
            item: CodexThreadItem::parse(required_value(
                &params,
                "item",
                "item/started 缺少 item",
            )?)?,
        }),
        "item/agentMessage/delta" => Ok(CodexKernelEvent::AgentMessageDelta {
            thread_id: required_string(&params, "threadId", "文本增量缺少 threadId")?,
            turn_id: required_string(&params, "turnId", "文本增量缺少 turnId")?,
            item_id: required_string(&params, "itemId", "文本增量缺少 itemId")?,
            delta: required_string(&params, "delta", "文本增量缺少 delta")?,
        }),
        "item/completed" => Ok(CodexKernelEvent::ItemCompleted {
            thread_id: required_string(&params, "threadId", "item/completed 缺少 threadId")?,
            turn_id: required_string(&params, "turnId", "item/completed 缺少 turnId")?,
            completed_at_ms: required_i64(
                &params,
                "completedAtMs",
                "item/completed 缺少 completedAtMs",
            )?,
            item: CodexThreadItem::parse(required_value(
                &params,
                "item",
                "item/completed 缺少 item",
            )?)?,
        }),
        "turn/completed" => {
            let turn = required_value(&params, "turn", "turn/completed 缺少 turn")?;
            let status = CodexTurnStatus::parse(
                required_value(turn, "status", "turn 缺少 status")?
                    .as_str()
                    .ok_or(CodexKernelError::InvalidWireMessage(
                        "turn status 必须是字符串",
                    ))?,
            )?;
            Ok(CodexKernelEvent::TurnCompleted {
                thread_id: required_string(&params, "threadId", "turn/completed 缺少 threadId")?,
                turn_id: required_string(turn, "id", "turn 缺少 id")?,
                status,
                error_message: turn
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                turn: turn.clone(),
            })
        }
        "error" => Ok(CodexKernelEvent::Error {
            thread_id: required_string(&params, "threadId", "error 通知缺少 threadId")?,
            turn_id: required_string(&params, "turnId", "error 通知缺少 turnId")?,
            will_retry: params
                .get("willRetry")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            error: required_value(&params, "error", "error 通知缺少 error")?.clone(),
        }),
        "serverRequest/resolved" => Ok(CodexKernelEvent::ServerRequestResolved {
            thread_id: required_string(
                &params,
                "threadId",
                "serverRequest/resolved 缺少 threadId",
            )?,
            request_id: required_value(
                &params,
                "requestId",
                "serverRequest/resolved 缺少 requestId",
            )?
            .clone(),
        }),
        _ => Ok(CodexKernelEvent::OtherNotification { method, params }),
    }
}

fn project_request(
    id: Value,
    method: String,
    params: Value,
) -> Result<CodexKernelEvent, CodexKernelError> {
    let kind = match method.as_str() {
        "item/commandExecution/requestApproval" => CodexApprovalKind::CommandExecution,
        "item/fileChange/requestApproval" => CodexApprovalKind::FileChange,
        _ => return Ok(CodexKernelEvent::OtherRequest { id, method, params }),
    };

    Ok(CodexKernelEvent::ApprovalRequested(CodexApprovalRequest {
        request_id: id,
        kind,
        thread_id: required_string(&params, "threadId", "审批请求缺少 threadId")?,
        turn_id: required_string(&params, "turnId", "审批请求缺少 turnId")?,
        item_id: required_string(&params, "itemId", "审批请求缺少 itemId")?,
        approval_id: optional_string(&params, "approvalId")?,
        started_at_ms: required_i64(&params, "startedAtMs", "审批请求缺少 startedAtMs")?,
        reason: optional_string(&params, "reason")?,
        command: optional_string(&params, "command")?,
        cwd: optional_string(&params, "cwd")?,
        params,
    }))
}

fn required_value<'a>(
    value: &'a Value,
    field: &str,
    message: &'static str,
) -> Result<&'a Value, CodexKernelError> {
    value
        .get(field)
        .ok_or(CodexKernelError::InvalidWireMessage(message))
}

fn required_string(
    value: &Value,
    field: &str,
    message: &'static str,
) -> Result<String, CodexKernelError> {
    required_value(value, field, message)?
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(CodexKernelError::InvalidWireMessage(message))
}

fn optional_string(value: &Value, field: &str) -> Result<Option<String>, CodexKernelError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_str().map(|value| Some(value.to_owned())).ok_or(
            CodexKernelError::InvalidWireMessage("可选字符串字段类型无效"),
        ),
    }
}

fn required_i64(
    value: &Value,
    field: &str,
    message: &'static str,
) -> Result<i64, CodexKernelError> {
    required_value(value, field, message)?
        .as_i64()
        .ok_or(CodexKernelError::InvalidWireMessage(message))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn command_completion_requires_a_successful_exit_code() {
        for (status, exit_code, expected) in [
            (json!("completed"), json!(0), true),
            (json!("completed"), json!(37), false),
            (json!("completed"), Value::Null, false),
            (Value::Null, json!(0), false),
            (json!("failed"), json!(0), false),
            (json!("inProgress"), json!(0), false),
        ] {
            let item = CodexThreadItem::parse(&json!({"id":"command", "type":"commandExecution", "status":status, "exitCode":exit_code})).expect("item");
            assert_eq!(item.execution_succeeded(), expected, "{item:?}");
        }
        let patch = CodexThreadItem::parse(
            &json!({"id":"patch", "type":"fileChange", "status":"completed"}),
        )
        .expect("item");
        assert!(patch.execution_succeeded());
    }

    #[test]
    fn projects_agent_delta_with_stable_item_id() {
        let event = CodexKernelEvent::project(CodexKernelWireMessage::Notification {
            method: "item/agentMessage/delta".to_owned(),
            params: json!({
                "threadId": "thread-1",
                "turnId": "turn-1",
                "itemId": "item-1",
                "delta": "hello"
            }),
        })
        .expect("project delta");

        assert!(matches!(
            event,
            CodexKernelEvent::AgentMessageDelta { item_id, delta, .. }
                if item_id == "item-1" && delta == "hello"
        ));
    }

    #[test]
    fn turn_completed_uses_status_instead_of_a_nonexistent_failed_method() {
        let event = CodexKernelEvent::project(CodexKernelWireMessage::Notification {
            method: "turn/completed".to_owned(),
            params: json!({
                "threadId": "thread-1",
                "turn": {
                    "id": "turn-1",
                    "status": "failed",
                    "error": { "message": "model failed" }
                }
            }),
        })
        .expect("project terminal turn");

        assert!(matches!(
            event,
            CodexKernelEvent::TurnCompleted {
                status: CodexTurnStatus::Failed,
                error_message: Some(message),
                ..
            } if message == "model failed"
        ));
    }

    #[test]
    fn approval_keeps_wire_request_id_for_response() {
        let event = CodexKernelEvent::project(CodexKernelWireMessage::Request {
            id: json!(7),
            method: "item/fileChange/requestApproval".to_owned(),
            params: json!({
                "threadId": "thread-1",
                "turnId": "turn-1",
                "itemId": "item-1",
                "startedAtMs": 42
            }),
        })
        .expect("project approval");

        assert!(matches!(
            event,
            CodexKernelEvent::ApprovalRequested(CodexApprovalRequest { request_id, .. })
                if request_id == json!(7)
        ));
    }
}
