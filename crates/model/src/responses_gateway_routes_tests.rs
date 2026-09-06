use super::*;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn config(server: &MockServer, alias: &str, model: &str, key: &str) -> ResponsesGatewayConfig {
    ResponsesGatewayConfig::new(
        server.uri(),
        model,
        Some(ApiKey::new(key).expect("fixture key")),
    )
    .with_model_alias(alias)
}

#[tokio::test]
async fn revisions_keep_endpoint_model_credential_and_inflight_request_isolated() {
    let old = MockServer::start().await;
    let new = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .and(header("authorization", "Bearer old-fixture"))
        .and(body_partial_json(json!({"model":"old-model"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(150))
                .set_body_string("old-result"),
        )
        .expect(2)
        .mount(&old)
        .await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .and(header("authorization", "Bearer new-fixture"))
        .and(body_partial_json(json!({"model":"new-model"})))
        .respond_with(ResponseTemplate::new(200).set_body_string("new-result"))
        .expect(1)
        .mount(&new)
        .await;
    let old_config = config(&old, "revision-old", "old-model", "old-fixture");
    let gateway = ResponsesGatewayHandle::start(old_config.clone())
        .await
        .expect("gateway");
    let request = |alias: &str| {
        Client::new()
            .post(format!("{}/responses", gateway.base_url()))
            .bearer_auth(gateway.client_token().expose_for_child())
            .json(&json!({"model":alias,"stream":true}))
    };
    let pending = tokio::spawn(request("revision-old").send());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !old
                .received_requests()
                .await
                .expect("recorded requests")
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("old request actually reached upstream");
    let new_config = config(&new, "revision-new", "new-model", "new-fixture");
    gateway
        .register_model(new_config.clone())
        .expect("new revision");
    gateway
        .register_model(new_config)
        .expect("idempotent registration");
    assert!(
        gateway
            .register_model(config(&new, "revision-old", "new-model", "new-fixture"))
            .is_err()
    );
    assert_eq!(
        request("revision-new")
            .send()
            .await
            .expect("new response")
            .text()
            .await
            .expect("body"),
        "new-result"
    );
    assert_eq!(
        pending
            .await
            .expect("request task")
            .expect("old response")
            .text()
            .await
            .expect("body"),
        "old-result"
    );
    assert_eq!(
        request("revision-old")
            .send()
            .await
            .expect("old still usable")
            .text()
            .await
            .expect("body"),
        "old-result"
    );
    assert_eq!(
        request("unknown")
            .send()
            .await
            .expect("unknown response")
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(!format!("{gateway:?}").contains("fixture"));
    gateway.shutdown().await.expect("shutdown");
}

#[test]
fn route_limit_preserves_existing_revisions_and_rejects_invalid_registration() {
    let config = ResponsesGatewayConfig::new("http://127.0.0.1:1", "fixture", None);
    let routes = routes::Routes::new(config.clone()).expect("routes");
    for index in 1..256 {
        routes
            .register(config.clone().with_model_alias(format!("revision-{index}")))
            .expect("within limit");
    }
    routes
        .register(config.clone())
        .expect("existing route at limit");
    assert!(
        routes
            .register(config.clone().with_model_alias("overflow"))
            .is_err()
    );
    assert!(routes.get("fixture").expect("lookup").is_some());
    let mut invalid = config.clone().with_model_alias("revision-1");
    invalid.max_request_bytes += 1;
    assert!(routes.register(invalid).is_err());
    assert!(routes.register(config.with_model_alias(" ")).is_err());
}
