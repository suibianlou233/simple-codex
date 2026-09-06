//! Simple-owned Codex compatibility layer.
use super::*;
use async_trait::async_trait;
use std::sync::Mutex;

struct FakeRpc {
    active: Mutex<HashSet<String>>,
    stopped: Mutex<Vec<String>>,
    bad_page: bool,
    root_finished: bool,
}

struct OutcomeRpc {
    status: &'static str,
}
#[async_trait]
impl CodexRpc for OutcomeRpc {
    async fn request(&self, method: &str, params: Value) -> Result<Value, CodexKernelError> {
        let id = params["threadId"].as_str().unwrap_or_default();
        match method {
            "thread/loaded/list" => Ok(json!({"data":["root"],"nextCursor":null})),
            "thread/read" => Ok(
                json!({"thread":{"id":id,"status":{"type":"notLoaded"},"parentThreadId":if id == "child" {Some("root")} else {None}}}),
            ),
            "thread/turns/list" if self.status == "unavailable" => {
                Err(CodexKernelError::Unavailable)
            }
            "thread/turns/list" => Ok(json!({"data":[{"id":"child-turn","status":self.status}]})),
            _ => Err(CodexKernelError::Unavailable),
        }
    }
}

#[tokio::test]
async fn seeded_unloaded_children_retain_results_and_unknown_is_never_success()
-> Result<(), CodexKernelError> {
    for (status, expected) in [
        ("completed", CodexChildStatus::Completed),
        ("failed", CodexChildStatus::Failed),
        ("interrupted", CodexChildStatus::Cancelled),
        ("inProgress", CodexChildStatus::Unknown),
        ("newStatus", CodexChildStatus::Unknown),
        ("unavailable", CodexChildStatus::Unknown),
    ] {
        let rpc = OutcomeRpc { status };
        let mut watch = CodexTaskTreeWatch::new("root")?;
        watch.include_spawned(["child".into()]);
        assert!(!watch.is_quiet(&rpc).await?);
        assert!(watch.is_quiet(&rpc).await?);
        let outcomes = watch.child_outcomes(&rpc).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, expected);
        watch.include_spawned(["unrelated".into()]);
        assert!(watch.is_quiet(&rpc).await.is_err());
    }
    Ok(())
}

#[tokio::test]
async fn ancestry_uses_validated_native_metadata_and_rejects_unrelated_or_malformed_chains()
-> Result<(), CodexKernelError> {
    let roots = HashSet::from(["root".to_owned()]);
    let rpc = AnomalyRpc::new("unloaded_parent");
    assert_eq!(
        find_codex_ancestor(&rpc, "grandchild", &roots).await?,
        Some("root".into())
    );
    assert_eq!(find_codex_ancestor(&rpc, "unrelated", &roots).await?, None);
    assert!(rpc.inner.stopped.lock().expect("stops").is_empty());
    for anomaly in [
        "missing_parent",
        "invalid_parent",
        "cyclic_parent",
        "wrong_identity",
        "unknown_status",
    ] {
        assert!(
            find_codex_ancestor(&AnomalyRpc::new(anomaly), "grandchild", &roots)
                .await
                .is_err(),
            "{anomaly}"
        );
    }
    rpc.inner
        .active
        .lock()
        .expect("active")
        .remove("grandchild");
    assert_eq!(find_codex_ancestor(&rpc, "grandchild", &roots).await?, None);
    Ok(())
}

#[async_trait]
impl CodexRpc for FakeRpc {
    async fn request(&self, method: &str, params: Value) -> Result<Value, CodexKernelError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| CodexKernelError::Unavailable)?;
        let id = params["threadId"].as_str().unwrap_or_default();
        match method {
            "turn/interrupt" => {
                if id == "root" && self.root_finished {
                    return Err(CodexKernelError::Unavailable);
                }
                active.remove(id);
                self.stopped
                    .lock()
                    .map_err(|_| CodexKernelError::Unavailable)?
                    .push(id.to_owned());
                Ok(json!({}))
            }
            "thread/loaded/list" if self.bad_page => Ok(json!({"data":null})),
            "thread/loaded/list" => {
                if params["cursor"].is_null() {
                    Ok(json!({"data":["root","child"],"nextCursor":"page2"}))
                } else {
                    Ok(json!({"data":["grandchild","unrelated"],"nextCursor":null}))
                }
            }
            "thread/read" => Ok(json!({"thread":{
                "id":id,"status":{"type":if active.contains(id) {"active"} else {"idle"}},
                "parentThreadId":match id {"child"=>Some("root"),"grandchild"=>Some("child"),_=>None}
            }})),
            "thread/turns/list" => {
                Ok(json!({"data":[{"id":format!("turn-{id}"),"status":"inProgress"}]}))
            }
            _ => Err(CodexKernelError::Unavailable),
        }
    }
}

