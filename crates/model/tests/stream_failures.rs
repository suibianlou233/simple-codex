//! Real loopback HTTP/SSE failures, never a paid model endpoint.
use std::io;
use std::time::Duration;

use axum::{
    Router,
    body::{Body, Bytes},
    http::header::CONTENT_TYPE,
    routing::post,
};
use futures_util::{StreamExt, stream};
use local_agent_model::{ChatDialect, ResponsesGatewayConfig, ResponsesGatewayHandle};
use serde_json::{Value, json};
use tokio::net::TcpListener;

async fn hanging_stream() -> ([(axum::http::HeaderName, &'static str); 1], Body) {
    let first = stream::once(async { Ok::<_, io::Error>(Bytes::from_static(b": connected\n\n")) });
    (
        [(CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(first.chain(stream::pending::<Result<Bytes, io::Error>>())),
    )
}

async fn broken_encoding() -> ([(axum::http::HeaderName, &'static str); 1], Body) {
    (
        [(CONTENT_TYPE, "text/event-stream")],
        Body::from(vec![0xff, 0xfe]),
    )
}

async fn check_failure(chat: bool, invalid: bool) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test HTTP");
    let address = listener.local_addr().expect("test address");
    let endpoint = if chat {
        "/chat/completions"
    } else {
        "/responses"
    };
    let router = if invalid {
        Router::new().route(endpoint, post(broken_encoding))
    } else {
        Router::new().route(endpoint, post(hanging_stream))
    };
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let config = ResponsesGatewayConfig::new(format!("http://{address}"), "test-model", None)
        .with_model_alias("alias")
        .with_timeout(Duration::from_millis(250));
    let config = if chat {
        config.with_chat_completions(ChatDialect::Standard)
    } else {
        config.with_deepseek_responses()
    };
    let gateway = ResponsesGatewayHandle::start(config)
        .await
        .expect("start actual gateway");
    let response = reqwest::Client::new()
        .post(format!("{}/responses", gateway.base_url()))
        .timeout(Duration::from_secs(5))
        .bearer_auth(gateway.client_token().expose_for_child())
        .json(&json!({"model":"alias", "stream":true, "input":[{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"fixture only"}]}], "tools":[]}))
        .send().await.expect("gateway headers");
    assert!(
        response.status().is_success(),
        "headers must arrive before the stream error"
    );
    let body = response
        .text()
        .await
        .expect("structured SSE failure, not a broken local HTTP body");
    let events: Vec<Value> = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str(data).expect("valid SSE JSON"))
        .collect();
    let failures: Vec<_> = events
        .iter()
        .filter(|event| event["type"] == "response.failed")
        .collect();
    assert_eq!(failures.len(), 1);
    let error = &failures[0]["response"]["error"];
    assert_eq!(
        error["code"],
        if invalid {
            "simple_upstream_stream_invalid"
        } else {
            "simple_upstream_stream_timeout"
        }
    );
    assert!(
        error["message"]
            .as_str()
            .expect("message")
            .contains(if invalid {
                "无法解析"
            } else {
                "模型响应超时"
            })
    );
    assert!(
        !body.contains(&address.to_string()),
        "do not leak request URL"
    );
    assert!(
        !events
            .iter()
            .any(|event| event["type"] == "response.completed"),
        "failure cannot be reported as success"
    );
    gateway.shutdown().await.expect("gateway cleanup");
    server.abort();
}

#[tokio::test]
async fn native_responses_stream_timeout_is_distinct() {
    check_failure(false, false).await;
}

#[tokio::test]
async fn chat_stream_timeout_is_distinct() {
    check_failure(true, false).await;
}

#[tokio::test]
async fn invalid_native_stream_is_not_mislabeled_as_timeout() {
    check_failure(false, true).await;
}

#[tokio::test]
async fn invalid_chat_stream_is_not_mislabeled_as_timeout() {
    check_failure(true, true).await;
}
