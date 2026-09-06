//! Sequential package switching with separate, disposable native histories.
//! No desktop, production database, external model endpoint or credentials are used.
use local_agent_model::{
    CodexKernelClient, CodexKernelWireMessage, KernelPackage, KernelSelection,
    ResponsesGatewayConfig,
};
use serde_json::json;

#[tokio::test]
#[ignore = "requires SIMPLE_TEST_KERNEL_MANIFEST and SIMPLE_TEST_SECOND_KERNEL_MANIFEST"]
async fn two_version_packages_can_switch_back_without_sharing_data()
-> Result<(), Box<dyn std::error::Error>> {
    let first = std::env::var_os("SIMPLE_TEST_KERNEL_MANIFEST").ok_or("first package required")?;
    let second =
        std::env::var_os("SIMPLE_TEST_SECOND_KERNEL_MANIFEST").ok_or("second package required")?;
    let packages = [
        KernelPackage::load(std::path::Path::new(&first))?,
        KernelPackage::load(std::path::Path::new(&second))?,
    ];
    assert_ne!(packages[0].id(), packages[1].id());
    let root = tempfile::tempdir()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        axum::serve(listener, axum::Router::new().route("/responses", axum::routing::post(|| async {
            let id = uuid::Uuid::new_v4().to_string();
            let item = json!({"type":"message","id":format!("msg_{id}"),"role":"assistant","status":"completed","content":[{"type":"output_text","text":"ISOLATED_HISTORY_FIXTURE","annotations":[]}]});
            let events = [
                json!({"type":"response.created","response":{"id":id,"status":"in_progress","output":[]}}),
                json!({"type":"response.output_item.done","output_index":0,"item":item}),
                json!({"type":"response.completed","response":{"id":id,"status":"completed","output":[item]}}),
            ];
            ([("content-type","text/event-stream")], events.iter().map(|event| format!("data: {event}\n\n")).collect::<String>())
        }))).await
    });
    let project = root.path().join("独立 项目");
    std::fs::create_dir(&project)?;
    let mut ids = [String::new(), String::new()];
    for index in [0, 1, 0, 1] {
        let home = root.path().join(format!("slot-{index}"));
        let config = KernelSelection::Package(packages[index].clone()).configuration(
            home,
            ResponsesGatewayConfig::new(&endpoint, "switch-fixture", None),
        );
        let (client, mut events) = CodexKernelClient::start(config).await?;
        println!("switch slot {index}: {}", packages[index].id());
        let result = if ids[index].is_empty() {
            client.request("thread/start", json!({"cwd":project,"sandbox":"read-only","approvalPolicy":"on-request","ephemeral":false})).await
        } else {
            client
                .request(
                    "thread/resume",
                    json!({"threadId":ids[index],"cwd":project}),
                )
                .await
        };
        let response = result?;
        let observed = response["thread"]["id"]
            .as_str()
            .ok_or("thread id missing")?;
        if ids[index].is_empty() {
            client.request("turn/start", json!({"threadId":observed,"input":[{"type":"text","text":"Reply with the fixture marker only.","text_elements":[]}]})).await?;
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                while let Some(event) = events.next().await {
                    if let CodexKernelWireMessage::Notification { method, params } = event? {
                        if method == "turn/completed" {
                            assert_eq!(params["turn"]["status"], "completed");
                            return Ok::<(), local_agent_model::CodexKernelError>(());
                        }
                    }
                }
                Err(local_agent_model::CodexKernelError::Unavailable)
            })
            .await??;
        }
        let turns = client
            .request("thread/turns/list", json!({"threadId":observed,"limit":10}))
            .await?;
        assert!(
            !turns["data"]
                .as_array()
                .ok_or("missing saved turns")?
                .is_empty()
        );
        let other_id = &ids[1 - index];
        if !other_id.is_empty() {
            assert!(
                client
                    .request("thread/resume", json!({"threadId":other_id,"cwd":project}))
                    .await
                    .is_err(),
                "other slot's thread must not resolve"
            );
        }
        client.shutdown().await?;
        if ids[index].is_empty() {
            ids[index] = observed.into();
        } else {
            assert_eq!(ids[index], observed);
        }
    }
    server.abort();
    Ok(())
}
