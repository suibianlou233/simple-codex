//! Real kernel -> desktop projection and native file edits, without workspace snapshots.
//! Scripted loopback inference tests mechanics, not real-model coding quality.
use super::*;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use local_agent_model::{CodexKernelConfig, CodexKernelWireMessage};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[path = "memory_notes_native_tests.rs"]
mod memory_notes;

#[path = "patch_conflict_native_tests.rs"]
mod patch_conflicts;

struct LostStartReply {
    client: CodexKernelClient,
    starts: AtomicUsize,
}

#[async_trait::async_trait]
impl local_agent_model::CodexRpc for LostStartReply {
    async fn request(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, local_agent_model::CodexKernelError> {
        if method == "turn/start" {
            self.starts.fetch_add(1, Ordering::SeqCst);
        }
        let response = self.client.request(method, params).await?;
        if method == "turn/start" {
            // Fault injection: the real kernel accepted the request, but this
            // caller loses its response. No fake native history is supplied.
            Err(local_agent_model::CodexKernelError::Unavailable)
        } else {
            Ok(response)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone)]
struct ScriptedModel {
    requests: Arc<AtomicUsize>,
    tool_result_seen: Arc<AtomicUsize>,
    ending: Ending,
}

async fn respond(
    State(model): State<ScriptedModel>,
    Json(request): Json<Value>,
) -> axum::response::Response {
    let index = model.requests.fetch_add(1, Ordering::SeqCst);
    if request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["type"] == "custom_tool_call_output"
                && item["output"]
                    .as_str()
                    .is_some_and(|output| output.contains("Success"))
        })
    }) {
        model.tool_result_seen.fetch_add(1, Ordering::SeqCst);
    }
    if index > 0 && model.ending == Ending::Failed {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": {
                "message": "SCRIPTED_FAILURE_AFTER_PATCH", "type": "invalid_request_error"
            }})),
        )
            .into_response();
    }
    if index > 0 && model.ending == Ending::Cancelled {
        // Cancellation is triggered by the actual successful fileChange event.
        tokio::time::sleep(Duration::from_secs(40)).await;
    }
    let id = Uuid::new_v4().to_string();
    let item = if index == 0 {
        json!({"id":format!("patch_{id}"),"type":"custom_tool_call","status":"completed",
            "call_id":format!("call_{id}"),"name":"apply_patch",
            "input":"*** Begin Patch\n*** Update File: file.txt\n@@\n-before\n+agent change\n*** Delete File: delete.txt\n*** Add File: 中文 空格.txt\n+中文 \"引号\" $value %PATH% & |\n+第二行\n*** End Patch"})
    } else {
        json!({"id":format!("msg_{id}"),"type":"message","role":"assistant","phase":"final_answer",
            "status":"completed","content":[{"type":"output_text","text":"SCRIPTED_REVIEW_DONE","annotations":[]}]})
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
                event["type"].as_str().expect("type")
            )
        })
        .collect::<String>();
    ([("content-type", "text/event-stream")], body).into_response()
}

fn persist_effects(
    runtime: &mut DesktopRuntime,
    effects: Vec<CodexDesktopEffect>,
) -> TestResult<Option<ProjectedTurnStatus>> {
    let mut terminal_status = None;
    for effect in effects {
        match effect {
            CodexDesktopEffect::Stream(_) => {} // No browser or Tauri event bus in this test.
            CodexDesktopEffect::PersistAssistant(message) => {
                runtime.persist_codex_assistant_projection(message)?
            }
            CodexDesktopEffect::Finish(terminal) => {
                runtime.finish_codex_projected_turn(&terminal)?;
                terminal_status = Some(terminal.status);
            }
        }
    }
    Ok(terminal_status)
}

fn native_configuration(home: PathBuf, gateway: ResponsesGatewayConfig) -> TestResult<CodexKernelConfig> {
    if let Some(path) = std::env::var_os("SIMPLE_TEST_KERNEL_MANIFEST") {
        let package = local_agent_model::KernelPackage::load(Path::new(&path))?;
        return Ok(local_agent_model::KernelSelection::Package(package).configuration(home, gateway));
    }
    let executable = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER").ok_or("explicit test kernel required")?;
    Ok(CodexKernelConfig::new(executable, home, gateway))
}

