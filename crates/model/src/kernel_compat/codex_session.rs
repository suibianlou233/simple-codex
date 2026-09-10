//! Simple-owned Codex compatibility layer.
use std::collections::HashSet;

use async_trait::async_trait;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{CodexKernelClient, CodexKernelError, CodexThreadItem};

const TURN_PAGE_SIZE: u32 = 100;
const ITEM_PAGE_SIZE: u32 = 200;
const MAX_HISTORY_PAGES: usize = 10_000;

/// Simple chooses policy; this module owns its native wire representation.
pub struct CodexThreadOptions<'a> {
    pub cwd: &'a std::path::Path,
    pub model: Option<&'a str>,
    pub approval_policy: &'a str,
    pub sandbox: &'a str,
    pub context_window: Option<i64>,
}

impl CodexThreadOptions<'_> {
    fn params(&self) -> Value {
        let mut value = json!({
            "cwd": self.cwd, "approvalPolicy": self.approval_policy,
            "approvalsReviewer": "user", "sandbox": self.sandbox,
            "ephemeral": false, "config": { "web_search": "disabled" }
        });
        if let Some(model) = self.model {
            value["model"] = json!(model);
        }
        if let Some(window) = self.context_window {
            value["config"]["model_context_window"] = json!(window);
        }
        value
    }
}

