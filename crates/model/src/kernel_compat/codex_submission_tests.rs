//! Simple-owned Codex compatibility layer.
use super::*;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Mutex;

struct Rpc {
    pages: Mutex<VecDeque<Value>>,
    calls: Mutex<Vec<String>>,
    allow_start: bool,
}
impl Rpc {
    fn new(pages: Vec<Value>) -> Self {
        Self {
            pages: Mutex::new(pages.into()),
            calls: Mutex::new(Vec::new()),
            allow_start: false,
        }
    }
}
#[async_trait]
impl CodexRpc for Rpc {
    async fn request(&self, method: &str, _params: Value) -> Result<Value, CodexKernelError> {
        assert!(
            matches!(method, "thread/read" | "thread/items/list")
                || self.allow_start && method == "turn/start",
            "unexpected mutation"
        );
        self.calls.lock().expect("calls").push(method.into());
        let value = self
            .pages
            .lock()
            .expect("pages")
            .pop_front()
            .ok_or(CodexKernelError::Unavailable)?;
        if value == json!("lost-reply") {
            Err(CodexKernelError::Unavailable)
        } else if let Some(error) = value.get("fixtureRpcError") {
            Err(CodexKernelError::Rpc(error.clone()))
        } else {
            Ok(value)
        }
    }
}
fn item(id: &str, turn: &str, client: &str) -> Value {
    json!({"turnId":turn,"item":{"id":id,"type":"userMessage","clientId":client,"content":[]}})
}

#[tokio::test]
async fn successful_start_does_not_scan_history() {
    let mut rpc = Rpc::new(vec![json!({"turn":{"id":"native","status":"inProgress"}})]);
    rpc.allow_start = true;
    let receipt = start_codex_turn_recovering(
        &rpc,
        "thread",
        "wanted",
        json!({"threadId":"thread","clientUserMessageId":"wanted"}),
    )
    .await
    .expect("start");
    assert!(!receipt.recovered);
    assert_eq!(receipt.turn["id"], "native");
    assert_eq!(*rpc.calls.lock().expect("calls"), vec!["turn/start"]);
}

#[tokio::test]
async fn warming_history_is_rechecked_without_resubmission() {
    let mut rpc = Rpc::new(vec![
        json!("lost-reply"),
        json!({"thread":{"id":"thread"}}),
        json!({"fixtureRpcError":{"code":-32601,"message":"not yet queryable"}}),
        json!({"thread":{"id":"thread"}}),
        json!({"data":[item("user","native","wanted")],"nextCursor":null}),
    ]);
    rpc.allow_start = true;
    let receipt = start_codex_turn_recovering(
        &rpc,
        "thread",
        "wanted",
        json!({"threadId":"thread","clientUserMessageId":"wanted"}),
    )
    .await
    .expect("recovered");
    assert!(receipt.recovered);
    assert_eq!(receipt.turn["id"], "native");
    assert_eq!(
        rpc.calls
            .lock()
            .expect("calls")
            .iter()
            .filter(|method| method.as_str() == "turn/start")
            .count(),
        1
    );
}