#[tokio::test]
async fn stops_paginated_descendants_but_preserves_unrelated_work() -> Result<(), CodexKernelError>
{
    let rpc = FakeRpc {
        active: Mutex::new(
            ["root", "child", "grandchild", "unrelated"]
                .map(str::to_owned)
                .into(),
        ),
        stopped: Mutex::new(Vec::new()),
        bad_page: false,
        root_finished: false,
    };
    stop_codex_task_tree(&rpc, "root", "turn-root").await?;
    assert_eq!(
        *rpc.active.lock().expect("active lock"),
        HashSet::from(["unrelated".to_owned()])
    );
    let mut stopped = rpc.stopped.lock().expect("stopped lock").clone();
    stopped.sort();
    assert_eq!(stopped, vec!["child", "grandchild", "root"]);
    Ok(())
}

#[tokio::test]
async fn malformed_descendant_listing_is_not_reported_as_success() {
    let rpc = FakeRpc {
        active: Mutex::new(HashSet::new()),
        stopped: Mutex::new(Vec::new()),
        bad_page: true,
        root_finished: false,
    };
    assert!(
        stop_codex_task_tree(&rpc, "root", "turn-root")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn finished_root_does_not_leave_active_descendants() -> Result<(), CodexKernelError> {
    let rpc = FakeRpc {
        active: Mutex::new(HashSet::from(["child".to_owned(), "unrelated".to_owned()])),
        stopped: Mutex::new(Vec::new()),
        bad_page: false,
        root_finished: true,
    };
    stop_codex_task_tree(&rpc, "root", "turn-root").await?;
    assert_eq!(
        *rpc.active.lock().expect("active lock"),
        HashSet::from(["unrelated".to_owned()])
    );
    Ok(())
}

#[tokio::test]
async fn passive_watch_never_stops_work_and_errors_invalidate_quiet_evidence()
-> Result<(), CodexKernelError> {
    let rpc = AnomalyRpc::new("unloaded_parent");
    rpc.inner.active.lock().expect("active").remove("child");
    let mut watch = CodexTaskTreeWatch::new("root")?;
    assert!(!watch.is_quiet(&rpc).await?);
    assert!(
        rpc.inner
            .active
            .lock()
            .expect("active")
            .contains("grandchild")
    );
    rpc.inner
        .active
        .lock()
        .expect("active")
        .remove("grandchild");
    assert!(!watch.is_quiet(&rpc).await?);
    assert!(
        watch
            .is_quiet(&AnomalyRpc::new("missing_status"))
            .await
            .is_err()
    );
    assert!(!watch.is_quiet(&rpc).await?);
    assert!(watch.is_quiet(&rpc).await?);
    assert!(rpc.inner.stopped.lock().expect("stopped").is_empty());
    assert!(
        rpc.inner
            .active
            .lock()
            .expect("active")
            .contains("unrelated")
    );
    Ok(())
}

struct AnomalyRpc {
    inner: FakeRpc,
    anomaly: &'static str,
    child_reads: std::sync::atomic::AtomicUsize,
}

impl AnomalyRpc {
    fn new(anomaly: &'static str) -> Self {
        Self {
            inner: FakeRpc {
                active: Mutex::new(HashSet::from([
                    "child".into(),
                    "grandchild".into(),
                    "unrelated".into(),
                ])),
                stopped: Mutex::new(Vec::new()),
                bad_page: false,
                root_finished: true,
            },
            anomaly,
            child_reads: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl CodexRpc for AnomalyRpc {
    async fn request(&self, method: &str, params: Value) -> Result<Value, CodexKernelError> {
        let child = params["threadId"] == "child";
        let first_page = params["cursor"].is_null();
        let mut response = self.inner.request(method, params).await?;
        if method == "thread/loaded/list" {
            match self.anomaly {
                "missing_cursor" => {
                    response
                        .as_object_mut()
                        .expect("fixture page")
                        .remove("nextCursor");
                }
                "invalid_cursor" => response["nextCursor"] = json!(12),
                "unloaded_parent" => {
                    response["data"] = if first_page {
                        json!(["root"])
                    } else {
                        json!(["grandchild", "unrelated"])
                    }
                }
                _ => {}
            }
        }
        if method == "thread/read" && child {
            use std::sync::atomic::Ordering;
            let reads = self.child_reads.fetch_add(1, Ordering::SeqCst) + 1;
            match self.anomaly {
                "missing_status" => response["thread"]["status"] = Value::Null,
                "unknown_status" => response["thread"]["status"]["type"] = json!("newUnknownState"),
                "system_error" => response["thread"]["status"]["type"] = json!("systemError"),
                "wrong_identity" => response["thread"]["id"] = json!("another-thread"),
                "missing_parent" => {
                    response["thread"]
                        .as_object_mut()
                        .expect("thread")
                        .remove("parentThreadId");
                }
                "invalid_parent" => response["thread"]["parentThreadId"] = json!(123),
                "cyclic_parent" => response["thread"]["parentThreadId"] = json!("grandchild"),
                "unloaded_parent" => response["thread"]["status"]["type"] = json!("notLoaded"),
                "lagging_turn_list" if reads >= 6 => {
                    response["thread"]["status"]["type"] = json!("idle")
                }
                _ => {}
            }
        }
        if method == "thread/turns/list" && child {
            match self.anomaly {
                "empty_turns" => response["data"] = json!([]),
                "missing_turns" => response["data"] = Value::Null,
                "unknown_turn" => response["data"][0]["status"] = json!("unknown"),
                "empty_turn_id" => response["data"][0]["id"] = json!(""),
                "lagging_turn_list" => response["data"][0]["status"] = json!("completed"),
                _ => {}
            }
        }
        Ok(response)
    }
}

#[tokio::test]
async fn incomplete_or_contradictory_tree_facts_never_certify_a_successful_stop() {
    for anomaly in [
        "missing_cursor",
        "invalid_cursor",
        "missing_status",
        "unknown_status",
        "system_error",
        "wrong_identity",
        "missing_parent",
        "invalid_parent",
        "cyclic_parent",
        "empty_turns",
        "missing_turns",
        "unknown_turn",
        "empty_turn_id",
    ] {
        let rpc = AnomalyRpc::new(anomaly);
        assert!(
            stop_codex_task_tree(&rpc, "root", "turn-root")
                .await
                .is_err(),
            "false successful stop for {anomaly}"
        );
        assert!(
            rpc.inner
                .active
                .lock()
                .expect("active")
                .contains("unrelated")
        );
    }
}

#[tokio::test]
async fn unloaded_middle_parent_does_not_hide_a_live_grandchild() -> Result<(), CodexKernelError> {
    let rpc = AnomalyRpc::new("unloaded_parent");
    rpc.inner.active.lock().expect("active").remove("child");
    stop_codex_task_tree(&rpc, "root", "turn-root").await?;
    assert_eq!(
        *rpc.inner.active.lock().expect("active"),
        HashSet::from(["unrelated".to_owned()])
    );
    assert_eq!(
        *rpc.inner.stopped.lock().expect("stopped"),
        vec!["grandchild"]
    );
    assert!(rpc.child_reads.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    Ok(())
}

#[tokio::test]
async fn completed_turn_list_is_not_quiet_until_active_thread_status_settles()
-> Result<(), CodexKernelError> {
    let rpc = AnomalyRpc::new("lagging_turn_list");
    stop_codex_task_tree(&rpc, "root", "turn-root").await?;
    assert!(
        rpc.child_reads.load(std::sync::atomic::Ordering::SeqCst) >= 7,
        "must wait for actual idle observations"
    );
    assert!(
        !rpc.inner
            .stopped
            .lock()
            .expect("stopped")
            .contains(&"child".to_owned()),
        "must not interrupt a stale completed turn"
    );
    assert!(
        rpc.inner
            .active
            .lock()
            .expect("active")
            .contains("unrelated")
    );
    Ok(())
}
