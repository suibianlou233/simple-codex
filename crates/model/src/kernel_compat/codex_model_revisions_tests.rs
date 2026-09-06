//! Simple-owned Codex compatibility layer.
use crate::{
    ChatDialect, CodexKernelClient, CodexKernelConfig, CodexKernelEvent, CodexTurnStatus,
    ResponsesGatewayConfig,
};
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn live_native_tasks_keep_revision_and_same_thread_switches_model_without_restart_when_configured()
 {
    let Some(executable) = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER") else {
        return;
    };
    let server = MockServer::start().await;
    for (model, marker, delay) in [
        ("model-old", "OLD_REVISION", 250),
        ("model-new", "NEW_REVISION", 0),
    ] {
        let body = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{marker}\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":10,\"completion_tokens\":2}}}}\n\ndata: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({"model":model})))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_delay(Duration::from_millis(delay))
                    .set_body_string(body),
            )
            .expect(2)
            .mount(&server)
            .await;
    }
    let config = |alias: &str, model: &str| {
        ResponsesGatewayConfig::new(server.uri(), model, None)
            .with_model_alias(alias)
            .with_chat_completions(ChatDialect::Standard)
    };
    let home = std::env::temp_dir().join(format!("simple-revision-test-{}", uuid::Uuid::new_v4()));
    let (client, mut events) = CodexKernelClient::start(CodexKernelConfig::new(
        executable,
        &home,
        config("revision-old", "model-old"),
    ))
    .await
    .expect("native kernel");
    let instance = client.instance_id().to_owned();
    let mut threads = Vec::new();
    for _ in 0..2 {
        let response = client.request("thread/start", json!({"cwd":home,"approvalPolicy":"never","sandbox":"read-only","ephemeral":false,"config":{"web_search":"disabled"}})).await.expect("thread");
        threads.push(
            response["thread"]["id"]
                .as_str()
                .expect("thread id")
                .to_owned(),
        );
    }
    for round in 0..2 {
        let revisions = if round == 0 {
            ["revision-old", "revision-new"]
        } else {
            ["revision-new", "revision-old"]
        };
        let mut expected = HashMap::new();
        for (index, thread) in threads.iter().enumerate() {
            if round == 0 && index == 1 {
                let reached = tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        if !server
                            .received_requests()
                            .await
                            .expect("requests")
                            .is_empty()
                        {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                })
                .await;
                if reached.is_err() {
                    let mut observed = Vec::new();
                    while let Ok(Some(event)) =
                        tokio::time::timeout(Duration::from_millis(10), events.next()).await
                    {
                        observed.push(event);
                    }
                    client.shutdown().await.expect("shutdown failed fixture");
                    panic!("old task did not reach model: {observed:?}");
                }
                client
                    .register_model(config("revision-new", "model-new"))
                    .await
                    .expect("live registration");
            }
            let response = client.request("turn/start", json!({"threadId":thread,"model":revisions[index],"input":[{"type":"text","text":"Only reply with the configured marker.","text_elements":[]}]})).await.expect("turn");
            let id = response["turn"]["id"].as_str().expect("turn id").to_owned();
            expected.insert(
                (thread.clone(), id),
                if revisions[index] == "revision-old" {
                    "OLD_REVISION"
                } else {
                    "NEW_REVISION"
                },
            );
        }
        let mut texts = HashMap::new();
        tokio::time::timeout(Duration::from_secs(20), async {
            while !expected.is_empty() {
                let event = CodexKernelEvent::project(
                    events
                        .next()
                        .await
                        .expect("events open")
                        .expect("wire event"),
                )
                .expect("event");
                match event {
                    CodexKernelEvent::ItemCompleted {
                        thread_id,
                        turn_id,
                        item,
                        ..
                    } if item.kind == "agentMessage" => {
                        texts.insert(
                            (thread_id, turn_id),
                            item.agent_text().expect("assistant text").to_owned(),
                        );
                    }
                    CodexKernelEvent::TurnCompleted {
                        thread_id,
                        turn_id,
                        status,
                        ..
                    } => {
                        let key = (thread_id, turn_id);
                        let marker = expected.remove(&key).expect("expected turn");
                        assert_eq!(status, CodexTurnStatus::Completed);
                        assert_eq!(texts.remove(&key).as_deref(), Some(marker));
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("both tasks completed");
    }
    assert_eq!(client.instance_id(), instance);
    let history = crate::CodexSessionBridge::new(client.clone())
        .resume_and_hydrate(&threads[0])
        .await
        .expect("same thread history");
    assert_eq!(history.thread_id, threads[0]);
    assert_eq!(history.turns.len(), 2);
    client.shutdown().await.expect("shutdown fixture");
}
