//! Uses the public native API and real product persistence without a desktop UI.
use local_agent_model::{
    CodexKernelClient, CodexKernelConfig, CodexKernelError, CodexSessionBridge,
    ResponsesGatewayConfig,
};
use local_agent_storage::{NewCodexThreadBinding, NewModelProfile, NewProject, NewTask, Storage};
use serde_json::json;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A second profile revision must not be launched over a live history writer.
/// Unsubscribe is not a release operation: upstream deliberately retains idle
/// threads for 30 minutes. Keep this real-API evidence for the host lifecycle fix.
#[tokio::test]
#[ignore = "requires SIMPLE_TEST_CODEX_APP_SERVER and its patch companion"]
async fn live_history_owner_blocks_profile_replacement_even_after_unsubscribe() -> Result<()> {
    let executable = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER")
        .ok_or("pinned native executable required")?;
    let temp = tempfile::tempdir()?;
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project)?;
    let config = || {
        CodexKernelConfig::new(
            &executable,
            temp.path().join("home"),
            ResponsesGatewayConfig::new("http://127.0.0.1:1/v1", "fixture", None),
        )
    };
    let (owner, _owner_events) = CodexKernelClient::start(config()).await?;
    let started = owner
        .request(
            "thread/start",
            json!({
                "cwd": project, "approvalPolicy":"never", "sandbox":"read-only", "ephemeral":false
            }),
        )
        .await?;
    let thread = started["thread"]["id"]
        .as_str()
        .ok_or("thread ID")?
        .to_owned();
    owner.request("thread/inject_items", json!({"threadId":thread,"items":[{
        "type":"message","role":"user","content":[{"type":"input_text","text":"Retain hot profile history."}]
    }]})).await?;
    owner
        .request(
            "thread/name/set",
            json!({"threadId":thread,"name":"live writer fixture"}),
        )
        .await?;
    let (replacement, _replacement_events) = CodexKernelClient::start(config()).await?;
    let blocked = replacement
        .request(
            "thread/resume",
            json!({"threadId":thread,"excludeTurns":true}),
        )
        .await;
    assert!(
        matches!(&blocked, Err(CodexKernelError::Rpc(error))
        if error["message"].as_str().is_some_and(|message| message.contains("already has an active writer"))),
        "expected a writer-ownership conflict, got {blocked:?}"
    );
    let unsubscribed = owner
        .request("thread/unsubscribe", json!({"threadId":thread}))
        .await?;
    assert_eq!(unsubscribed["status"], "unsubscribed");
    let still_blocked = replacement
        .request(
            "thread/resume",
            json!({"threadId":thread,"excludeTurns":true}),
        )
        .await;
    assert!(
        matches!(&still_blocked, Err(CodexKernelError::Rpc(error))
        if error["message"].as_str().is_some_and(|message| message.contains("already has an active writer"))),
        "unsubscribe must not be mistaken for writer release: {still_blocked:?}"
    );
    owner.shutdown().await?;
    let snapshot = CodexSessionBridge::new(replacement.clone())
        .resume_and_hydrate(&thread)
        .await?;
    assert_eq!(snapshot.thread_id, thread);
    assert_eq!(snapshot.thread["name"], "live writer fixture");
    replacement.shutdown().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires SIMPLE_TEST_CODEX_APP_SERVER and its patch companion"]