pub enum CodexForkPoint<'a> {
    Before(&'a str),
    Through(&'a str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexHistoryTurn {
    pub id: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexHistoryItem {
    pub turn_id: String,
    pub item: CodexThreadItem,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexThreadSnapshot {
    pub thread_id: String,
    pub thread: Value,
    pub turns: Vec<CodexHistoryTurn>,
    pub items: Vec<CodexHistoryItem>,
}

#[derive(Debug, Error)]
pub enum CodexSessionError {
    #[error(transparent)]
    Kernel(#[from] CodexKernelError),
    #[error("Codex 会话历史响应无效：{0}")]
    InvalidResponse(&'static str),
    #[error("Codex 会话历史分页游标重复，已停止恢复")]
    CursorLoop,
    #[error("Codex 会话历史页数超过安全上限")]
    PageLimitExceeded,
    #[error("Codex 会话历史包含重复的 {kind} 标识 `{id}`")]
    DuplicateId { kind: &'static str, id: String },
    #[error("Codex 历史 item `{item_id}` 指向未知 turn `{turn_id}`")]
    UnknownItemTurn { item_id: String, turn_id: String },
}

#[async_trait]
pub trait CodexRpc: Send + Sync {
    async fn request(&self, method: &str, params: Value) -> Result<Value, CodexKernelError>;
}

#[async_trait]
impl CodexRpc for CodexKernelClient {
    async fn request(&self, method: &str, params: Value) -> Result<Value, CodexKernelError> {
        CodexKernelClient::request(self, method.to_owned(), params).await
    }
}

pub struct CodexSessionBridge<R> {
    rpc: R,
}

impl<R> CodexSessionBridge<R>
where
    R: CodexRpc,
{
    pub fn new(rpc: R) -> Self {
        Self { rpc }
    }

    pub async fn start_thread(
        &self,
        options: CodexThreadOptions<'_>,
    ) -> Result<String, CodexSessionError> {
        let mut params = options.params();
        params["dynamicTools"] = super::browser_tools::tools();
        let response = self.rpc.request("thread/start", params).await?;
        response_thread_id(&response)
    }

    pub async fn fork_thread(
        &self,
        thread_id: &str,
        point: CodexForkPoint<'_>,
        options: CodexThreadOptions<'_>,
    ) -> Result<String, CodexSessionError> {
        let mut params = options.params();
        params["threadId"] = json!(thread_id);
        params["excludeTurns"] = json!(true);
        match point {
            CodexForkPoint::Before(id) => params["beforeTurnId"] = json!(id),
            CodexForkPoint::Through(id) => params["lastTurnId"] = json!(id),
        }
        let response = self.rpc.request("thread/fork", params).await?;
        response_thread_id(&response)
    }

    pub async fn revert_before(
        &self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<(), CodexSessionError> {
        self.rpc
            .request(
                "thread/revert",
                json!({"threadId": thread_id, "beforeTurnId": turn_id}),
            )
            .await?;
        Ok(())
    }

    pub async fn inject_items(
        &self,
        thread_id: &str,
        items: &[Value],
    ) -> Result<(), CodexSessionError> {
        if !items.is_empty() {
            self.rpc
                .request(
                    "thread/inject_items",
                    json!({"threadId": thread_id, "items": items}),
                )
                .await?;
        }
        Ok(())
    }

    /// Resume a persisted thread and rebuild its complete canonical history.
    ///
    /// `thread/resume` deliberately excludes turns. Paginated turn and item APIs
    /// are then read in ascending order so callers never have to merge partial
    /// desktop state with Codex's authoritative rollout history.
    pub async fn resume_and_hydrate(
        &self,
        thread_id: &str,
    ) -> Result<CodexThreadSnapshot, CodexSessionError> {
        if thread_id.is_empty() {
            return Err(CodexSessionError::InvalidResponse("thread id 不能为空"));
        }
        let response = self
            .rpc
            .request(
                "thread/resume",
                json!({ "threadId": thread_id, "excludeTurns": true }),
            )
            .await?;
        let thread = response
            .get("thread")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or(CodexSessionError::InvalidResponse(
                "thread/resume 缺少 thread",
            ))?;
        let resumed_id = required_non_empty_string(&thread, "id", "thread/resume 缺少 thread.id")?;
        if resumed_id != thread_id {
            return Err(CodexSessionError::InvalidResponse(
                "thread/resume 返回了不同的 thread.id",
            ));
        }

        self.hydrate(thread_id, thread).await
    }

    /// Read existing history without resuming, submitting input or changing permissions.
    pub async fn read_and_hydrate(
        &self,
        thread_id: &str,
    ) -> Result<CodexThreadSnapshot, CodexSessionError> {
        if thread_id.is_empty() {
            return Err(CodexSessionError::InvalidResponse("thread id 不能为空"));
        }
        let response = self
            .rpc
            .request(
                "thread/read",
                json!({"threadId":thread_id,"includeTurns":false}),
            )
            .await?;
        let thread = response
            .get("thread")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or(CodexSessionError::InvalidResponse(
                "thread/read 缺少 thread",
            ))?;
        if required_non_empty_string(&thread, "id", "thread/read 缺少 thread.id")? != thread_id {
            return Err(CodexSessionError::InvalidResponse(
                "thread/read 返回了不同的 thread.id",
            ));
        }
        self.hydrate(thread_id, thread).await
    }

    async fn hydrate(
        &self,
        thread_id: &str,
        thread: Value,
    ) -> Result<CodexThreadSnapshot, CodexSessionError> {
        let turns = self.list_all_turns(thread_id).await?;
        let known_turns = turns
            .iter()
            .map(|turn| turn.id.as_str())
            .collect::<HashSet<_>>();
        let items = self.list_all_items(thread_id, &known_turns).await?;
        Ok(CodexThreadSnapshot {
            thread_id: thread_id.to_owned(),
            thread,
            turns,
            items,
        })
    }

    async fn list_all_turns(
        &self,
        thread_id: &str,
    ) -> Result<Vec<CodexHistoryTurn>, CodexSessionError> {
        let mut cursor = None;
        let mut seen_cursors = HashSet::new();
        let mut seen_turns = HashSet::new();
        let mut turns = Vec::new();
        for _ in 0..MAX_HISTORY_PAGES {
            let response = self
                .rpc
                .request(
                    "thread/turns/list",
                    json!({
                        "threadId": thread_id,
                        "cursor": cursor,
                        "limit": TURN_PAGE_SIZE,
                        "sortDirection": "asc",
                        "itemsView": "notLoaded"
                    }),
                )
                .await?;
            let page = required_array(&response, "data", "thread/turns/list 缺少 data")?;
            for value in page {
                let id = required_non_empty_string(value, "id", "turn 缺少 id")?;
                if !seen_turns.insert(id.to_owned()) {
                    return Err(CodexSessionError::DuplicateId {
                        kind: "turn",
                        id: id.to_owned(),
                    });
                }
                turns.push(CodexHistoryTurn {
                    id: id.to_owned(),
                    value: value.clone(),
                });
            }
            cursor = next_cursor(&response)?;
            if !remember_cursor(&mut seen_cursors, cursor.as_deref())? {
                return Ok(turns);
            }
        }
        Err(CodexSessionError::PageLimitExceeded)
    }

    async fn list_all_items(
        &self,
        thread_id: &str,
        known_turns: &HashSet<&str>,
    ) -> Result<Vec<CodexHistoryItem>, CodexSessionError> {
        let mut cursor = None;
        let mut seen_cursors = HashSet::new();
        let mut seen_items = HashSet::new();
        let mut items = Vec::new();
        for _ in 0..MAX_HISTORY_PAGES {
            let response = self
                .rpc
                .request(
                    "thread/items/list",
                    json!({
                        "threadId": thread_id,
                        "turnId": null,
                        "cursor": cursor,
                        "limit": ITEM_PAGE_SIZE,
                        "sortDirection": "asc"
                    }),
                )
                .await?;
            let page = required_array(&response, "data", "thread/items/list 缺少 data")?;
            for entry in page {
                let turn_id = required_non_empty_string(entry, "turnId", "item entry 缺少 turnId")?;
                let item_value = entry
                    .get("item")
                    .ok_or(CodexSessionError::InvalidResponse("item entry 缺少 item"))?;
                let item = CodexThreadItem::parse(item_value)?;
                if !known_turns.contains(turn_id) {
                    return Err(CodexSessionError::UnknownItemTurn {
                        item_id: item.id,
                        turn_id: turn_id.to_owned(),
                    });
                }
                if !seen_items.insert(item.id.clone()) {
                    return Err(CodexSessionError::DuplicateId {
                        kind: "item",
                        id: item.id,
                    });
                }
                items.push(CodexHistoryItem {
                    turn_id: turn_id.to_owned(),
                    item,
                });
            }
            cursor = next_cursor(&response)?;
            if !remember_cursor(&mut seen_cursors, cursor.as_deref())? {
                return Ok(items);
            }
        }
        Err(CodexSessionError::PageLimitExceeded)
    }
}

fn required_array<'a>(
    value: &'a Value,
    key: &str,
    message: &'static str,
) -> Result<&'a Vec<Value>, CodexSessionError> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or(CodexSessionError::InvalidResponse(message))
}

fn response_thread_id(response: &Value) -> Result<String, CodexSessionError> {
    let thread = response
        .get("thread")
        .ok_or(CodexSessionError::InvalidResponse("缺少 thread"))?;
    required_non_empty_string(thread, "id", "缺少 thread.id").map(str::to_owned)
}

fn required_non_empty_string<'a>(
    value: &'a Value,
    key: &str,
    message: &'static str,
) -> Result<&'a str, CodexSessionError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(CodexSessionError::InvalidResponse(message))
}

fn next_cursor(value: &Value) -> Result<Option<String>, CodexSessionError> {
    match value.get("nextCursor") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(cursor)) if !cursor.is_empty() => Ok(Some(cursor.clone())),
        Some(_) => Err(CodexSessionError::InvalidResponse(
            "nextCursor 必须是非空字符串或 null",
        )),
    }
}

fn remember_cursor(
    seen: &mut HashSet<String>,
    cursor: Option<&str>,
) -> Result<bool, CodexSessionError> {
    let Some(cursor) = cursor else {
        return Ok(false);
    };
    if !seen.insert(cursor.to_owned()) {
        return Err(CodexSessionError::CursorLoop);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    async fn mutation_wire_contracts_remain_stable() -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![
            json!({"thread":{"id":"created"}}),
            json!({"thread":{"id":"before"}}),
            json!({"thread":{"id":"through"}}),
            json!({}),
            json!({}),
        ]);
        let session = CodexSessionBridge::new(rpc.clone());
        let options = || CodexThreadOptions {
            cwd: std::path::Path::new("project"),
            model: Some("local-model"),
            approval_policy: "on-request",
            sandbox: "read-only",
            context_window: Some(32000),
        };
        assert_eq!(session.start_thread(options()).await?, "created");
        assert_eq!(
            session
                .fork_thread("source", CodexForkPoint::Before("t"), options())
                .await?,
            "before"
        );
        assert_eq!(
            session
                .fork_thread("source", CodexForkPoint::Through("t"), options())
                .await?,
            "through"
        );
        session.revert_before("source", "t").await?;
        session.inject_items("source", &[]).await?; // No empty mutation RPC.
        session
            .inject_items("source", &[json!({"type":"message"})])
            .await?;
        let calls = rpc.calls()?;
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0].0, "thread/start");
        assert_eq!(calls[0].1["dynamicTools"][0]["name"], "simple_browser");
        assert_eq!(calls[0].1["config"]["web_search"], "disabled");
        assert_eq!(calls[0].1["config"]["model_context_window"], 32000);
        assert_eq!(calls[0].1["approvalPolicy"], "on-request");
        assert_eq!(calls[0].1["sandbox"], "read-only");
        assert_eq!(calls[0].1["approvalsReviewer"], "user");
        assert_eq!(calls[0].1["ephemeral"], false);
        assert_eq!(calls[1].0, "thread/fork");
        assert_eq!(calls[1].1["beforeTurnId"], "t");
        assert!(calls[1].1.get("lastTurnId").is_none());
        assert_eq!(calls[2].1["lastTurnId"], "t");
        assert!(calls[2].1.get("beforeTurnId").is_none());
        assert_eq!(calls[3].0, "thread/revert");
        assert_eq!(calls[4].0, "thread/inject_items");
        Ok(())
    }

    #[tokio::test]
    async fn new_thread_rejects_missing_identity() {
        for response in [json!({}), json!({"thread":{"id":""}})] {
            let session = CodexSessionBridge::new(FakeRpc::new(vec![response]));
            let result = session
                .start_thread(CodexThreadOptions {
                    cwd: std::path::Path::new("project"),
                    model: None,
                    approval_policy: "on-request",
                    sandbox: "read-only",
                    context_window: None,
                })
                .await;
            assert!(matches!(result, Err(CodexSessionError::InvalidResponse(_))));
        }
    }

    #[derive(Clone)]
    struct FakeRpc {
        responses: Arc<Mutex<VecDeque<Value>>>,
        calls: Arc<Mutex<Vec<(String, Value)>>>,
    }

    impl FakeRpc {
        fn new(responses: Vec<Value>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into())),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Result<Vec<(String, Value)>, CodexSessionError> {
            self.calls
                .lock()
                .map(|calls| calls.clone())
                .map_err(|_| CodexSessionError::InvalidResponse("测试调用锁损坏"))
        }
    }

    #[async_trait]
    impl CodexRpc for FakeRpc {
        async fn request(&self, method: &str, params: Value) -> Result<Value, CodexKernelError> {
            self.calls
                .lock()
                .map_err(|_| CodexKernelError::Unavailable)?
                .push((method.to_owned(), params));
            self.responses
                .lock()
                .map_err(|_| CodexKernelError::Unavailable)?
                .pop_front()
                .ok_or(CodexKernelError::Unavailable)
        }
    }

    #[tokio::test]
    async fn hydrates_all_pages_in_ascending_order() -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![
            json!({"thread": {"id": "thread-1"}}),
            json!({"data": [{"id": "turn-1"}], "nextCursor": "turn-page-2"}),
            json!({"data": [{"id": "turn-2"}], "nextCursor": null}),
            json!({"data": [{"turnId": "turn-1", "item": {"id": "item-1", "type": "userMessage", "content": []}}], "nextCursor": "item-page-2"}),
            json!({"data": [{"turnId": "turn-2", "item": {"id": "item-2", "type": "agentMessage", "text": "done"}}], "nextCursor": null}),
        ]);
        let snapshot = CodexSessionBridge::new(rpc.clone())
            .resume_and_hydrate("thread-1")
            .await?;
        assert_eq!(
            snapshot
                .turns
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-1", "turn-2"]
        );
        assert_eq!(
            snapshot
                .items
                .iter()
                .map(|entry| entry.item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["item-1", "item-2"]
        );
        let calls = rpc.calls()?;
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[1].1["sortDirection"], "asc");
        assert_eq!(calls[1].1["itemsView"], "notLoaded");
        assert_eq!(calls[4].1["cursor"], "item-page-2");
        Ok(())
    }

    #[tokio::test]
    async fn readonly_hydration_never_resumes_and_rejects_wrong_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![
            json!({"thread":{"id":"thread"}}),
            json!({"data":[],"nextCursor":null}),
            json!({"data":[],"nextCursor":null}),
        ]);
        let history = CodexSessionBridge::new(rpc.clone())
            .read_and_hydrate("thread")
            .await?;
        assert_eq!(history.thread_id, "thread");
        assert_eq!(
            rpc.calls()?
                .iter()
                .map(|(method, _)| method.as_str())
                .collect::<Vec<_>>(),
            vec!["thread/read", "thread/turns/list", "thread/items/list"]
        );
        let rpc = FakeRpc::new(vec![json!({"thread":{"id":"other"}})]);
        assert!(
            CodexSessionBridge::new(rpc.clone())
                .read_and_hydrate("thread")
                .await
                .is_err()
        );
        assert_eq!(rpc.calls()?.len(), 1);
        assert!(
            CodexSessionBridge::new(rpc.clone())
                .read_and_hydrate("")
                .await
                .is_err()
        );
        assert_eq!(rpc.calls()?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_repeated_pagination_cursor() -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![
            json!({"thread": {"id": "thread-1"}}),
            json!({"data": [{"id": "turn-1"}], "nextCursor": "again"}),
            json!({"data": [{"id": "turn-2"}], "nextCursor": "again"}),
        ]);
        let error = CodexSessionBridge::new(rpc)
            .resume_and_hydrate("thread-1")
            .await
            .err()
            .ok_or("expected cursor loop")?;
        assert!(matches!(error, CodexSessionError::CursorLoop));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_mismatched_thread_id() -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![json!({"thread": {"id": "other"}})]);
        let error = CodexSessionBridge::new(rpc)
            .resume_and_hydrate("thread-1")
            .await
            .err()
            .ok_or("expected thread mismatch")?;
        assert!(matches!(error, CodexSessionError::InvalidResponse(_)));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_malformed_item() -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![
            json!({"thread": {"id": "thread-1"}}),
            json!({"data": [{"id": "turn-1"}], "nextCursor": null}),
            json!({"data": [{"turnId": "turn-1", "item": {"type": "agentMessage"}}], "nextCursor": null}),
        ]);
        let error = CodexSessionBridge::new(rpc)
            .resume_and_hydrate("thread-1")
            .await
            .err()
            .ok_or("expected malformed item")?;
        assert!(matches!(error, CodexSessionError::Kernel(_)));
        Ok(())
    }

    #[tokio::test]
    async fn rejects_duplicate_item_ids_across_pages() -> Result<(), Box<dyn std::error::Error>> {
        let rpc = FakeRpc::new(vec![
            json!({"thread": {"id": "thread-1"}}),
            json!({"data": [{"id": "turn-1"}], "nextCursor": null}),
            json!({"data": [{"turnId": "turn-1", "item": {"id": "same", "type": "agentMessage", "text": "first"}}], "nextCursor": "next"}),
            json!({"data": [{"turnId": "turn-1", "item": {"id": "same", "type": "agentMessage", "text": "second"}}], "nextCursor": null}),
        ]);
        let error = CodexSessionBridge::new(rpc)
            .resume_and_hydrate("thread-1")
            .await
            .err()
            .ok_or("expected duplicate item")?;
        assert!(matches!(
            error,
            CodexSessionError::DuplicateId { kind: "item", .. }
        ));
        Ok(())
    }
}
