//! Headless startup policy acceptance against the pinned native executable.
//! No external model requests; fixtures are retained under a unique temp root.
use local_agent_model::{
    CodexKernelClient, CodexKernelConfig, CodexKernelError, CodexKernelProcess, CodexProjectMemory,
    ResponsesGatewayConfig,
};
use serde_json::json;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[ignore = "requires freshly built pinned SIMPLE_TEST_CODEX_APP_SERVER"]
async fn scoped_native_instances_reset_only_own_memory_and_reject_scope_override() -> Result<()> {
    let executable =
        std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER").ok_or("pinned executable required")?;
    let root = std::env::temp_dir().join(format!("simple-project-memory-{}", uuid::Uuid::new_v4()));
    let history = root.join("history");
    let project_a = root.join("项目 A");
    let project_b = root.join("项目 B");
    let memory_a = root.join("记忆 A");
    let memory_b = root.join("记忆 B");
    for project in [&project_a, &project_b] {
        std::fs::create_dir_all(project)?;
    }
    let config =
        |project: &std::path::Path, memory: &std::path::Path| -> Result<CodexKernelConfig> {
            let mut config = CodexKernelConfig::new(
                &executable,
                &history,
                ResponsesGatewayConfig::new("http://127.0.0.1:1", "scope-test", None),
            );
            config.project_memory = Some(CodexProjectMemory::new(project, memory)?);
            Ok(config)
        };
    let (a, _events_a) = CodexKernelClient::start(config(&project_a, &memory_a)?).await?;
    let (b, _events_b) = CodexKernelClient::start(config(&project_b, &memory_b)?).await?;
    for (client, project) in [(&a, &project_a), (&b, &project_b)] {
        client.request("thread/start", json!({"cwd":std::fs::canonicalize(project)?,"approvalPolicy":"never","sandbox":"read-only","config":{"web_search":"disabled"}})).await?;
    }
    for memory in [&memory_a, &memory_b, &history] {
        std::fs::create_dir_all(memory.join("memories"))?;
        std::fs::write(memory.join("memories/MEMORY.md"), "isolated fixture marker")?;
    }
    let denied = a.request("thread/start", json!({"cwd":std::fs::canonicalize(&project_b)?,"config":{"memories.project_scope.project_root":std::fs::canonicalize(&project_b)?,"memories.project_scope.storage_home":memory_b}})).await;
    assert!(
        matches!(&denied, Err(CodexKernelError::Rpc(error)) if error["message"].as_str().is_some_and(|message| message.contains("project memory scope cannot change"))),
        "{denied:?}"
    );
    a.request("memory/reset", json!({})).await?;
    assert!(!memory_a.join("memories/MEMORY.md").exists());
    assert_eq!(
        std::fs::read_to_string(memory_b.join("memories/MEMORY.md"))?,
        "isolated fixture marker"
    );
    assert_eq!(
        std::fs::read_to_string(history.join("memories/MEMORY.md"))?,
        "isolated fixture marker"
    );
    b.request("memory/reset", json!({})).await?;
    assert!(!memory_b.join("memories/MEMORY.md").exists());
    assert_eq!(
        std::fs::read_to_string(history.join("memories/MEMORY.md"))?,
        "isolated fixture marker"
    );
    a.shutdown().await?;
    b.shutdown().await?;
    Ok(())
}

#[test]
fn project_memory_launch_rejects_missing_project_and_relative_storage() {
    let root = std::env::temp_dir().join(format!("simple-memory-path-{}", uuid::Uuid::new_v4()));
    assert!(CodexProjectMemory::new(&root, &root.join("memory")).is_err());
    std::fs::create_dir_all(&root).expect("fixture root");
    assert!(CodexProjectMemory::new(&root, std::path::Path::new("relative")).is_err());
    assert!(!root.join("memory").exists());
}

#[tokio::test]
#[ignore = "requires SIMPLE_TEST_CODEX_APP_SERVER and its pinned patch companion"]
async fn invalid_native_configuration_cannot_fall_back_to_shared_memory() -> Result<()> {
    let executable = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER")
        .ok_or("set SIMPLE_TEST_CODEX_APP_SERVER to the pinned native executable")?;
    let root = std::env::temp_dir().join(format!("simple-config-policy-{}", uuid::Uuid::new_v4()));
    let valid_home = root.join("valid");
    let process = CodexKernelProcess::start(CodexKernelConfig::new(
        &executable,
        &valid_home,
        ResponsesGatewayConfig::new("http://127.0.0.1:1/v1", "configuration-test", None),
    ))
    .await?;
    process.shutdown().await?;

    for (name, input) in [
        ("invalid-syntax", "[memories.project_scope\n"),
        (
            "unknown-field",
            "[memories.project_scope]\nmisspelled_root = 'unused'\n",
        ),
    ] {
        let home = root.join(name);
        let shared = home.join("memories/MEMORY.md");
        std::fs::create_dir_all(home.join("memories"))?;
        std::fs::write(&shared, "shared sentinel")?;
        std::fs::write(home.join("config.toml"), input)?;
        let result = CodexKernelProcess::start(CodexKernelConfig::new(
            &executable,
            &home,
            ResponsesGatewayConfig::new("http://127.0.0.1:1/v1", "configuration-test", None),
        ))
        .await;
        let error = match result {
            Ok(process) => {
                process.shutdown().await?;
                return Err(
                    format!("{name}: invalid configuration exposed an initialized kernel").into(),
                );
            }
            Err(error) => error,
        };
        assert!(
            matches!(
                error,
                CodexKernelError::UnexpectedExit | CodexKernelError::Write(_)
            ),
            "{name}: {error:?}"
        );
        assert!(
            !home.join("state_5.sqlite").exists(),
            "invalid configuration must fail before state initialization"
        );
        assert_eq!(std::fs::read_to_string(home.join("config.toml"))?, input);
        assert_eq!(std::fs::read_to_string(shared)?, "shared sentinel");
    }
    Ok(())
}
