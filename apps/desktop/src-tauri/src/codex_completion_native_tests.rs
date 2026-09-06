//! Real native child writes after root completion; the production asynchronous
//! watcher owns completion. Only its UI notification callback is substituted.
use super::*;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use local_agent_model::{CodexKernelConfig, CodexKernelWireMessage};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Semaphore;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone)]
struct Script {
    fail_child: bool,
    root_calls: Arc<AtomicUsize>,
    child_calls: Arc<AtomicUsize>,
    child_entered: Arc<Semaphore>,
    release_child: Arc<Semaphore>,
}

async fn respond(
    State(script): State<Script>,
    Json(request): Json<Value>,
) -> axum::response::Response {
    let root = request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["role"] == "user" && item.to_string().contains("ROOT_COMPLETION_FIXTURE")
        })
    });
    let first = if root {
        script.root_calls.fetch_add(1, Ordering::SeqCst) == 0
    } else {
        script.child_calls.fetch_add(1, Ordering::SeqCst) == 0
    };
    if !root && first {
        script.child_entered.add_permits(1);
        script
            .release_child
            .acquire()
            .await
            .expect("release gate")
            .forget();
    }
    let id = Uuid::new_v4().to_string();
    if !root && script.fail_child {
        return (axum::http::StatusCode::BAD_REQUEST,Json(json!({"error":{"message":"PRIVATE_CHILD_MODEL_FAILURE","type":"invalid_request_error"}}))).into_response();
    }
    let item = if root && first {
        json!({"id":format!("spawn_{id}"),"type":"function_call","status":"completed","call_id":format!("call_{id}"),
            "namespace":"multi_agent_v1","name":"spawn_agent","arguments":json!({"message":"CHILD_COMPLETION_FIXTURE: apply the scripted patch in the test project.","fork_context":false}).to_string()})
    } else if !root && first {
        json!({"id":format!("patch_{id}"),"type":"custom_tool_call","status":"completed","call_id":format!("call_{id}"),"name":"apply_patch",
            "input":"*** Begin Patch\n*** Update File: file.txt\n@@\n-before\n+native child wrote after root completed\n*** End Patch"})
    } else {
        json!({"id":format!("msg_{id}"),"type":"message","role":"assistant","phase":"final_answer","status":"completed",
            "content":[{"type":"output_text","text":"SCRIPTED_DONE","annotations":[]}]})
    };
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
#[ignore = "explicit pinned kernel required; actual child patch with loopback scripted model"]
async fn native_child_late_patch_is_included_by_automatic_completion() -> TestResult {
    exercise_child_approval(None, false).await
}

#[tokio::test]
#[ignore = "explicit pinned kernel; real child approval accepted through desktop attribution"]
async fn native_child_approval_accepts_only_after_explicit_confirmation() -> TestResult {
    exercise_child_approval(Some(true), false).await
}

#[tokio::test]
#[ignore = "explicit pinned kernel; real child approval rejected without writes"]
async fn native_child_approval_rejection_preserves_files_and_root_lifecycle() -> TestResult {
    exercise_child_approval(Some(false), false).await
}

#[tokio::test]
#[ignore = "explicit pinned kernel; child model failure must survive successful root and restart"]
async fn native_child_failure_is_reported_beside_completed_root() -> TestResult {
    exercise_child_approval(None, true).await
}