async fn exercise(ending: Ending, register_late: bool) -> TestResult {
    let (temp, mut runtime, prepared) = fixture();
    let model = ScriptedModel {
        requests: Arc::new(AtomicUsize::new(0)),
        tool_result_seen: Arc::new(AtomicUsize::new(0)),
        ending,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let app = Router::new()
        .route("/responses", post(respond))
        .with_state(model.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let gateway =
        ResponsesGatewayConfig::new(endpoint, "review-fixture", None).with_deepseek_responses();
    let (client, mut events) = CodexKernelClient::start(native_configuration(temp.path().join("kernel-home"), gateway)?)
    .await?;
    let outcome = async {
        let task = prepared.task_id.to_string();
        let turn = prepared.turn_id.to_string();
        assert!(!prepared.project_root.join(".git").exists());
        assert!(!runtime.storage.load_events(&task)?.iter().any(|event| event.event_type.starts_with("review_")));
        let started = client.request("thread/start", json!({
            "cwd":prepared.project_root, "model":"review-fixture", "approvalPolicy":"on-request",
            "approvalsReviewer":"user", "sandbox":"workspace-write", "ephemeral":false,
            "config":{"web_search":"disabled", "features.memories":false}
        })).await?;
        let thread = started["thread"]["id"].as_str().ok_or("thread id")?.to_owned();
        runtime.storage.bind_codex_thread(NewCodexThreadBinding {
            task_id: task.clone(), codex_thread_id: thread.clone(),
            model_profile_id: prepared.profile.profile_id.clone(), created_at_ms: unix_time_ms()?,
        })?;
        let pending_submission = if register_late {
            runtime.codex_turn_owners.insert(turn.clone(),CodexTurnOwner {kernel_key:"fixture".into(),instance_id:client.instance_id().into()});
            Some(runtime.begin_codex_submission(&prepared,&thread,client.instance_id())?)
        } else {None};
        let started = client.request("turn/start", json!({"threadId":thread,"clientUserMessageId":prepared.user_message_id,
            "input":[{"type":"text","text":"Apply the scripted patch only to these fake fixtures.","text_elements":[]}]
        })).await?;
        let native_turn = started["turn"]["id"].as_str().ok_or("turn id")?.to_owned();
        let binding = CodexTurnBinding {
            task_id: task.clone(), turn_id: turn.clone(), codex_thread_id: thread.clone(), codex_turn_id: native_turn.clone(),
        };
        if !register_late {
            let effects = runtime.register_codex_turn(binding.clone())?;
            persist_effects(&mut runtime, effects)?;
        }
        let (status, patch_seen, runtime) = tokio::time::timeout(Duration::from_secs(55), async {
            let mut patch_seen = false;
            loop {
                let wire = events.next().await.ok_or("native stream closed")??;
                if let CodexKernelWireMessage::Request { id, .. } = wire {
                    client.respond(id, json!({"decision":"decline"})).await?;
                    return Err("unexpected approval: never elevate acceptance fixtures".into());
                }
                let event = CodexKernelEvent::project(wire)?;
                let patch_completed = matches!(&event, CodexKernelEvent::ItemCompleted { turn_id, item, .. }
                    if turn_id == &native_turn && item.kind == "fileChange" && item.execution_succeeded());
                let terminal_arrived = matches!(&event,CodexKernelEvent::TurnCompleted{turn_id,..} if turn_id == &native_turn);
                let mut effects = runtime.project_codex_event(event)?;
                if register_late && terminal_arrived {
                    assert!(effects.is_empty());
                    assert!(runtime.actions.is_empty());
                    // Simulate losing the response: the production lookup only
                    // receives the pre-existing client id, never response.turn.id.
                    let recovered = local_agent_model::find_codex_submission(&client,&thread,&prepared.user_message_id).await?.ok_or("missing submission identity")?;
                    assert_eq!(recovered,native_turn);
                    assert_eq!(local_agent_model::find_codex_submission(&client,&thread,&prepared.user_message_id).await?,Some(recovered.clone()));
                    assert!(local_agent_model::find_codex_submission(&client,&thread,"never-submitted").await?.is_none());
                    let pending = pending_submission.as_ref().ok_or("pending submission")?;
                    assert!(matches!(runtime.mark_codex_submission_failed(prepared.task_id,prepared.turn_id,"discarded reply"),Err(DesktopError::SubmissionPending)));
                    let shared = Arc::new(Mutex::new(runtime));
                    effects.extend(crate::runtime::codex_submission::recover_once(&shared,&client,pending).await?.ok_or("real history must recover")?);
                    runtime = Arc::try_unwrap(shared).map_err(|_|"unexpected runtime owner")?.into_inner().map_err(|_|"poisoned runtime")?;
                    assert_eq!(runtime.codex_interrupt_target(&turn)?.ok_or("restored stop target")?.turn_id,recovered);
                    assert!(runtime.actions.values().any(|action| action.status == ActionStatus::Applied));
                    assert!(runtime.pending_codex_events.iter().all(|event| !codex_event_matches_turn(event,&binding)));
                }
                let status = persist_effects(&mut runtime, effects)?;
                if patch_completed {
                    patch_seen = true;
                    assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?, "agent change\n");
                    if ending == Ending::Cancelled {
                        client.request("turn/interrupt", json!({"threadId":thread,"turnId":native_turn})).await?;
                    }
                }
                if let Some(status) = status {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>((status, patch_seen, runtime));
                }
            }
        }).await??;
        assert!(patch_seen, "actual successful native patch event required");
        assert_eq!(status, match ending {
            Ending::Completed => ProjectedTurnStatus::Completed,
            Ending::Failed => ProjectedTurnStatus::Failed,
            Ending::Cancelled => ProjectedTurnStatus::Cancelled,
        });
        if ending != Ending::Cancelled {
            assert!(model.tool_result_seen.load(Ordering::SeqCst) > 0, "upstream must receive successful patch feedback");
        }
        assert!(!runtime.storage.load_events(&task)?.iter().any(|event| event.event_type.starts_with("review_")));
        assert!(!temp.path().join("review-blobs").exists());
        drop(runtime);
        let runtime = DesktopRuntime::open(&temp.path().join("data.db"), Arc::new(MemorySecretStore::new()))?;
        assert_eq!(runtime.core.snapshot().turns.iter().find(|item| item.id == prepared.turn_id).ok_or("persisted turn")?.status,
            match ending { Ending::Completed => TurnStatus::Completed, Ending::Failed => TurnStatus::Failed, Ending::Cancelled => TurnStatus::Cancelled });
        assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?, "agent change\n");
        assert!(!prepared.project_root.join("delete.txt").exists());
        assert!(prepared.project_root.join("中文 空格.txt").exists());
        assert!(!prepared.project_root.join(".git").exists());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let shutdown = client.shutdown().await;
    server.abort();
    outcome?;
    shutdown?;
    Ok(())
}

#[tokio::test]
#[ignore = "explicit pinned kernel; early approval held until local registration, no automatic approval"]
async fn native_early_approval_waits_for_local_registration() -> TestResult {
    early_approval(false).await
}

#[tokio::test]
#[ignore = "explicit pinned kernel; uncertain dispatch stop is durable and never grants approval"]
async fn native_uncertain_submission_recovers_identity_and_stops_without_writing() -> TestResult {
    early_approval(true).await
}

async fn early_approval(stop_pending: bool) -> TestResult {
    let (temp, runtime, prepared) = fixture();
    let runtime = Arc::new(Mutex::new(runtime));
    let model = ScriptedModel {
        requests: Arc::new(AtomicUsize::new(0)),
        tool_result_seen: Arc::new(AtomicUsize::new(0)),
        ending: Ending::Completed,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let gateway = ResponsesGatewayConfig::new(
        format!("http://{}", listener.local_addr()?),
        "review-fixture",
        None,
    )
    .with_deepseek_responses();
    let app = Router::new()
        .route("/responses", post(respond))
        .with_state(model);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let (client, mut events) = CodexKernelClient::start(native_configuration(temp.path().join("kernel-home"), gateway)?)
    .await?;
    let outcome = async {
        let started = client.request("thread/start",json!({"cwd":prepared.project_root,"model":"review-fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":"read-only","ephemeral":false,"config":{"web_search":"disabled","features.memories":false}})).await?;
        let thread = started["thread"]["id"].as_str().ok_or("thread id")?.to_owned();
        let gate = {
            let mut live = runtime.lock().map_err(|_|"runtime lock")?;
            live.codex_turn_owners.insert(prepared.turn_id.to_string(),CodexTurnOwner{kernel_key:"test".into(),instance_id:client.instance_id().into()});
            live.codex_event_gate(client.instance_id())
        };
        let handoff = gate.lock().await;
        let pending = {
            let mut live = runtime.lock().map_err(|_|"runtime lock")?;
            live.storage.bind_codex_thread(NewCodexThreadBinding {task_id:prepared.task_id.to_string(),codex_thread_id:thread.clone(),model_profile_id:prepared.profile.profile_id.clone(),created_at_ms:unix_time_ms()?})?;
            live.begin_codex_submission(&prepared,&thread,client.instance_id())?
        };
        // The independent RPC reader must deliver this response while event
        // processing is paused; otherwise this test times out instead of passing.
        let faulty = LostStartReply {client:client.clone(),starts:AtomicUsize::new(0)};
        let receipt = tokio::time::timeout(Duration::from_secs(20),local_agent_model::start_codex_turn_recovering(&faulty,&thread,&prepared.user_message_id,json!({"threadId":thread,"clientUserMessageId":prepared.user_message_id,"input":[{"type":"text","text":"Apply the fixture patch only.","text_elements":[]}]}))).await??;
        assert!(receipt.recovered);
        assert_eq!(faulty.starts.load(Ordering::SeqCst),1);
        let turn = receipt.turn["id"].as_str().ok_or("turn id")?.to_owned();
        let request = tokio::time::timeout(Duration::from_secs(15),async {
            loop {
                let event = CodexKernelEvent::project(events.next().await.ok_or("stream ended")??)?;
                if let CodexKernelEvent::ApprovalRequested(request) = event {
                    return Ok::<_,Box<dyn std::error::Error + Send + Sync>>(request);
                }
            }
        }).await??;
        assert_eq!(request.thread_id,thread);
        assert_eq!(request.turn_id,turn);
        assert_eq!(local_agent_model::find_codex_submission(&client,&thread,&prepared.user_message_id).await?,Some(turn.clone()));
        if stop_pending {
            {
                let mut live = runtime.lock().map_err(|_|"runtime lock")?;
                assert!(live.request_submission_stop(&prepared.turn_id.to_string())?);
                assert!(matches!(live.mark_codex_submission_failed(prepared.task_id,prepared.turn_id,"unknown reply"),Err(DesktopError::SubmissionPending)));
            }
            let effects = crate::runtime::codex_submission::recover_once(&runtime,&client,&pending).await?.ok_or("pending identity must recover")?;
            persist_effects(&mut *runtime.lock().map_err(|_|"runtime lock")?,effects)?;
            assert_eq!(runtime.lock().map_err(|_|"runtime lock")?.codex_interrupt_target(&prepared.turn_id.to_string())?.ok_or("stop target")?.turn_id,turn);
            drop(handoff);
            local_agent_model::stop_codex_task_tree(&client,&thread,&turn).await?;
            tokio::time::timeout(Duration::from_secs(15),async {
                loop {
                    let event=CodexKernelEvent::project(events.next().await.ok_or("stream ended")??)?;
                    let effects=runtime.lock().map_err(|_|"runtime lock")?.project_codex_event(event)?;
                    if persist_effects(&mut *runtime.lock().map_err(|_|"runtime lock")?,effects)?.is_some() { break; }
                }
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
            }).await??;
            let live=runtime.lock().map_err(|_|"runtime lock")?;
            assert_eq!(live.snapshot()?.turns[0].status,"cancelled");
            assert!(live.pending_codex_submissions.is_empty());
            assert!(live.project_leases.is_empty());
            assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?,"before\n");
            assert_eq!(faulty.starts.load(Ordering::SeqCst),1);
            return Ok(());
        }
        let delivery_gate = runtime.lock().map_err(|_|"runtime lock")?.codex_event_gate(client.instance_id());
        let delivery = async {
            let _guard = delivery_gate.lock().await;
            codex_descendants::prepare_approval(&runtime,&client,&request).await
        };
        tokio::pin!(delivery);
        assert!(tokio::time::timeout(Duration::from_millis(25),&mut delivery).await.is_err());
        {
            let mut live = runtime.lock().map_err(|_|"runtime lock")?;
            assert!(live.actions.is_empty());
            live.register_codex_turn(CodexTurnBinding {task_id:prepared.task_id.to_string(),turn_id:prepared.turn_id.to_string(),codex_thread_id:thread.clone(),codex_turn_id:turn.clone()})?;
        }
        drop(handoff);
        let action = tokio::time::timeout(Duration::from_secs(10),&mut delivery).await??.ok_or("approval was incorrectly declined")?;
        assert_eq!(runtime.lock().map_err(|_|"runtime lock")?.actions[&action].status,ActionStatus::Pending);
        assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?,"before\n");
        client.respond(request.request_id.clone(),json!({"decision":"decline"})).await?;
        tokio::time::timeout(Duration::from_secs(15),async {
            loop {
                let event = CodexKernelEvent::project(events.next().await.ok_or("stream ended")??)?;
                if matches!(event,CodexKernelEvent::TurnCompleted{turn_id,..} if turn_id == turn) {
                    return Ok::<_,Box<dyn std::error::Error + Send + Sync>>(());
                }
            }
        }).await??;
        assert_eq!(fs::read_to_string(prepared.project_root.join("file.txt"))?,"before\n");
        Ok::<_,Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let shutdown = client.shutdown().await;
    server.abort();
    outcome?;
    shutdown?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit pinned native kernel; loopback scripted model, no credentials"]
async fn native_completed_patch_survives_restart_without_snapshots() -> TestResult {
    exercise(Ending::Completed, false).await
}

#[tokio::test]
#[ignore = "explicit pinned kernel; actual full native turn buffered before local registration"]
async fn native_late_registration_replays_patch_actions_without_snapshots() -> TestResult {
    exercise(Ending::Completed, true).await
}

#[tokio::test]
#[ignore = "requires explicit pinned native kernel; loopback scripted model, no credentials"]
async fn native_failed_turn_preserves_partial_patch_without_snapshots() -> TestResult {
    exercise(Ending::Failed, false).await
}

#[tokio::test]
#[ignore = "requires explicit pinned native kernel; loopback scripted model, no credentials"]
async fn native_cancelled_turn_preserves_partial_patch_without_snapshots() -> TestResult {
    exercise(Ending::Cancelled, false).await
}
