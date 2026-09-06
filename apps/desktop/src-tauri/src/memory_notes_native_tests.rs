use super::*;
use crate::runtime::memory_notes::SaveMemoryNotesInput;

async fn answer(
    State(requests): State<Arc<Mutex<Vec<Value>>>>,
    Json(request): Json<Value>,
) -> axum::response::Response {
    requests.lock().expect("test capture").push(request);
    let id = Uuid::new_v4().to_string();
    let item = json!({"id":format!("msg_{id}"),"type":"message","role":"assistant","phase":"final_answer",
        "status":"completed","content":[{"type":"output_text","text":"NOTES_FIXTURE_DONE","annotations":[]}]});
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
#[ignore = "explicit pinned kernel; captures actual model requests, no paid inference or credentials"]
async fn native_manual_memory_corrections_are_user_input_and_next_turn_switch_is_respected()
-> TestResult {
    let executable =
        std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER").ok_or("explicit kernel required")?;
    let (temp, mut runtime, prepared) = fixture();
    finish(&mut runtime, &prepared, ProjectedTurnStatus::Completed);
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    let app = Router::new()
        .route("/responses", post(answer))
        .with_state(Arc::clone(&requests));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let started = CodexKernelClient::start(CodexKernelConfig::new(
        executable,
        temp.path().join("kernel-home"),
        ResponsesGatewayConfig::new(endpoint, "notes-fixture", None).with_deepseek_responses(),
    ))
    .await;
    let (client, mut events) = match started {
        Ok(value) => value,
        Err(error) => {
            server.abort();
            return Err(error.into());
        }
    };
    let result = async {
        let start = client.request("thread/start",json!({"cwd":prepared.project_root,"model":"notes-fixture",
            "approvalPolicy":"on-request","sandbox":"read-only","ephemeral":false,
            "config":{"web_search":"disabled","features.memories":false}})).await?;
        let thread = start["thread"]["id"].as_str().ok_or("thread id")?;
        for (index, content) in ["MANUAL_FACT_V1", "MANUAL_CORRECTED_V2", ""].into_iter().enumerate() {
            if !content.is_empty() {
                runtime.save_memory_notes(SaveMemoryNotesInput {task_id:prepared.task_id.to_string(),content:content.into(),expected_revision:index as i64})?;
            } else { runtime.task_memory_enabled.insert(prepared.task_id,false); }
            let input = runtime.codex_user_input(&prepared)?;
            let response = client.request("turn/start",json!({"threadId":thread,"clientUserMessageId":Uuid::new_v4().to_string(),"input":input})).await?;
            let turn = response["turn"]["id"].as_str().ok_or("turn id")?;
            tokio::time::timeout(Duration::from_secs(30),async {
                loop {
                    let wire = events.next().await.ok_or("native events closed")??;
                    if let CodexKernelWireMessage::Request{id,..} = wire {
                        client.respond(id,json!({"decision":"decline"})).await?;
                        return Err("unexpected tool approval".into());
                    }
                    if let CodexKernelEvent::TurnCompleted{turn_id,status,..} = CodexKernelEvent::project(wire)?
                        && turn_id == turn {
                        assert_eq!(status,CodexTurnStatus::Completed); return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());
                    }
                }
            }).await??;
            let captured = requests.lock().map_err(|_|"capture poisoned")?;
            assert_eq!(captured.len(),index+1,"one actual request per scripted turn");
            let items = captured.last().ok_or("request missing")?["input"].as_array().ok_or("input array")?;
            let last_user = items.iter().rev().find(|item|item["role"] == "user").ok_or("user input")?.to_string();
            assert!(last_user.contains(&prepared.content));
            if !content.is_empty() { assert!(last_user.contains(content)); }
            else { assert!(!last_user.contains("MANUAL_"),"disabled memory is not appended to the new input; prior history intentionally remains"); }
            assert!(items.iter().filter(|item| item["role"] != "user").all(|item| !item.to_string().contains("MANUAL_")),"manual notes never become system/developer authority");
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let shutdown = client.shutdown().await;
    server.abort();
    result?;
    shutdown?;
    Ok(())
}
