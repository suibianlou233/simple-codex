//! Simple-owned Codex compatibility layer.
//! Read-only recovery of a submitted turn's identity, never a retry of its input.
use std::collections::HashSet;
use std::time::Duration;

use serde_json::{Value, json};

use crate::{CodexKernelError, CodexRpc};

const PAGE_SIZE: usize = 200;
const MAX_PAGES: usize = 256;

pub struct CodexStartReceipt {
    /// A recovered receipt contains only a verified id, not an invented status.
    pub turn: Value,
    pub recovered: bool,
}

/// Submit once. If the reply is lost or malformed, look up the already sent
/// client id instead of calling turn/start a second time.
pub async fn start_codex_turn_recovering(
    rpc: &impl CodexRpc,
    thread: &str,
    client_message_id: &str,
    params: Value,
) -> Result<CodexStartReceipt, CodexKernelError> {
    if thread.is_empty()
        || client_message_id.is_empty()
        || params["threadId"].as_str() != Some(thread)
        || params["clientUserMessageId"].as_str() != Some(client_message_id)
    {
        return Err(invalid("启动请求关联标识不一致，未提交"));
    }
    if let Ok(response) = rpc.request("turn/start", params).await
        && response["turn"]["id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    {
        return Ok(CodexStartReceipt {
            turn: response["turn"].clone(),
            recovered: false,
        });
    }
    // Native history may lag the start acknowledgement. Re-read only; never
    // resend input. A bounded timeout remains uncertainty, not non-execution.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match find_codex_submission(rpc, thread, client_message_id).await {
                Ok(Some(id)) => {
                    return Ok(CodexStartReceipt {
                        turn: json!({"id":id}),
                        recovered: true,
                    });
                }
                // The pinned thread store can report Unsupported while the
                // first item is not queryable yet. Retry reads within the same
                // deadline; never reinterpret this as an absent submission.
                Ok(None) | Err(CodexKernelError::Rpc(_)) | Err(CodexKernelError::Timeout) => {}
                Err(error) => return Err(error),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| invalid("无法确认本次启动结果；未自动重新提交"))?
}

/// Find the one native user-message item carrying this Simple submission id.
/// `None` means not observed, NOT "never submitted" or permission to resubmit.
/// Duplicate correlation ids, incomplete pagination and unreadable history fail
/// closed. This does not resume a thread, execute tools or grant permissions.
pub async fn find_codex_submission(
    rpc: &impl CodexRpc,
    thread: &str,
    client_message_id: &str,
) -> Result<Option<String>, CodexKernelError> {
    if thread.is_empty() || client_message_id.is_empty() {
        return Err(invalid("启动结果核对缺少关联标识"));
    }
    tokio::time::timeout(
        Duration::from_secs(10),
        find(rpc, thread, client_message_id),
    )
    .await
    .map_err(|_| invalid("启动结果核对超时，不能确认是否已提交"))?
}

async fn find(
    rpc: &impl CodexRpc,
    thread: &str,
    client_message_id: &str,
) -> Result<Option<String>, CodexKernelError> {
    let response = rpc
        .request(
            "thread/read",
            json!({"threadId":thread,"includeTurns":false}),
        )
        .await?;
    if response["thread"]["id"].as_str() != Some(thread) {
        return Err(invalid("启动结果核对返回了不同的会话"));
    }
    let mut cursor: Option<String> = None;
    let mut cursors = HashSet::new();
    let mut item_ids = HashSet::new();
    let mut matched = None;
    for _ in 0..MAX_PAGES {
        let response = rpc
            .request(
                "thread/items/list",
                json!({"threadId":thread,"cursor":cursor,"limit":PAGE_SIZE,"sortDirection":"desc"}),
            )
            .await?;
        let rows = response["data"]
            .as_array()
            .ok_or_else(|| invalid("启动结果核对缺少消息列表"))?;
        if rows.len() > PAGE_SIZE {
            return Err(invalid("启动结果核对消息页超过上限"));
        }
        for row in rows {
            let turn = nonempty(&row["turnId"])?;
            let item = &row["item"];
            let id = nonempty(&item["id"])?;
            if !item_ids.insert(id.to_owned()) {
                return Err(invalid("启动结果核对出现重复消息，不能确认归属"));
            }
            if nonempty(&item["type"])? == "userMessage" {
                let client_id = match item.get("clientId") {
                    Some(Value::Null) => None,
                    Some(Value::String(id)) if !id.is_empty() => Some(id.as_str()),
                    _ => return Err(invalid("原生用户消息关联标识无效")),
                };
                if client_id == Some(client_message_id) {
                    if matched.is_some() {
                        return Err(invalid("同一提交标识对应多条用户消息，不能自动恢复"));
                    }
                    matched = Some(turn.to_owned());
                }
            }
        }
        match response.get("nextCursor") {
            Some(Value::Null) => return Ok(matched),
            Some(Value::String(next)) if !next.is_empty() && cursors.insert(next.clone()) => {
                cursor = Some(next.clone())
            }
            _ => return Err(invalid("启动结果核对分页不完整或重复")),
        }
    }
    Err(invalid("启动结果核对超过历史上限，不能确认归属"))
}

fn nonempty(value: &Value) -> Result<&str, CodexKernelError> {
    value
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("启动结果核对缺少有效标识"))
}

fn invalid(message: &'static str) -> CodexKernelError {
    CodexKernelError::InvalidWireMessage(message)
}

#[cfg(test)]
#[path = "codex_submission_tests.rs"]
mod tests;