#[tokio::test]
async fn lost_or_malformed_reply_is_recovered_without_a_second_start() {
    for first in [json!("lost-reply"), json!({"turn":{}})] {
        let mut rpc = Rpc::new(vec![
            first,
            json!({"thread":{"id":"thread"}}),
            json!({"data":[],"nextCursor":null}),
            json!({"thread":{"id":"thread"}}),
            json!({"data":[item("user","native","wanted")],"nextCursor":null}),
        ]);
        rpc.allow_start = true;
        let receipt = start_codex_turn_recovering(
            &rpc,
            "thread",
            "wanted",
            json!({"threadId":"thread","clientUserMessageId":"wanted"}),
        )
        .await
        .expect("recovered");
        assert!(receipt.recovered);
        assert_eq!(receipt.turn, json!({"id":"native"}));
        assert_eq!(
            rpc.calls
                .lock()
                .expect("calls")
                .iter()
                .filter(|method| method.as_str() == "turn/start")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn failed_reconciliation_never_resubmits_and_mismatched_input_never_submits() {
    let mut rpc = Rpc::new(vec![json!("lost-reply"), json!({"thread":{"id":"other"}})]);
    rpc.allow_start = true;
    assert!(
        start_codex_turn_recovering(
            &rpc,
            "thread",
            "wanted",
            json!({"threadId":"thread","clientUserMessageId":"wanted"})
        )
        .await
        .is_err()
    );
    assert_eq!(
        rpc.calls
            .lock()
            .expect("calls")
            .iter()
            .filter(|method| method.as_str() == "turn/start")
            .count(),
        1
    );
    let rpc = Rpc::new(vec![]);
    assert!(
        start_codex_turn_recovering(
            &rpc,
            "thread",
            "wanted",
            json!({"threadId":"wrong","clientUserMessageId":"wanted"})
        )
        .await
        .is_err()
    );
    assert!(rpc.calls.lock().expect("calls").is_empty());
}

#[derive(Clone, Copy)]
enum UnresolvedHistory {
    Empty,
    Unsupported,
    Stalled,
}

struct UnresolvedRpc {
    mode: UnresolvedHistory,
    starts: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl CodexRpc for UnresolvedRpc {
    async fn request(&self, method: &str, _params: Value) -> Result<Value, CodexKernelError> {
        match method {
            "turn/start" => {
                self.starts
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(CodexKernelError::Unavailable)
            }
            "thread/read" => match self.mode {
                UnresolvedHistory::Stalled => std::future::pending().await,
                _ => Ok(json!({"thread":{"id":"thread"}})),
            },
            "thread/items/list" => match self.mode {
                UnresolvedHistory::Empty => Ok(json!({"data":[],"nextCursor":null})),
                UnresolvedHistory::Unsupported => {
                    Err(CodexKernelError::Rpc(json!({"code":-32601})))
                }
                UnresolvedHistory::Stalled => panic!("stalled read must not reach listing"),
            },
            _ => panic!("recovery must not mutate native state: {method}"),
        }
    }
}

#[tokio::test]
async fn unobservable_history_has_a_real_deadline_and_never_resubmits() {
    async fn check(mode: UnresolvedHistory) {
        let rpc = UnresolvedRpc {
            mode,
            starts: std::sync::atomic::AtomicUsize::new(0),
        };
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            start_codex_turn_recovering(
                &rpc,
                "thread",
                "wanted",
                json!({"threadId":"thread","clientUserMessageId":"wanted"}),
            ),
        )
        .await
        .expect("the production recovery deadline must end even a stalled RPC");
        assert!(
            result.is_err(),
            "absence cannot become a successful submission receipt"
        );
        assert_eq!(rpc.starts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    tokio::join!(
        check(UnresolvedHistory::Empty),
        check(UnresolvedHistory::Unsupported),
        check(UnresolvedHistory::Stalled)
    );
}

#[tokio::test]
async fn ambiguous_history_after_lost_reply_never_produces_a_receipt() {
    let mut rpc = Rpc::new(vec![
        json!("lost-reply"),
        json!({"thread":{"id":"thread"}}),
        json!({"data":[item("first","native","wanted")],"nextCursor":"next"}),
        json!({"data":[item("second","other-native","wanted")],"nextCursor":null}),
    ]);
    rpc.allow_start = true;
    assert!(
        start_codex_turn_recovering(
            &rpc,
            "thread",
            "wanted",
            json!({"threadId":"thread","clientUserMessageId":"wanted"})
        )
        .await
        .is_err()
    );
    assert_eq!(
        rpc.calls
            .lock()
            .expect("calls")
            .iter()
            .filter(|method| method.as_str() == "turn/start")
            .count(),
        1
    );
}

#[tokio::test]
async fn searches_all_pages_and_uses_native_client_id_not_model_text() {
    let rpc = Rpc::new(vec![
        json!({"thread":{"id":"thread"}}),
        json!({"data":[{"turnId":"wrong","item":{"id":"assistant","type":"agentMessage","clientId":"wanted","text":"wanted"}}],"nextCursor":"next"}),
        json!({"data":[item("user","right","wanted")],"nextCursor":null}),
    ]);
    assert_eq!(
        find_codex_submission(&rpc, "thread", "wanted")
            .await
            .expect("lookup"),
        Some("right".into())
    );
    assert_eq!(rpc.calls.lock().expect("calls").len(), 3);
}

#[tokio::test]
async fn absence_is_only_not_observed_and_never_triggers_resubmission() {
    let rpc = Rpc::new(vec![
        json!({"thread":{"id":"thread"}}),
        json!({"data":[],"nextCursor":null}),
    ]);
    assert_eq!(
        find_codex_submission(&rpc, "thread", "wanted")
            .await
            .expect("lookup"),
        None
    );
    assert_eq!(rpc.calls.lock().expect("calls").len(), 2);
}

#[tokio::test]
async fn duplicate_client_ids_are_ambiguous_even_within_the_same_turn() {
    for second_turn in ["first", "second"] {
        let rpc = Rpc::new(vec![
            json!({"thread":{"id":"thread"}}),
            json!({"data":[item("u1","first","wanted")],"nextCursor":"next"}),
            json!({"data":[item("u2",second_turn,"wanted")],"nextCursor":null}),
        ]);
        assert!(
            find_codex_submission(&rpc, "thread", "wanted")
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn malformed_identity_or_pagination_never_becomes_a_unique_match() {
    for page in [
        json!({"data":[item("user","turn","wanted")]}),
        json!({"data":[],"nextCursor":7}),
        json!({"data":[],"nextCursor":""}),
        json!({"data":[{"turnId":"","item":{"id":"user","type":"userMessage","clientId":"wanted"}}],"nextCursor":null}),
        json!({"data":[{"turnId":"turn","item":{"id":"user","type":"userMessage","clientId":7}}],"nextCursor":null}),
        json!({"data":[item("dup","one","wanted"),item("dup","two","other")],"nextCursor":null}),
        json!({"data":[item("user","turn","wanted")],"nextCursor":"again"}),
    ] {
        let rpc = Rpc::new(vec![
            json!({"thread":{"id":"thread"}}),
            page,
            json!({"data":[],"nextCursor":"again"}),
        ]);
        assert!(
            find_codex_submission(&rpc, "thread", "wanted")
                .await
                .is_err()
        );
    }
    let wrong = Rpc::new(vec![json!({"thread":{"id":"other"}})]);
    assert!(
        find_codex_submission(&wrong, "thread", "wanted")
            .await
            .is_err()
    );
    let unavailable = Rpc::new(vec![]);
    assert!(
        find_codex_submission(&unavailable, "thread", "wanted")
            .await
            .is_err()
    );
    assert!(
        find_codex_submission(&unavailable, "", "wanted")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn page_limits_fail_closed() {
    let mut pages = vec![json!({"thread":{"id":"thread"}})];
    pages.extend(
        (0..MAX_PAGES).map(|index| json!({"data":[],"nextCursor":format!("page-{index}")})),
    );
    assert!(
        find_codex_submission(&Rpc::new(pages), "thread", "wanted")
            .await
            .is_err()
    );
    let oversized = Rpc::new(vec![
        json!({"thread":{"id":"thread"}}),
        json!({"data":vec![item("user","turn","wanted");PAGE_SIZE+1],"nextCursor":null}),
    ]);
    assert!(
        find_codex_submission(&oversized, "thread", "wanted")
            .await
            .is_err()
    );
}
