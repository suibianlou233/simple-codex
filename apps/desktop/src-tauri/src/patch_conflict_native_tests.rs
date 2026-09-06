//! Real native approval wait: a changed file invalidates the whole proposal.
use super::*;

#[derive(Clone, Default)]
struct ConflictScript {
    calls: Arc<AtomicUsize>,
    conflict_feedback: Arc<AtomicUsize>,
}

async fn respond_conflict(
    State(script): State<ConflictScript>,
    Json(request): Json<Value>,
) -> axum::response::Response {
    let index = script.calls.fetch_add(1, Ordering::SeqCst);
    if request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["type"] == "custom_tool_call_output"
                && item["output"].as_str().is_some_and(|output| {
                    output.contains("Patch conflict") && output.contains("Exit code: 1")
                })
        })
    }) {
        script.conflict_feedback.fetch_add(1, Ordering::SeqCst);
    }
    let item = if index == 0 {
        json!({"id":"patch_conflict", "call_id":"patch_conflict", "type":"custom_tool_call", "name":"apply_patch", "status":"completed",
          "input":"*** Begin Patch\n*** Add File: should-not-exist.txt\n+new\n*** Update File: file.txt\n@@\n-before\n+agent replacement\n*** Delete File: delete.txt\n*** End Patch"})
    } else {
        json!({"id":"conflict_final", "type":"message", "role":"assistant", "phase":"final_answer", "status":"completed",
          "content":[{"type":"output_text", "text":"A conflict was returned; this script does not retry.", "annotations":[]}]})
    };
    let id = format!("conflict_response_{index}");
    let events = [
        json!({"type":"response.created","response":{"id":id,"status":"in_progress","output":[]}}),
        json!({"type":"response.output_item.added","output_index":0,"item":item}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":id,"status":"completed","output":[item],"usage":{"input_tokens":10,"output_tokens":10,"total_tokens":20}}}),
    ];
    let body = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().expect("event type")
            )
        })
        .collect::<String>();
    ([("content-type", "text/event-stream")], body).into_response()
}

#[tokio::test]
#[ignore = "explicit fixed native kernel and isolated loopback model; no user credentials"]
async fn native_patch_approval_rejects_intervening_edit_without_partial_writes() -> TestResult {
    let executable = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER")
        .ok_or("explicit native kernel required")?;
    let (temp, mut runtime, prepared) = fixture();
    let script = ConflictScript::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway = ResponsesGatewayConfig::new(
        format!("http://{}", listener.local_addr()?),
        "conflict-fixture",
        None,
    )
    .with_deepseek_responses();
    let app = Router::new()
        .route("/responses", post(respond_conflict))
        .with_state(script.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let (client, mut events) = CodexKernelClient::start(CodexKernelConfig::new(
        executable,
        temp.path().join("kernel-home"),
        gateway,
    ))
    .await?;
    let result = async {
        let task = prepared.task_id.to_string();
        let turn = prepared.turn_id.to_string();
        let started = client.request("thread/start", json!({"cwd":prepared.project_root,"model":"conflict-fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":"read-only","ephemeral":false,"config":{"web_search":"disabled","features.memories":false}})).await?;
        let thread = started["thread"]["id"].as_str().ok_or("thread id")?.to_owned();
        runtime.storage.bind_codex_thread(NewCodexThreadBinding {task_id:task.clone(),codex_thread_id:thread.clone(),model_profile_id:prepared.profile.profile_id.clone(),created_at_ms:unix_time_ms()?})?;
        let started = client.request("turn/start", json!({"threadId":thread,"input":[{"type":"text","text":"Use the scripted patch only on these fake fixtures.","text_elements":[]}]})).await?;
        let native_turn = started["turn"]["id"].as_str().ok_or("turn id")?.to_owned();
        let effects = runtime.register_codex_turn(CodexTurnBinding {task_id:task.clone(),turn_id:turn.clone(),codex_thread_id:thread,codex_turn_id:native_turn})?;
        persist_effects(&mut runtime, effects)?;
        let mut approved = false;
        tokio::time::timeout(Duration::from_secs(50), async {
            loop {
                let event = CodexKernelEvent::project(events.next().await.ok_or("native stream ended")??)?;
                if let CodexKernelEvent::ApprovalRequested(request) = &event {
                    assert!(!approved, "fixture has exactly one approval");
                    assert_eq!(request.kind, CodexApprovalKind::FileChange);
                    let action = runtime.prepare_codex_approval(request)?.ok_or("local approval")?;
                    // The user's file changes after the native proposal/preview and
                    // before explicit approval; the model must not delete it.
                    fs::write(prepared.project_root.join("delete.txt"), "user changed after preview\n")?;
                    runtime.resolve_codex_approval_action(&action, true)?;
                    client.respond(request.request_id.clone(), json!({"decision":"accept"})).await?;
                    approved = true;
                    continue;
                }
                let effects = runtime.project_codex_event(event)?;
                if persist_effects(&mut runtime, effects)?.is_some() { break; }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }).await??;
        assert!(approved);
        let update_preserved = fs::read_to_string(prepared.project_root.join("file.txt"))? == "before\n";
        let delete_preserved = fs::read_to_string(prepared.project_root.join("delete.txt")).is_ok_and(|text| text == "user changed after preview\n");
        let add_preserved = !prepared.project_root.join("should-not-exist.txt").exists();
        if !(update_preserved && delete_preserved && add_preserved) {
            return Err(format!("conflicting proposal changed fixture files: update preserved={update_preserved}, delete preserved={delete_preserved}, add prevented={add_preserved}").into());
        }
        assert_eq!(script.conflict_feedback.load(Ordering::SeqCst), 1, "model must receive a real nonzero conflict result");
        assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?, "before\n");
        assert_eq!(fs::read_to_string(prepared.project_root.join("delete.txt"))?, "user changed after preview\n");
        assert!(!prepared.project_root.join("should-not-exist.txt").exists());
        assert!(runtime.actions.values().any(|action| action.status == ActionStatus::Failed));
        drop(runtime);
        let reopened = DesktopRuntime::open(&temp.path().join("data.db"), Arc::new(MemorySecretStore::new()))?;
        assert!(reopened.actions.values().any(|action| action.status == ActionStatus::Failed));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let shutdown = client.shutdown().await;
    server.abort();
    result?;
    shutdown?;
    Ok(())
}