async fn exercise_child_approval(decision: Option<bool>, fail_child: bool) -> TestResult {
    let executable = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER")
        .ok_or("explicit pinned kernel required")?;
    let (temp, mut runtime, prepared) = running_fixture();
    let script = Script {
        fail_child,
        root_calls: Arc::new(AtomicUsize::new(0)),
        child_calls: Arc::new(AtomicUsize::new(0)),
        child_entered: Arc::new(Semaphore::new(0)),
        release_child: Arc::new(Semaphore::new(0)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway = ResponsesGatewayConfig::new(
        format!("http://{}", listener.local_addr()?),
        "completion-fixture",
        None,
    )
    .with_deepseek_responses();
    let app = Router::new()
        .route("/responses", post(respond))
        .with_state(script.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let (client, mut events) = CodexKernelClient::start(CodexKernelConfig::new(
        executable,
        temp.path().join("kernel-home"),
        gateway,
    ))
    .await?;
    let mut watcher = None;
    let outcome = async {
        let task = prepared.task_id.to_string();
        let turn = prepared.turn_id.to_string();
        let started = client.request("thread/start", json!({"cwd":prepared.project_root,"model":"completion-fixture",
            "approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":if decision.is_some() {"read-only"} else {"workspace-write"},"ephemeral":false,
            "config":{"web_search":"disabled","features.memories":false}})).await?;
        let thread = started["thread"]["id"].as_str().ok_or("root thread")?.to_owned();
        let started = client.request("turn/start", json!({"threadId":thread,
            "input":[{"type":"text","text":"ROOT_COMPLETION_FIXTURE: run only the scripted operation on fake test files.","text_elements":[]}]})).await?;
        let native_turn = started["turn"]["id"].as_str().ok_or("native turn")?.to_owned();
        runtime.register_codex_turn(CodexTurnBinding {task_id:task.clone(),turn_id:turn.clone(),codex_thread_id:thread.clone(),codex_turn_id:native_turn})?;
        runtime.codex_turn_owners.insert(turn.clone(), CodexTurnOwner {kernel_key:"fixture".into(),instance_id:client.instance_id().into()});
        let mut child = None;
        let pending = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let wire = events.next().await.ok_or("native stream closed")??;
                if let CodexKernelWireMessage::Request { id, .. } = &wire {
                    client.respond(id.clone(), json!({"decision":"decline"})).await?;
                    return Err("unexpected approval; never elevate fixture".into());
                }
                if let CodexKernelWireMessage::Notification {method,params} = &wire
                    && method == "item/completed" && params["item"]["tool"] == "spawnAgent" {
                    child = params["item"]["receiverThreadIds"][0].as_str().map(str::to_owned);
                }
                for effect in runtime.project_codex_event(CodexKernelEvent::project(wire)?)? {
                    match effect {
                        CodexDesktopEffect::PersistAssistant(message) => runtime.persist_codex_assistant_projection(message)?,
                        CodexDesktopEffect::Stream(_) => {},
                        CodexDesktopEffect::Finish(terminal) => {
                            assert_eq!(terminal.status, ProjectedTurnStatus::Completed);
                            let Dispatch::Watch(pending) = runtime.defer_completion(&terminal, client.instance_id())? else { return Err("live completion must defer".into()); };
                            return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(*pending);
                        }
                    }
                }
            }
        }).await??;
        let child = child.ok_or("actual native child required")?;
        assert!(runtime.spawned_children(&task,&turn)?.contains(&child),"real spawn id must be durably seeded even if it later unloads");
        tokio::time::timeout(Duration::from_secs(10), script.child_entered.acquire()).await??.forget();
        let state = client.request("thread/read", json!({"threadId":child})).await?;
        assert_eq!(state["thread"]["status"]["type"], "active");
        let runtime = Arc::new(Mutex::new(runtime));
        let (send, mut notices) = tokio::sync::mpsc::unbounded_channel();
        watcher = Some(tokio::spawn(watch_completion(Arc::clone(&runtime), client.clone(), pending, move |terminal| {
            if let Some(terminal) = terminal { let _ = send.send(terminal); }
        })));
        // The child is held by an explicit gate, not a guessed response delay.
        assert!(tokio::time::timeout(Duration::from_millis(1200), notices.recv()).await.is_err());
        {
            let live = runtime.lock().expect("runtime");
            assert_eq!(live.snapshot()?.turns[0].phase, "waiting_children");
            assert!(live.project_leases.contains_key(&turn));
            assert!(!live.storage.load_events(&task)?.iter().any(|event| event.event_type == "review_after" || event.event_type == "turn_finished"));
            assert!(live.project_execution_busy(&task, None)?);
            assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?, "before\n");
        }
        script.release_child.add_permits(1);
        let mut approval_action: Option<String> = None;
        let terminal = tokio::time::timeout(Duration::from_secs(30), async {
            let mut completed = None;
            loop {
                let action_done = approval_action.as_ref().is_none_or(|id| matches!(runtime.lock().expect("runtime").actions[id].status,ActionStatus::Applied | ActionStatus::Rejected));
                if action_done && let Some(terminal) = completed.take() { return Ok::<_,Box<dyn std::error::Error + Send + Sync>>(terminal); }
                tokio::select! {
                    terminal = notices.recv(), if completed.is_none() => completed = Some(terminal.ok_or("watcher closed without completion")?),
                    wire = events.next() => {
                        let event = CodexKernelEvent::project(wire.ok_or("native stream closed")??)?;
                        if let CodexKernelEvent::ApprovalRequested(request) = event {
                            let Some(accept) = decision else {
                                client.respond(request.request_id,json!({"decision":"decline"})).await?;
                                return Err("unexpected child approval".into());
                            };
                            assert_eq!(request.thread_id,child);
                            assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?,"before\n");
                            let id = codex_descendants::prepare_approval(&runtime,&client,&request).await?.ok_or("child approval must be attributed")?;
                            {
                                let mut live = runtime.lock().expect("runtime");
                                assert_eq!(live.actions[&id].status,ActionStatus::Pending);
                                assert_eq!(live.actions[&id].task_id,task);
                                assert_eq!(live.actions[&id].turn_id,turn);
                                assert!(live.project_leases.contains_key(&turn));
                                live.resolve_codex_approval_action(&id,accept)?;
                            }
                            client.respond(request.request_id,json!({"decision":if accept {"accept"} else {"decline"}})).await?;
                            approval_action = Some(id);
                        } else {
                            let effects = runtime.lock().expect("runtime").project_codex_event(event)?;
                            assert!(!effects.iter().any(|effect| matches!(effect,CodexDesktopEffect::Finish(_))),"child events cannot finish the root");
                        }
                    }
                }
            }
        }).await??;
        assert_eq!(terminal.status, ProjectedTurnStatus::Completed);
        if let Some(worker) = watcher.take() { worker.await?; }
        assert_eq!(decision.is_some(),approval_action.is_some(),"explicit native approval must actually occur");
        let changed = decision != Some(false) && !fail_child;
        assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?, if changed {"native child wrote after root completed\n"} else {"before\n"});
        let child_turns = client.request("thread/turns/list", json!({"threadId":child,"limit":1,"sortDirection":"desc","itemsView":"notLoaded"})).await?;
        assert_eq!(child_turns["data"][0]["status"], if fail_child {"failed"} else {"completed"});
        {
            let live = runtime.lock().expect("runtime");
            assert!(!live.project_leases.contains_key(&turn));
            assert!(!live.storage.load_events(&task)?.iter().any(|event| event.event_type.starts_with("review_")));
            assert_eq!(live.snapshot()?.turns[0].status, "completed");
        }
        drop(runtime);
        let reopened = DesktopRuntime::open(&temp.path().join("data.db"), Arc::new(MemorySecretStore::new()))?;
        let snapshot = reopened.snapshot()?;
        let report = snapshot.turns[0].child_report.as_ref().ok_or("persisted child report required")?;
        assert_eq!(report.assignments.len(), 1, "actual native spawn instruction must survive restart");
        assert_eq!(report.assignments[0].receivers, vec![child.clone()]);
        assert_eq!(report.assignments[0].status, crate::runtime::subagent_report::DispatchStatus::Dispatched);
        assert!(report.assignments[0].instruction.contains("CHILD_COMPLETION_FIXTURE"));
        assert_eq!(report.outcomes.len(),1);
        assert_eq!(report.outcomes[0].thread_id,child);
        assert_eq!(report.outcomes[0].status,if fail_child {local_agent_model::CodexChildStatus::Failed} else {local_agent_model::CodexChildStatus::Completed});
        assert_eq!(report.rejected_operations,usize::from(decision == Some(false)));
        assert_eq!(snapshot.turns[0].status,"completed","root native status must not be rewritten");
        if let Some(id) = approval_action { assert_eq!(reopened.actions[&id].status,if changed {ActionStatus::Applied} else {ActionStatus::Rejected}); }
        assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?, "before\n");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    script.release_child.add_permits(1);
    let shutdown = client.shutdown().await;
    if let Some(watcher) = watcher {
        watcher.abort();
        let _ = watcher.await;
    }
    server.abort();
    outcome?;
    shutdown?;
    Ok(())
}
