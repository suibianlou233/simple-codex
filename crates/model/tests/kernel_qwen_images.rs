//! Real packaged kernel + Simple gateway + local mock provider, no paid API calls.
use local_agent_model::{ChatDialect,CodexKernelClient,CodexKernelEvent,CodexTurnStatus,KernelPackage,KernelSelection,ResponsesGatewayConfig};
use serde_json::{json,Value};
use std::time::Duration;
use wiremock::{Mock,MockServer,ResponseTemplate,matchers::{method,path}};

#[tokio::test]
#[ignore = "requires SIMPLE_TEST_KERNEL_MANIFEST; isolated native image test"]
async fn qwen_receives_native_images_and_keeps_them_in_followup_history() -> Result<(),Box<dyn std::error::Error>> {
    let manifest=std::env::var_os("SIMPLE_TEST_KERNEL_MANIFEST").ok_or("manifest required")?;
    let package=KernelPackage::load(std::path::Path::new(&manifest))?;
    let server=MockServer::start().await;
    Mock::given(method("POST")).and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).insert_header("content-type","text/event-stream")
            .set_body_string("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"IMAGE_OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
        .expect(2).mount(&server).await;
    let home=tempfile::tempdir()?;
    let config=KernelSelection::Package(package).configuration(home.path().join("home"),
        ResponsesGatewayConfig::new(server.uri(),"qwen3.8-max",None).with_chat_completions(ChatDialect::Qwen));
    let (client,mut events)=CodexKernelClient::start(config).await?;
    let thread=client.request("thread/start",json!({"cwd":home.path(),"sandbox":"read-only","approvalPolicy":"never"})).await?;
    let id=thread["thread"]["id"].as_str().ok_or("missing thread")?;
    let picture="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAANUlEQVR4nO3QsQ0AMAzDsLT//9yeoCkbeYAN6LzZdZf3x0GSKEmUJEoSJYmSREmiJFGSaMoHo8QBPwYSAhsAAAAASUVORK5CYII=";
    for input in [json!([{"type":"text","text":"Describe the attached image","text_elements":[]},{"type":"image","url":picture}]),
        json!([{"type":"text","text":"Refer to the same image again","text_elements":[]}])] {
        let started=client.request("turn/start",json!({"threadId":id,"input":input})).await?;
        let turn_id=started["turn"]["id"].as_str().ok_or("missing turn")?.to_owned();
        tokio::time::timeout(Duration::from_secs(30),async {
            while let Some(event)=events.next().await {
                let event=CodexKernelEvent::project(event.unwrap()).unwrap();
                if let CodexKernelEvent::TurnCompleted {turn_id:id,status,..}=event {
                    if id==turn_id {assert_eq!(status,CodexTurnStatus::Completed);return;}
                }
            }
            panic!("kernel stream ended");
        }).await?;
    }
    for request in server.received_requests().await.unwrap() {
        let body:Value=serde_json::from_slice(&request.body)?;
        assert_eq!(body["model"],"qwen3.8-max");
        let messages=body["messages"].as_array().unwrap();
        assert!(!messages.iter().any(|m|m["role"]=="developer"));
        assert!(messages.iter().any(|m|m["content"].as_array().is_some_and(|parts|parts.iter().any(|p|
            p["type"]=="image_url" && p["image_url"]["url"].as_str().is_some_and(|url|url.starts_with("data:image/"))))),"image missing from native request: {:?}",messages.iter().filter(|m|m["role"]=="user").map(|m|m.to_string().chars().take(500).collect::<String>()).collect::<Vec<_>>());
    }
    server.verify().await;
    client.shutdown().await?;
    Ok(())
}
