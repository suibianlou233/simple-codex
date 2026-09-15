use local_agent_model::{CodexKernelClient, KernelPackage, KernelSelection, ResponsesGatewayConfig};
use serde_json::json;
use std::time::Instant;

#[tokio::test]
#[ignore = "isolated pinned-kernel measurement, no remote model"]
async fn measure_delete_stages() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var_os("SIMPLE_TEST_KERNEL_MANIFEST").ok_or("manifest required")?;
    let begin = Instant::now();
    let package = KernelPackage::load(std::path::Path::new(&path))?;
    eprintln!("package_load_ms={}", begin.elapsed().as_millis());
    let root = tempfile::tempdir()?;
    let begin = Instant::now();
    let config = KernelSelection::Package(package).configuration(root.path().join("home"),
        ResponsesGatewayConfig::new("http://127.0.0.1:9/v1", "fixture", None));
    let (client, _events) = CodexKernelClient::start(config).await?;
    eprintln!("kernel_start_ms={}", begin.elapsed().as_millis());
    let started = client.request("thread/start", json!({"cwd":root.path(),"ephemeral":false,"sandbox":"read-only","approvalPolicy":"never"})).await?;
    let id = started["thread"]["id"].as_str().ok_or("thread missing")?;
    let begin = Instant::now();
    client.request("thread/delete", json!({"threadId":id})).await?;
    eprintln!("native_delete_ms={}", begin.elapsed().as_millis());
    client.shutdown().await?;
    Ok(())
}
