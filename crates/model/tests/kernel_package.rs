//! Opt-in acceptance of the real imported package, using isolated local data.
use local_agent_model::{
    CodexKernelClient, CodexProjectMemory, KernelPackage, KernelSelection, ResponsesGatewayConfig,
};
use serde_json::json;

#[tokio::test]
#[ignore = "requires SIMPLE_TEST_KERNEL_MANIFEST; launches only the selected local kernel"]
async fn packaged_kernel_initializes_and_opens_isolated_project()
-> Result<(), Box<dyn std::error::Error>> {
    let path =
        std::env::var_os("SIMPLE_TEST_KERNEL_MANIFEST").ok_or("set SIMPLE_TEST_KERNEL_MANIFEST")?;
    let package = KernelPackage::load(std::path::Path::new(&path))?;
    let root = tempfile::tempdir()?;
    let project = root.path().join("项目 package");
    std::fs::create_dir(&project)?;
    let selection = KernelSelection::Package(package);
    let mut config = selection.configuration(
        root.path().join("history"),
        ResponsesGatewayConfig::new("http://127.0.0.1:1/v1", "package-acceptance", None),
    );
    config.project_memory = Some(CodexProjectMemory::new(
        &project,
        &root.path().join("memory"),
    )?);
    let (client, _events) = CodexKernelClient::start(config).await?;
    let result = client
        .request(
            "thread/start",
            json!({
                "cwd": project, "sandbox": "read-only", "approvalPolicy": "on-request",
                "config": {"web_search": "disabled"}
            }),
        )
        .await;
    client.shutdown().await?;
    assert!(
        result?["thread"]["id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    Ok(())
}