async fn native_history_is_located_and_cold_resumed_in_place_after_profile_update() -> Result<()> {
    let executable = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER")
        .ok_or("pinned native executable required")?;
    let temp = tempfile::tempdir()?;
    let managed = temp.path().join("kernels");
    let database = temp.path().join("simple.sqlite");
    let home = "a".repeat(64);
    let changed_default = "b".repeat(64);
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project)?;
    let mut storage = Storage::open(&database)?;
    storage.create_project(NewProject {
        project_id: "project".into(),
        root: project.to_string_lossy().into(),
        name: "fixture".into(),
    })?;
    storage.create_task(NewTask {
        task_id: "task".into(),
        project_id: "project".into(),
        title: "fixture".into(),
        created_at_ms: 1,
    })?;
    storage.upsert_model_profile(NewModelProfile {
        profile_id: "profile".into(),
        name: "fixture".into(),
        base_url: "http://127.0.0.1:1".into(),
        model: "fixture".into(),
        dialect: "standard".into(),
        credential_ref: "fixture-no-secret".into(),
        max_output_tokens: None,
        context_window_tokens: None,
        timeout_ms: 1000,
        is_default: true,
        created_at_ms: 1,
        updated_at_ms: 1,
    })?;
    assert_eq!(
        storage.resolve_codex_history_home("task", &managed, &home)?,
        home
    );
    let (client, _events) = CodexKernelClient::start(CodexKernelConfig::new(
        &executable,
        managed.join(&home),
        ResponsesGatewayConfig::new("http://127.0.0.1:1/v1", "fixture", None),
    ))
    .await?;
    let started = client
        .request(
            "thread/start",
            json!({"cwd":project,"approvalPolicy":"never","sandbox":"read-only","ephemeral":false}),
        )
        .await?;
    let thread = started["thread"]["id"]
        .as_str()
        .ok_or("thread ID")?
        .to_owned();
    storage.bind_codex_thread(NewCodexThreadBinding {
        task_id: "task".into(),
        codex_thread_id: thread.clone(),
        model_profile_id: "profile".into(),
        created_at_ms: 2,
    })?;
    // An ordinary task can request controls immediately after thread/start.
    assert_eq!(
        storage.resolve_codex_history_home("task", &managed, &changed_default)?,
        home
    );
    client.request("thread/inject_items", json!({"threadId":thread,"items":[{
        "type":"message","role":"user","content":[{"type":"input_text","text":"Retain this isolated history fixture."}]
    }]})).await?;
    client
        .request(
            "thread/name/set",
            json!({"threadId":thread,"name":"preserved native history"}),
        )
        .await?;
    client.shutdown().await?;
    drop(storage);
    let mut storage = Storage::open(&database)?;
    storage.set_default_model_profile("profile", 500)?;
    let resolved = storage.resolve_codex_history_home("task", &managed, &changed_default)?;
    assert_eq!(resolved, home);
    // Simulate an existing pre-upgrade task: only the native thread association
    // exists, so discovery must consult the real native SQLite index.
    let mut legacy = Storage::open(temp.path().join("legacy-product.sqlite"))?;
    legacy.create_project(NewProject {
        project_id: "project".into(),
        root: project.to_string_lossy().into(),
        name: "legacy".into(),
    })?;
    legacy.create_task(NewTask {
        task_id: "legacy".into(),
        project_id: "project".into(),
        title: "legacy".into(),
        created_at_ms: 1,
    })?;
    legacy.upsert_model_profile(NewModelProfile {
        profile_id: "profile".into(),
        name: "updated".into(),
        base_url: "http://127.0.0.1:1".into(),
        model: "fixture".into(),
        dialect: "standard".into(),
        credential_ref: "fixture-no-secret".into(),
        max_output_tokens: None,
        context_window_tokens: None,
        timeout_ms: 1000,
        is_default: true,
        created_at_ms: 1,
        updated_at_ms: 500,
    })?;
    legacy.bind_codex_thread(NewCodexThreadBinding {
        task_id: "legacy".into(),
        codex_thread_id: thread.clone(),
        model_profile_id: "profile".into(),
        created_at_ms: 2,
    })?;
    assert_eq!(
        legacy.resolve_codex_history_home("legacy", &managed, &changed_default)?,
        home
    );
    let (client, _events) = CodexKernelClient::start(CodexKernelConfig::new(
        executable,
        managed.join(resolved),
        ResponsesGatewayConfig::new("http://127.0.0.1:1/v1", "fixture", None),
    ))
    .await?;
    let snapshot = CodexSessionBridge::new(client.clone())
        .resume_and_hydrate(&thread)
        .await?;
    assert_eq!(snapshot.thread_id, thread);
    assert_eq!(snapshot.thread["name"], "preserved native history");
    client.shutdown().await?;
    assert!(!managed.join(changed_default).exists());
    Ok(())
}
