//! Simple-owned Codex compatibility layer.
//! Task-tree cancellation over the pinned app-server's public APIs.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{CodexKernelError, CodexRpc};

/// Stop the requested root turn and every loaded spawned descendant, without
/// touching unrelated tasks or deleting resumable history. Re-scan after each
/// pass because a child may have been spawning when the interrupt arrived.
pub async fn stop_codex_task_tree(
    rpc: &impl CodexRpc,
    root: &str,
    turn: &str,
) -> Result<(), CodexKernelError> {
    if root.is_empty() || turn.is_empty() {
        return Err(CodexKernelError::InvalidWireMessage("停止目标不能为空"));
    }
    tokio::time::timeout(Duration::from_secs(45), async {
        if let Err(error) = rpc
            .request("turn/interrupt", json!({"threadId":root,"turnId":turn}))
            .await
        {
            // The root can finish between the click and this request while its
            // children are still working. Continue cleanup only if confirmed idle.
            let current = read_thread(rpc, root).await?;
            if !confirmed_quiet(rpc, root, &current).await? {
                return Err(error);
            }
        }
        let mut known = HashSet::from([root.to_owned()]);
        let mut quiet_passes = 0;
        loop {
            let before = known.len();
            let loaded = collect_task_tree(rpc, root, &mut known).await?;
            let mut running = false;
            for id in &known {
                let thread = loaded
                    .get(id)
                    .ok_or(CodexKernelError::InvalidWireMessage("停止目标缺少状态"))?;
                if confirmed_quiet(rpc, id, thread).await? {
                    continue;
                }
                // Active metadata is not evidence of quiet, even if the turn
                // list lags completion/startup. Require a later idle observation.
                running = true;
                let turns = rpc
                    .request(
                        "thread/turns/list",
                        json!({
                            "threadId":id,"limit":1,"sortDirection":"desc","itemsView":"notLoaded"
                        }),
                    )
                    .await?;
                let data = turns["data"]
                    .as_array()
                    .ok_or(CodexKernelError::InvalidWireMessage(
                        "活动轮次列表缺少 data",
                    ))?;
                let Some(latest) = data.first() else {
                    let current = read_thread(rpc, id).await?;
                    if !confirmed_quiet(rpc, id, &current).await? {
                        return Err(CodexKernelError::InvalidWireMessage(
                            "任务仍活动但没有可停止的轮次",
                        ));
                    }
                    continue;
                };
                match latest["status"].as_str() {
                    Some("inProgress") => {}
                    Some("completed" | "interrupted" | "failed") => continue,
                    _ => return Err(CodexKernelError::InvalidWireMessage("活动轮次状态无效")),
                };
                let turn_id = latest["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or(CodexKernelError::InvalidWireMessage("活动轮次缺少 id"))?;
                if let Err(error) = rpc
                    .request("turn/interrupt", json!({"threadId":id,"turnId":turn_id}))
                    .await
                {
                    // Finishing between read and interrupt is benign; other
                    // failures must not be presented as a successful stop.
                    let current = read_thread(rpc, id).await?;
                    if !confirmed_quiet(rpc, id, &current).await? {
                        return Err(error);
                    }
                }
            }
            quiet_passes = if !running && before == known.len() {
                quiet_passes + 1
            } else {
                0
            };
            if quiet_passes >= 2 {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| CodexKernelError::Timeout)?
}

/// Passive observation of a native task tree. This never interrupts, resumes or
/// loads a task. Two complete quiet observations are required; errors reset that
/// evidence. It is not an atomic native spawn/process-exit barrier.
pub struct CodexTaskTreeWatch {
    root: String,
    known: HashSet<String>,
    quiet_passes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexChildOutcome {
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub status: CodexChildStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexChildStatus {
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

impl CodexTaskTreeWatch {
    /// Seed with ids from native spawn events; ancestry is validated during scan.
    pub fn include_spawned(&mut self, ids: impl IntoIterator<Item = String>) {
        self.known.extend(ids);
        self.quiet_passes = 0;
    }

    /// Result uncertainty is not success, and does not itself mean a thread is
    /// still active. The caller must confirm quiet again after reading results.
    pub async fn child_outcomes(&self, rpc: &impl CodexRpc) -> Vec<CodexChildOutcome> {
        let mut ids = self
            .known
            .iter()
            .filter(|id| *id != &self.root)
            .cloned()
            .collect::<Vec<_>>();
        ids.sort();
        let mut outcomes = Vec::new();
        for id in ids {
            let response = rpc
                .request(
                    "thread/turns/list",
                    json!({"threadId":id,"limit":1,"sortDirection":"desc","itemsView":"notLoaded"}),
                )
                .await;
            let latest = response
                .as_ref()
                .ok()
                .and_then(|response| response["data"].as_array())
                .filter(|data| data.len() == 1)
                .and_then(|data| data.first());
            let turn_id = latest
                .and_then(|turn| turn["id"].as_str())
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
            let status = if turn_id.is_some() {
                match latest.and_then(|turn| turn["status"].as_str()) {
                    Some("completed") => CodexChildStatus::Completed,
                    Some("failed") => CodexChildStatus::Failed,
                    Some("interrupted") => CodexChildStatus::Cancelled,
                    _ => CodexChildStatus::Unknown,
                }
            } else {
                CodexChildStatus::Unknown
            };
            outcomes.push(CodexChildOutcome {
                thread_id: id,
                turn_id,
                status,
            });
        }
        outcomes
    }
    pub fn new(root: &str) -> Result<Self, CodexKernelError> {
        if root.is_empty() {
            return Err(CodexKernelError::InvalidWireMessage("任务树根不能为空"));
        }
        Ok(Self {
            root: root.into(),
            known: HashSet::from([root.into()]),
            quiet_passes: 0,
        })
    }

    pub async fn is_quiet(&mut self, rpc: &impl CodexRpc) -> Result<bool, CodexKernelError> {
        let previous = self.quiet_passes;
        self.quiet_passes = 0;
        let before = self.known.len();
        let tree = collect_task_tree(rpc, &self.root, &mut self.known).await?;
        for id in &self.known {
            if !confirmed_quiet(rpc, id, &tree[id]).await? {
                return Ok(false);
            }
        }
        if before == self.known.len() {
            self.quiet_passes = previous.saturating_add(1);
        }
        Ok(self.quiet_passes >= 2)
    }
}

async fn collect_task_tree(
    rpc: &impl CodexRpc,
    root: &str,
    known: &mut HashSet<String>,
) -> Result<HashMap<String, Value>, CodexKernelError> {
    let mut loaded = loaded_threads(rpc).await?;
    // Metadata-only ancestry also finds descendants behind unloaded parents.
    let candidates = loaded
        .keys()
        .cloned()
        .chain(known.iter().cloned())
        .collect::<Vec<_>>();
    for candidate in candidates {
        let mut next = Some(candidate);
        let mut ancestors = HashSet::new();
        while let Some(id) = next {
            if !ancestors.insert(id.clone()) || ancestors.len() > 512 {
                return Err(CodexKernelError::InvalidWireMessage(
                    "子任务祖先关系循环或超限",
                ));
            }
            if !loaded.contains_key(&id) {
                if loaded.len() >= 10_000 {
                    return Err(CodexKernelError::InvalidWireMessage("任务树超过检查上限"));
                }
                loaded.insert(id.clone(), read_thread(rpc, &id).await?);
            }
            if id == root {
                break;
            }
            next = loaded[&id]["parentThreadId"].as_str().map(str::to_owned);
        }
    }
    loop {
        let before = known.len();
        for (id, thread) in &loaded {
            if thread["parentThreadId"]
                .as_str()
                .is_some_and(|parent| known.contains(parent))
            {
                known.insert(id.clone());
            }
        }
        if before == known.len() {
            break;
        }
    }
    // Seeded ids must actually descend from this root, not just share a host.
    for id in known.iter().filter(|id| id.as_str() != root) {
        let mut cursor = id.as_str();
        let mut seen = HashSet::new();
        while cursor != root {
            if !seen.insert(cursor) {
                return Err(CodexKernelError::InvalidWireMessage("任务树关系循环"));
            }
            cursor = loaded
                .get(cursor)
                .and_then(|thread| thread["parentThreadId"].as_str())
                .ok_or(CodexKernelError::InvalidWireMessage(
                    "已登记子任务不属于当前根任务",
                ))?;
        }
    }
    Ok(loaded)
}

#[cfg(test)]
#[path = "codex_task_control_tests.rs"]
mod tests;

async fn loaded_threads(rpc: &impl CodexRpc) -> Result<HashMap<String, Value>, CodexKernelError> {
    let mut threads = HashMap::new();
    let mut cursor = Value::Null;
    let mut cursors = HashSet::new();
    for _ in 0..100 {
        let page = rpc
            .request("thread/loaded/list", json!({"cursor":cursor,"limit":100}))
            .await?;
        let ids = page["data"]
            .as_array()
            .ok_or(CodexKernelError::InvalidWireMessage(
                "loaded/list 缺少 data",
            ))?;
        for id in ids {
            let id = id
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or(CodexKernelError::InvalidWireMessage("loaded/list 标识无效"))?;
            if threads.len() >= 10_000 {
                return Err(CodexKernelError::InvalidWireMessage("任务树超过检查上限"));
            }
            threads.insert(id.to_owned(), read_thread(rpc, id).await?);
        }
        cursor = page
            .get("nextCursor")
            .cloned()
            .ok_or(CodexKernelError::InvalidWireMessage(
                "loaded/list 缺少 nextCursor",
            ))?;
        if cursor.is_null() {
            return Ok(threads);
        }
        if !cursor.as_str().is_some_and(|value| !value.is_empty()) {
            return Err(CodexKernelError::InvalidWireMessage("loaded/list 游标无效"));
        }
        if !cursors.insert(cursor.to_string()) {
            return Err(CodexKernelError::InvalidWireMessage("loaded/list 游标重复"));
        }
    }
    Err(CodexKernelError::InvalidWireMessage(
        "loaded/list 超过分页上限",
    ))
}

async fn read_thread(rpc: &impl CodexRpc, id: &str) -> Result<Value, CodexKernelError> {
    let response = rpc.request("thread/read", json!({"threadId":id})).await?;
    let thread = response
        .get("thread")
        .filter(|thread| thread.is_object())
        .ok_or(CodexKernelError::InvalidWireMessage(
            "thread/read 缺少 thread",
        ))?;
    if thread["id"].as_str() != Some(id) {
        return Err(CodexKernelError::InvalidWireMessage(
            "thread/read 返回了错误任务",
        ));
    }
    if !matches!(
        thread["status"]["type"].as_str(),
        Some("active" | "idle" | "notLoaded" | "systemError")
    ) {
        return Err(CodexKernelError::InvalidWireMessage("任务状态缺失或未知"));
    }
    match thread.get("parentThreadId") {
        Some(Value::Null) => {}
        Some(Value::String(parent)) if !parent.is_empty() => {}
        _ => return Err(CodexKernelError::InvalidWireMessage("任务缺少有效父级关系")),
    }
    Ok(thread.clone())
}

/// Resolve an active approval source's ancestry without trusting tool output or
/// resuming any thread. An inactive source cannot request new authority.
/// The caller must revalidate ownership after the asynchronous lookup.
pub async fn find_codex_ancestor(
    rpc: &impl CodexRpc,
    child: &str,
    roots: &HashSet<String>,
) -> Result<Option<String>, CodexKernelError> {
    if child.is_empty() {
        return Err(CodexKernelError::InvalidWireMessage("子任务标识不能为空"));
    }
    let mut current = child.to_owned();
    let mut seen = HashSet::new();
    for _ in 0..512 {
        if !seen.insert(current.clone()) {
            return Err(CodexKernelError::InvalidWireMessage("子任务祖先关系循环"));
        }
        let thread = read_thread(rpc, &current).await?;
        if current == child && thread["status"]["type"] != "active" {
            return Ok(None);
        }
        if roots.contains(&current) {
            return Ok(Some(current));
        }
        let Some(parent) = thread["parentThreadId"].as_str() else {
            return Ok(None);
        };
        current = parent.to_owned();
    }
    Err(CodexKernelError::InvalidWireMessage("子任务祖先关系超限"))
}

fn confirmed_idle(thread: &Value) -> Result<bool, CodexKernelError> {
    match thread["status"]["type"].as_str() {
        Some("idle" | "notLoaded") => Ok(true),
        Some("active") => Ok(false),
        _ => Err(CodexKernelError::InvalidWireMessage("无法确认任务已停止")),
    }
}

async fn confirmed_quiet(
    rpc: &impl CodexRpc,
    id: &str,
    thread: &Value,
) -> Result<bool, CodexKernelError> {
    if thread["status"]["type"] != "systemError" {
        return confirmed_idle(thread);
    }
    // Pinned app-server thread_status.rs uses systemError as an idle error flag,
    // but that notification can precede the actual turn terminal. Require both.
    let turns = rpc
        .request(
            "thread/turns/list",
            json!({"threadId":id,"limit":1,"sortDirection":"desc","itemsView":"notLoaded"}),
        )
        .await?;
    let terminal = turns["data"]
        .as_array()
        .filter(|data| data.len() == 1)
        .and_then(|data| data.first());
    if terminal.is_some_and(|turn| {
        turn["id"].as_str().is_some_and(|id| !id.is_empty())
            && matches!(
                turn["status"].as_str(),
                Some("completed" | "failed" | "interrupted")
            )
    }) {
        return Ok(true);
    }
    Err(CodexKernelError::InvalidWireMessage(
        "错误状态尚无明确结束轮次",
    ))
}
