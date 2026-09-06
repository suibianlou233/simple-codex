//! Opt-in, headless acceptance against Simple's shipped kernel and gateway.
//! The upstream is scripted; model quality is deliberately NOT claimed here.
//! All writable artifacts are UUID-scoped and retained for inspection.
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use local_agent_model::{
    CodexFeatureBridge, CodexKernelClient, CodexKernelConfig, CodexKernelEvents,
    CodexKernelWireMessage, ResponsesGatewayConfig,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, time::timeout};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[cfg(windows)]
#[tokio::test]
#[ignore = "real shipped kernel and companion patch executable required"]
async fn native_patch_alias_preserves_unicode_multiline_and_exit_status() -> Result<()> {
    let h = Harness::new("native-patch-alias").await?;
    // The app-server lives on the development volume; CODEX_HOME can be on a
    // different volume in production. Native aliases contain no encoded path.
    let aliases = std::fs::read_dir(h.root.join("kernel-home/tmp/arg0"))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let helper = aliases
        .iter()
        .map(|entry| entry.path().join("apply_patch.exe"))
        .find(|path| path.is_file())
        .ok_or("native patch alias missing")?;
    assert!(!helper.with_extension("bat").exists());
    let relocated = std::env::temp_dir()
        .join(format!("simple-patch-{}", uuid::Uuid::new_v4()))
        .join("中文 工具 %literal%");
    std::fs::create_dir_all(&relocated)?;
    let relocated_helper = relocated.join("apply_patch.exe");
    std::fs::copy(&helper, &relocated_helper)?;
    let patch = "*** Begin Patch\n*** Add File: 中文 空格 %name%.txt\n+中文 \"引号\" $value %PATH% & | < >\n+第二行\n*** End Patch";
    let output = std::process::Command::new(&relocated_helper)
        .current_dir(h.root.join("project"))
        .arg(patch)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(h.root.join("project/中文 空格 %name%.txt"))?,
        "中文 \"引号\" $value %PATH% & | < >\n第二行\n"
    );
    let failure = std::process::Command::new(&helper)
        .current_dir(h.root.join("project"))
        .arg("*** Begin Patch\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch")
        .output()?;
    assert!(!failure.status.success());
    assert!(!h.root.join("project/missing.txt").exists());
    h.finish().await
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "real shipped kernel required; commands only touch UUID test fixtures"]
async fn native_command_failure_survives_trailing_output() -> Result<()> {
    let mut h = Harness::new("command-status").await?;
    let t = h.thread("project", "workspace-write", "never").await?;
    let events = h.turn(&t, Some("text(await tools.exec_command({cmd:'cmd.exe /d /c exit 37; Write-Output trailing-output',shell:'powershell.exe',login:false}));"), Value::Null).await?;
    let command = events
        .iter()
        .find(|e| {
            e["method"] == "item/completed" && e["params"]["item"]["type"] == "commandExecution"
        })
        .ok_or("command completion not observed")?;
    // PowerShell 7 raises a NativeCommandExitException (shell exit 1), whereas
    // Windows PowerShell 5.1 reaches the suffix and returns the native exit 37.
    assert!(matches!(
        command["params"]["item"]["exitCode"].as_i64(),
        Some(1 | 37)
    ));
    assert_eq!(command["params"]["item"]["status"], "failed");
    assert!(h.last_output().contains("37"));
    h.finish().await
}

#[derive(Clone, Default)]
struct Upstream {
    scripts: Arc<Mutex<VecDeque<String>>>,
    requests: Arc<Mutex<Vec<Value>>>,
}

#[tokio::test]
#[ignore = "real pinned kernel required; native patch routing, no shell, no paid model"]
async fn native_structured_patch_round_trip_and_rejection() -> Result<()> {
    let mut h = Harness::new("structured-patch").await?;
    let t = h.thread("project", "workspace-write", "on-request").await?;
    let file = h.root.join("project/中文 引号.txt");
    let added = h.turn(&t, Some("PATCH:*** Begin Patch\n*** Add File: 中文 引号.txt\n+中文 \"双引号\" $value %PATH% & |\n+末行\n*** End Patch"), Value::Null).await?;
    assert_eq!(
        std::fs::read_to_string(&file)?,
        "中文 \"双引号\" $value %PATH% & |\n末行\n"
    );
    assert!(added.iter().any(|e| e["method"] == "item/completed"
        && e["params"]["item"]["type"] == "fileChange"
        && e["params"]["item"]["status"] == "completed"));
    assert!(
        !added
            .iter()
            .any(|e| e["params"]["item"]["type"] == "commandExecution")
    );
    h.turn(&t, Some("PATCH:*** Begin Patch\n*** Update File: 中文 引号.txt\n@@\n-末行\n+新末行\n*** End Patch"), Value::Null).await?;
    let expected = std::fs::read_to_string(&file)?;
    assert!(expected.ends_with("新末行\n"));
    h.turn(&t, Some("PATCH:*** Begin Patch\n*** Update File: 中文 引号.txt\n@@\n-不存在的锚点\n+不应写入\n*** End Patch"), Value::Null).await?;
    assert_eq!(std::fs::read_to_string(&file)?, expected);
    assert!(h.last_output().contains("verification failed"));
    // Outside-root writes must still require approval, which this harness denies.
    let outside = h.root.join("outside-patch.txt");
    let patch = format!(
        "PATCH:*** Begin Patch\n*** Add File: {}\n+OUTSIDE_NOT_ALLOWED\n*** End Patch",
        outside.display()
    );
    let denied = h.turn(&t, Some(&patch), Value::Null).await?;
    assert!(!outside.exists());
    assert!(
        denied
            .iter()
            .any(|e| e["method"] == "item/fileChange/requestApproval"
                && e["acceptanceDecision"] == "decline")
    );
    h.turn(
        &t,
        Some("PATCH:*** Begin Patch\n*** Delete File: 中文 引号.txt\n*** End Patch"),
        Value::Null,
    )
    .await?;
    assert!(!file.exists());
    h.finish().await
}

async fn respond(State(state): State<Upstream>, Json(request): Json<Value>) -> impl IntoResponse {
    let is_main = request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["role"] == "user"
                && item
                    .to_string()
                    .contains("explicitly scripted acceptance operation")
        })
    });
    let child_error = !is_main && request["input"].to_string().contains("CHILD_ERROR_PROBE");
    let child_hold = !is_main && request["input"].to_string().contains("CHILD_HOLD_PROBE");
    state
        .requests
        .lock()
        .expect("request recorder lock")
        .push(request);
    if child_error {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"INTENTIONAL_CHILD_MODEL_FAILURE","type":"invalid_request_error"}}))).into_response();
    }
    if child_hold {
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
    let script = if is_main {
        state.scripts.lock().expect("script queue lock").pop_front()
    } else {
        None
    };
    if script.as_deref() == Some("HOLD_UPSTREAM") {
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
    if script.as_deref() == Some("FAIL_UPSTREAM") {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"INTENTIONAL_LOCAL_MODEL_FAILURE","type":"invalid_request_error"}}))).into_response();
    }
    let id = uuid::Uuid::new_v4().to_string();
    let item = if let Some(script) = script {
        if let Some(patch) = script.strip_prefix("PATCH:") {
            json!({"id":format!("patch_{id}"),"type":"custom_tool_call","status":"completed","call_id":format!("call_{id}"),"name":"apply_patch","input":patch})
        } else if let Some(raw) = script.strip_prefix("FUNCTION:") {
            let mut call: Value = serde_json::from_str(raw).expect("valid scripted call");
            call["id"] = json!(format!("fc_{id}"));
            call["type"] = json!("function_call");
            call["call_id"] = json!(format!("call_{id}"));
            call["status"] = json!("completed");
            call
        } else {
            json!({"id":format!("fc_{id}"),"type":"function_call","status":"completed","call_id":format!("call_{id}"),"name":"exec","arguments":json!({"input":script}).to_string()})
        }
    } else {
        json!({"id":format!("msg_{id}"),"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"SCRIPTED_UPSTREAM_DONE","annotations":[]}]})
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
                event["type"].as_str().expect("scripted SSE event type")
            )
        })
        .collect::<String>();
    ([("content-type", "text/event-stream")], body).into_response()
}

struct Harness {
    root: PathBuf,
    client: CodexKernelClient,
    events: CodexKernelEvents,
    upstream: Upstream,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    port: u16,
    evidence: Vec<Value>,
    interrupt_next: bool,
}

impl Harness {
    async fn new(label: &str) -> Result<Self> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/backend-acceptance")
            .join(format!("{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project"))?;
        std::fs::create_dir_all(root.join("other-project"))?;
        std::fs::write(root.join("outside-canary.txt"), "FAKE_OUTSIDE_CANARY")?;
        let upstream = Upstream::default();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let app = Router::new()
            .route("/responses", post(respond))
            .with_state(upstream.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let (client, events) = Self::launch(&root, port).await?;
        println!("EVIDENCE={}", root.display());
        Ok(Self {
            root,
            client,
            events,
            upstream,
            server,
            port,
            evidence: Vec::new(),
            interrupt_next: false,
        })
    }
    async fn launch(
        root: &std::path::Path,
        port: u16,
        ) -> Result<(CodexKernelClient, CodexKernelEvents)> {
        let gateway =
            ResponsesGatewayConfig::new(format!("http://127.0.0.1:{port}"), "deepseek-test", None)
                .with_deepseek_responses();
        let config = if let Some(path) = std::env::var_os("SIMPLE_TEST_KERNEL_MANIFEST") {
            local_agent_model::KernelSelection::Package(local_agent_model::KernelPackage::load(
                std::path::Path::new(&path),
            )?)
            .configuration(root.join("kernel-home"), gateway)
        } else {
            let executable = std::env::var_os("SIMPLE_TEST_CODEX_APP_SERVER")
                .ok_or("SIMPLE_TEST_KERNEL_MANIFEST or SIMPLE_TEST_CODEX_APP_SERVER required")?;
            CodexKernelConfig::new(executable, root.join("kernel-home"), gateway)
        };
        Ok(CodexKernelClient::start(config).await?)
    }
    async fn thread(&self, project: &str, sandbox: &str, approval: &str) -> Result<String> {
        let r = self.client.request("thread/start", json!({"cwd":self.root.join(project),"model":"deepseek-test","approvalPolicy":approval,"approvalsReviewer":"user","sandbox":sandbox,"ephemeral":false,"config":{"web_search":"disabled"}})).await?;
        Ok(r["thread"]["id"]
            .as_str()
            .ok_or("missing thread id")?
            .into())
    }
    async fn turn(
        &mut self,
        thread: &str,
        script: Option<&str>,
        policy: Value,
    ) -> Result<Vec<Value>> {
        if let Some(script) = script {
            self.upstream
                .scripts
                .lock()
                .expect("script queue lock")
                .push_back(script.into());
        }
        let mut params = json!({"threadId":thread,"input":[{"type":"text","text":"Run the explicitly scripted acceptance operation on fake fixtures only.","text_elements":[]}]});
        if !policy.is_null() {
            params["sandboxPolicy"] = policy;
        }
        let started = self.client.request("turn/start", params).await?;
        let turn_id = started["turn"]["id"]
            .as_str()
            .ok_or("missing turn id")?
            .to_owned();
        let stop_task = if self.interrupt_next {
            self.interrupt_next = false;
            let client = self.client.clone();
            let thread = thread.to_owned();
            let turn_id = turn_id.clone();
            Some(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                local_agent_model::stop_codex_task_tree(&client, &thread, &turn_id).await
            }))
        } else {
            None
        };
        let mut records = Vec::new();
        timeout(Duration::from_secs(55), async {
            loop {
                let event = self.events.next().await.ok_or("event stream closed")??;
                match event {
                    CodexKernelWireMessage::Notification { method, params } => {
                        let done = method == "turn/completed" && params["turn"]["id"] == turn_id;
                        records.push(json!({"method":method,"params":params}));
                        if done {
                            break;
                        }
                    }
                    CodexKernelWireMessage::Request { id, method, params } => {
                        // Never elevate a probe: reject every approval request.
                        records.push(
                            json!({"method":method,"params":params,"acceptanceDecision":"decline"}),
                        );
                        self.client
                            .respond(id, json!({"decision":"decline"}))
                            .await?;
                    }
                    _ => {}
                }
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await??;
        if let Some(task) = stop_task {
            task.await??;
        }
        for record in &records {
            if record["method"] == "item/completed" || record.get("acceptanceDecision").is_some() {
                println!("EVENT={record}");
            }
        }
        self.evidence.extend(records.clone());
        std::fs::write(
            self.root.join(format!("requests-{turn_id}.json")),
            serde_json::to_vec_pretty(
                &*self
                    .upstream
                    .requests
                    .lock()
                    .expect("request recorder lock"),
            )?,
        )?;
        self.persist()?;
        Ok(records)
    }
    fn persist(&self) -> Result<()> {
        std::fs::write(
            self.root.join("events.json"),
            serde_json::to_vec_pretty(&self.evidence)?,
        )?;
        std::fs::write(
            self.root.join("scripted-upstream-requests.json"),
            serde_json::to_vec_pretty(
                &*self
                    .upstream
                    .requests
                    .lock()
                    .expect("request recorder lock"),
            )?,
        )?;
        Ok(())
    }
    fn outputs(&self) -> String {
        self.upstream
            .requests
            .lock()
            .expect("request recorder lock")
            .iter()
            .flat_map(|r| r["input"].as_array().into_iter().flatten())
            .filter(|i| {
                i["type"] == "function_call_output" || i["type"] == "custom_tool_call_output"
            })
            .map(|i| i["output"].to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn last_output(&self) -> String {
        self.upstream
            .requests
            .lock()
            .expect("request recorder lock")
            .iter()
            .rev()
            .flat_map(|r| r["input"].as_array().into_iter().flatten().rev())
            .find(|i| i["type"] == "function_call_output" || i["type"] == "custom_tool_call_output")
            .map(|i| i["output"].to_string())
            .unwrap_or_default()
    }
    async fn restart(&mut self) -> Result<()> {
        self.client.shutdown().await?;
        (self.client, self.events) =
            Self::launch(&self.root, self.port).await?;
        Ok(())
    }
    async fn finish(self) -> Result<()> {
        self.persist()?;
        self.client.shutdown().await?;
        self.server.abort();
        Ok(())
    }
}

#[tokio::test]
#[ignore = "real shipped kernel required; retained fixture evidence, no model API usage"]
async fn inspect_enabled_backend_tools() -> Result<()> {
    let mut h = Harness::new("discovery").await?;
    let thread = h.thread("project", "read-only", "on-request").await?;
    h.turn(
        &thread,
        Some("text(ALL_TOOLS.map(t=>({name:t.name,description:t.description})));"),
        json!({"type":"readOnly","networkAccess":false}),
    )
    .await?;
    assert!(h.last_output().contains("memories__read"));
    assert!(h.last_output().contains("multi_agent_v1__wait_agent"));
    CodexFeatureBridge::new(h.client.clone())
        .set_thread_memory(&thread, false)
        .await?;
    h.restart().await?;
    let config = h
        .client
        .request("config/read", json!({"includeLayers":false}))
        .await?;
    std::fs::write(
        h.root.join("effective-config.json"),
        serde_json::to_vec_pretty(&config)?,
    )?;
    h.finish().await
}

#[tokio::test]
#[ignore = "real shipped kernel required; tests use only unique fake files"]
async fn native_project_sandbox_boundary() -> Result<()> {
    let mut h = Harness::new("native-sandbox").await?;
    let t = h.thread("project", "workspace-write", "never").await?;
    let policy = json!({"type":"workspaceWrite","writableRoots":[h.root.join("project")],"networkAccess":true,"excludeTmpdirEnvVar":true,"excludeSlashTmp":true});
    let command = format!(
        "Set-Content -LiteralPath '{}' -Value 'INSIDE_OK'; try {{ Set-Content -LiteralPath '{}' -Value 'OUTSIDE_ESCAPED' }} catch {{ Write-Output 'EXPECTED_OUTSIDE_WRITE_DENIED' }}; Get-Content -LiteralPath '{}'; & powershell.exe -NoProfile -Command \"Set-Content -LiteralPath '{}' -Value 'CHILD_INSIDE_OK'; Set-Content -LiteralPath '{}' -Value 'CHILD_OUTSIDE_ESCAPED'\"",
        h.root.join("project/inside.txt").display(),
        h.root.join("outside-write.txt").display(),
        h.root.join("outside-canary.txt").display(),
        h.root.join("project/child-inside.txt").display(),
        h.root.join("child-outside-write.txt").display()
    );
    let script = format!(
        "text(await tools.exec_command({{cmd:{},shell:'powershell.exe',login:false}}));",
        json!(command)
    );
    h.turn(&t, Some(&script), policy).await?;
    let inside = h.root.join("project/inside.txt").exists();
    let outside = h.root.join("outside-write.txt").exists();
    let child_inside = h.root.join("project/child-inside.txt").exists();
    let child_outside = h.root.join("child-outside-write.txt").exists();
    let outputs = h.outputs();
    std::fs::write(
        h.root.join("boundary-observed.json"),
        serde_json::to_vec_pretty(
            &json!({"inside_write":inside,"outside_write":outside,"child_inside_write":child_inside,"child_outside_write":child_outside,"outside_read":outputs.contains("FAKE_OUTSIDE_CANARY"),"tool_outputs":outputs}),
        )?,
    )?;
    h.finish().await?;
    assert!(inside, "allowed project write did not execute");
    assert!(
        child_inside,
        "allowed child-process project write did not execute"
    );
    assert!(
        !child_outside,
        "child process escaped project write boundary"
    );
    assert!(
        !outside,
        "native Codex tool wrote outside the project without approval"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "real shipped kernel required; only fixture memory is reset"]
async fn memory_save_restart_disable_reset() -> Result<()> {
    let mut h = Harness::new("memory").await?;
    let t = h.thread("project", "read-only", "on-request").await?;
    let filename = "2026-09-05T12-00-00-acceptance.md";
    let add = format!(
        "text(await tools.memories__add_ad_hoc_note({{filename:{},note:'Project A only: currency unit is cents. MEMORY_ACCEPTANCE_CENTS'}}));",
        json!(filename)
    );
    h.turn(&t, Some(&add), Value::Null).await?;
    let path = h
        .root
        .join("kernel-home/memories/extensions/ad_hoc/notes")
        .join(filename);
    let saved = path.exists();
    h.restart().await?;
    let new_thread = h.thread("project", "read-only", "on-request").await?;
    let read = "text(await tools.memories__read({path:'extensions/ad_hoc/notes/2026-09-05T12-00-00-acceptance.md'}));";
    h.upstream
        .requests
        .lock()
        .expect("request recorder lock")
        .clear();
    h.turn(&new_thread, Some(read), Value::Null).await?;
    let retrieved = h.last_output().contains("MEMORY_ACCEPTANCE_CENTS");
    CodexFeatureBridge::new(h.client.clone())
        .set_thread_memory(&new_thread, false)
        .await?;
    h.upstream
        .requests
        .lock()
        .expect("request recorder lock")
        .clear();
    h.turn(&new_thread, Some(read), Value::Null).await?;
    let readable_when_disabled = h.last_output().contains("MEMORY_ACCEPTANCE_CENTS");
    h.restart().await?;
    h.client
        .request("thread/resume", json!({"threadId":new_thread}))
        .await?;
    // Match Simple's durable per-task switch application before each turn.
    CodexFeatureBridge::new(h.client.clone())
        .set_thread_memory(&new_thread, false)
        .await?;
    h.upstream
        .requests
        .lock()
        .expect("request recorder lock")
        .clear();
    h.turn(&new_thread, Some(read), Value::Null).await?;
    let readable_after_disabled_resume = h.last_output().contains("MEMORY_ACCEPTANCE_CENTS");
    CodexFeatureBridge::new(h.client.clone())
        .set_thread_memory(&new_thread, true)
        .await?;
    h.upstream
        .requests
        .lock()
        .expect("request recorder lock")
        .clear();
    h.turn(&new_thread, Some(read), Value::Null).await?;
    let readable_after_reenable = h.last_output().contains("MEMORY_ACCEPTANCE_CENTS");
    let other = h.thread("other-project", "read-only", "on-request").await?;
    h.upstream
        .requests
        .lock()
        .expect("request recorder lock")
        .clear();
    h.turn(&other, Some(read), Value::Null).await?;
    let readable_other_project = h.last_output().contains("MEMORY_ACCEPTANCE_CENTS");
    CodexFeatureBridge::new(h.client.clone())
        .reset_memory()
        .await?;
    h.upstream
        .requests
        .lock()
        .expect("request recorder lock")
        .clear();
    let after_reset = h.thread("project", "read-only", "on-request").await?;
    h.turn(&after_reset, Some(read), Value::Null).await?;
    let reset_removed = !path.exists() && !h.last_output().contains("MEMORY_ACCEPTANCE_CENTS");
    println!(
        "MEMORY_OBSERVED={}",
        json!({"saved":saved,"retrieved_after_restart":retrieved,"readable_when_disabled":readable_when_disabled,"readable_after_disabled_resume":readable_after_disabled_resume,"readable_after_reenable":readable_after_reenable,"readable_other_project":readable_other_project,"reset_removed":reset_removed})
    );
    std::fs::write(
        h.root.join("memory-observed.json"),
        serde_json::to_vec_pretty(
            &json!({"saved":saved,"retrieved_after_restart":retrieved,"readable_when_disabled":readable_when_disabled,"readable_after_disabled_resume":readable_after_disabled_resume,"readable_after_reenable":readable_after_reenable,"readable_other_project":readable_other_project,"reset_removed":reset_removed}),
        )?,
    )?;
    h.finish().await?;
    assert!(
        saved && retrieved && reset_removed,
        "memory persistence/reset invariant failed"
    );
    assert!(
        !readable_when_disabled,
        "disabled thread still read memory via native tools"
    );
    assert!(
        !readable_after_disabled_resume,
        "disabled memory became readable after resume"
    );
    assert!(
        readable_after_reenable,
        "reenabling did not restore memory tools"
    );
    assert!(
        readable_other_project,
        "shared model-profile memory scope changed unexpectedly"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "real shipped kernel required; scripted child models, no external API"]
async fn multi_agent_spawn_wait_close() -> Result<()> {
    let mut h = Harness::new("multi-agent").await?;
    let parent = h.thread("project", "read-only", "on-request").await?;
    let mut ids = Vec::new();
    for task in [
        "Inspect fake calculation task A",
        "Inspect fake validation task B",
    ] {
        let call = format!(
            "FUNCTION:{}",
            json!({"namespace":"multi_agent_v1","name":"spawn_agent","arguments":json!({"message":task,"fork_context":false}).to_string()})
        );
        let records = h.turn(&parent, Some(&call), Value::Null).await?;
        let id = records
            .iter()
            .filter(|r| {
                r["method"] == "item/completed" && r["params"]["item"]["tool"] == "spawnAgent"
            })
            .find_map(|r| {
                r["params"]["item"]["receiverThreadIds"][0]
                    .as_str()
                    .map(str::to_owned)
            });
        if let Some(id) = id {
            ids.push(id);
        }
    }
    println!("CHILD_IDS={ids:?}");
    if ids.len() == 2 {
        h.turn(
            &parent,
            Some(&format!(
                "text(await tools.multi_agent_v1__wait_agent({{targets:{},timeout_ms:10000}}));",
                json!(ids)
            )),
            Value::Null,
        )
        .await?;
        for id in &ids {
            let child = h
                .client
                .request("thread/read", json!({"threadId":id,"includeTurns":true}))
                .await?;
            assert_eq!(child["thread"]["parentThreadId"], parent);
            assert_eq!(child["thread"]["turns"][0]["status"], "completed");
            let rollout = std::fs::read_to_string(
                child["thread"]["path"]
                    .as_str()
                    .ok_or("child rollout path missing")?,
            )?;
            let context = rollout
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .find(|line| line["type"] == "turn_context")
                .ok_or("child context missing")?;
            assert_eq!(context["payload"]["approval_policy"], "on-request");
            assert_eq!(context["payload"]["sandbox_policy"]["type"], "read-only");
            std::fs::write(
                h.root.join(format!("child-{id}.json")),
                serde_json::to_vec_pretty(&child)?,
            )?;
            let close = format!(
                "FUNCTION:{}",
                json!({"namespace":"multi_agent_v1","name":"close_agent","arguments":json!({"target":id}).to_string()})
            );
            h.turn(&parent, Some(&close), Value::Null).await?;
        }
    }
    let outputs = h.outputs();
    std::fs::write(
        h.root.join("multi-agent-observed.json"),
        serde_json::to_vec_pretty(&json!({"child_ids":ids,"outputs":outputs}))?,
    )?;
    h.finish().await?;
    assert_eq!(
        ids.len(),
        2,
        "native multi-agent did not return two child IDs"
    );
    assert_ne!(ids[0], ids[1]);
    assert!(
        outputs.contains("completed"),
        "no completion result returned to parent"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "real shipped kernel required; rejects all approvals"]
async fn native_approval_rejection_has_no_side_effects() -> Result<()> {
    let mut h = Harness::new("approval-reject").await?;
    let t = h.thread("project", "read-only", "on-request").await?;
    let target = h.root.join("project/should-not-exist.txt");
    let cmd = format!(
        "Set-Content -LiteralPath '{}' -Value 'DENIED_PROBE'",
        target.display()
    );
    let script = format!(
        "text(await tools.exec_command({{cmd:{},login:false,sandbox_permissions:'require_escalated',justification:'Decline this fake-file acceptance probe.'}}));",
        json!(cmd)
    );
    let events = h
        .turn(
            &t,
            Some(&script),
            json!({"type":"readOnly","networkAccess":false}),
        )
        .await?;
    let approvals = events
        .iter()
        .filter(|e| e["acceptanceDecision"] == "decline")
        .count();
    let exists = target.exists();
    h.finish().await?;
    assert!(approvals > 0, "no actual approval callback observed");
    assert!(!exists, "declined command produced a side effect");
    Ok(())
}

#[tokio::test]
#[ignore = "real shipped kernel required; unrestricted positive control only writes a unique fixture"]
async fn native_command_fixture_positive_control() -> Result<()> {
    let mut h = Harness::new("command-control").await?;
    let t = h.thread("project", "danger-full-access", "never").await?;
    let target = h.root.join("project/control.txt");
    let cmd = format!(
        "[System.IO.File]::WriteAllText('{}','CONTROL_OK')",
        target.display()
    );
    h.turn(
        &t,
        Some(&format!(
            "text(await tools.exec_command({{cmd:{},login:false}}));",
            json!(cmd)
        )),
        json!({"type":"dangerFullAccess"}),
    )
    .await?;
    let content = std::fs::read_to_string(target);
    h.finish().await?;
    assert_eq!(content?, "CONTROL_OK");
    Ok(())
}

#[tokio::test]
#[ignore = "selected real kernel; isolated restart/history and gateway-secret probe"]
async fn packaged_kernel_text_resume_interrupt_and_environment() -> Result<()> {
    let mut h = Harness::new("package-lifecycle").await?;
    let thread = h.thread("project", "workspace-write", "never").await?;
    let events = h.turn(&thread, None, Value::Null).await?;
    assert!(
        events.iter().any(
            |e| e["method"] == "turn/completed" && e["params"]["turn"]["status"] == "completed"
        )
    );
    assert!(
        events
            .iter()
            .any(|e| e["params"]["item"]["type"] == "agentMessage"
                && e["params"]["item"]["text"] == "SCRIPTED_UPSTREAM_DONE")
    );
    h.restart().await?;
    h.client
        .request(
            "thread/resume",
            json!({"threadId":thread,"cwd":h.root.join("project")}),
        )
        .await?;
    let history = h
        .client
        .request("thread/turns/list", json!({"threadId":thread,"limit":20}))
        .await?;
    assert!(
        !history["data"]
            .as_array()
            .ok_or("missing history")?
            .is_empty()
    );
    let cmd = "if ($env:SIMPLE_MODEL_GATEWAY_TOKEN -or $env:OPENAI_API_KEY -or $env:CODEX_API_KEY) { throw 'SECRET_ENV_LEAK' }; Write-Output 'SECRET_ENV_CLEAN'; Get-Content -LiteralPath outside-missing.txt -ErrorAction SilentlyContinue; exit 0";
    h.turn(
        &thread,
        Some(&format!(
            "text(await tools.exec_command({{cmd:{},login:false}}));",
            json!(cmd)
        )),
        Value::Null,
    )
    .await?;
    assert!(h.last_output().contains("SECRET_ENV_CLEAN"));
    assert!(!h.last_output().contains("SECRET_ENV_LEAK"));
    h.interrupt_next = true;
    let events = h.turn(&thread, Some("HOLD_UPSTREAM"), Value::Null).await?;
    assert!(
        events
            .iter()
            .any(|e| e["method"] == "turn/completed"
                && e["params"]["turn"]["status"] == "interrupted")
    );
    h.finish().await
}

#[tokio::test]
#[ignore = "selected real kernel; intentional local mock model failure"]
async fn packaged_kernel_surfaces_model_failure() -> Result<()> {
    let mut h = Harness::new("package-model-error").await?;
    let thread = h.thread("project", "read-only", "on-request").await?;
    let events = h.turn(&thread, Some("FAIL_UPSTREAM"), Value::Null).await?;
    assert!(
        events
            .iter()
            .any(|e| e["method"] == "turn/completed" && e["params"]["turn"]["status"] == "failed")
    );
    assert!(
        !events
            .iter()
            .any(|e| e["params"]["item"]["type"] == "agentMessage"
                && e["params"]["item"]["text"] == "SCRIPTED_UPSTREAM_DONE")
    );
    h.finish().await
}

#[tokio::test]
#[ignore = "real shipped kernel required; synthetic upstream errors and delayed child response"]
async fn multi_agent_error_and_running_close() -> Result<()> {
    let mut h = Harness::new("multi-agent-errors").await?;
    let parent = h.thread("project", "read-only", "on-request").await?;
    let mut errors_seen = false;
    let mut running_closed = false;
    for marker in ["CHILD_ERROR_PROBE", "CHILD_HOLD_PROBE"] {
        let call = format!(
            "FUNCTION:{}",
            json!({"namespace":"multi_agent_v1","name":"spawn_agent","arguments":json!({"message":marker,"fork_context":false}).to_string()})
        );
        let records = h.turn(&parent, Some(&call), Value::Null).await?;
        let id = records
            .iter()
            .filter(|r| {
                r["method"] == "item/completed" && r["params"]["item"]["tool"] == "spawnAgent"
            })
            .find_map(|r| {
                r["params"]["item"]["receiverThreadIds"][0]
                    .as_str()
                    .map(str::to_owned)
            })
            .ok_or("spawn failed")?;
        if marker == "CHILD_ERROR_PROBE" {
            h.turn(&parent, Some(&format!("text(await tools.multi_agent_v1__wait_agent({{targets:[{}],timeout_ms:10000}}));", json!(id))), Value::Null).await?;
            errors_seen = h.last_output().contains("errored");
        }
        let close = format!(
            "FUNCTION:{}",
            json!({"namespace":"multi_agent_v1","name":"close_agent","arguments":json!({"target":id}).to_string()})
        );
        h.turn(&parent, Some(&close), Value::Null).await?;
        if marker == "CHILD_HOLD_PROBE" {
            let output = h.last_output();
            running_closed = output.contains("running") || output.contains("pending_init");
        }
        let child = h
            .client
            .request("thread/read", json!({"threadId":id,"includeTurns":true}))
            .await?;
        if marker == "CHILD_HOLD_PROBE" {
            assert_eq!(child["thread"]["turns"][0]["status"], "interrupted");
        }
        std::fs::write(
            h.root.join(format!("{marker}.json")),
            serde_json::to_vec_pretty(&child)?,
        )?;
    }
    std::fs::write(
        h.root.join("error-cancel-observed.json"),
        serde_json::to_vec_pretty(
            &json!({"child_error_returned":errors_seen,"closed_while_running":running_closed}),
        )?,
    )?;
    h.finish().await?;
    assert!(errors_seen, "child error did not reach parent");
    assert!(running_closed, "did not close a still-running child");
    Ok(())
}

#[tokio::test]
#[ignore = "real shipped kernel required; tests native task-stop propagation to a running child"]
async fn parent_interrupt_stops_running_child() -> Result<()> {
    let mut h = Harness::new("parent-cancel").await?;
    let parent = h.thread("project", "read-only", "on-request").await?;
    let spawn = format!(
        "FUNCTION:{}",
        json!({"namespace":"multi_agent_v1","name":"spawn_agent","arguments":json!({"message":"CHILD_HOLD_PROBE","fork_context":false}).to_string()})
    );
    let events = h.turn(&parent, Some(&spawn), Value::Null).await?;
    let id = events
        .iter()
        .filter(|r| r["method"] == "item/completed" && r["params"]["item"]["tool"] == "spawnAgent")
        .find_map(|r| {
            r["params"]["item"]["receiverThreadIds"][0]
                .as_str()
                .map(str::to_owned)
        })
        .ok_or("spawn failed")?;
    h.interrupt_next = true;
    let interrupted = h.turn(&parent, Some("HOLD_UPSTREAM"), Value::Null).await?;
    let parent_stopped = interrupted
        .iter()
        .any(|r| r["method"] == "turn/completed" && r["params"]["turn"]["status"] == "interrupted");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let child = loop {
        let child = h
            .client
            .request("thread/read", json!({"threadId":id,"includeTurns":true}))
            .await?;
        if child["thread"]["turns"][0]["status"] != "inProgress"
            || tokio::time::Instant::now() >= deadline
        {
            break child;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    let child_status = child["thread"]["turns"][0]["status"]
        .as_str()
        .unwrap_or("missing")
        .to_owned();
    std::fs::write(
        h.root.join("cancel-observed.json"),
        serde_json::to_vec_pretty(
            &json!({"parent_interrupted":parent_stopped,"child_status":child_status,"child":child}),
        )?,
    )?;
    // Always explicitly close the fixture child, regardless of the assertion.
    let close = format!(
        "FUNCTION:{}",
        json!({"namespace":"multi_agent_v1","name":"close_agent","arguments":json!({"target":id}).to_string()})
    );
    h.turn(&parent, Some(&close), Value::Null).await?;
    h.finish().await?;
    assert!(parent_stopped, "parent interrupt did not complete");
    assert_eq!(
        child_status, "interrupted",
        "root task stopped but its child did not stop"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "real pinned kernel required; completed root, active child, unrelated active task"]
async fn finished_parent_stop_reaches_running_child() -> Result<()> {
    let mut h = Harness::new("finished-parent-stop").await?;
    let parent = h.thread("project", "read-only", "on-request").await?;
    let spawn = format!(
        "FUNCTION:{}",
        json!({"namespace":"multi_agent_v1","name":"spawn_agent",
        "arguments":json!({"message":"CHILD_HOLD_PROBE","fork_context":false}).to_string()})
    );
    let events = h.turn(&parent, Some(&spawn), Value::Null).await?;
    let child = events
        .iter()
        .filter(|record| {
            record["method"] == "item/completed" && record["params"]["item"]["tool"] == "spawnAgent"
        })
        .find_map(|record| {
            record["params"]["item"]["receiverThreadIds"][0]
                .as_str()
                .map(str::to_owned)
        })
        .ok_or("actual child spawn required")?;
    let root_turn = events
        .iter()
        .find(|record| {
            record["method"] == "turn/completed"
                && record["params"]["turn"]["status"] == "completed"
        })
        .and_then(|record| record["params"]["turn"]["id"].as_str())
        .ok_or("completed root required")?;
    let unrelated = h.thread("project", "read-only", "on-request").await?;
    let started = h
        .client
        .request(
            "turn/start",
            json!({"threadId":unrelated,
        "input":[{"type":"text","text":"CHILD_HOLD_PROBE unrelated task","text_elements":[]}]}),
        )
        .await?;
    let unrelated_turn = started["turn"]["id"]
        .as_str()
        .ok_or("unrelated turn")?
        .to_owned();
    let result = async {
        timeout(Duration::from_secs(5), async {
            loop {
                let child_state = h
                    .client
                    .request("thread/read", json!({"threadId":child}))
                    .await?;
                let other_state = h
                    .client
                    .request("thread/read", json!({"threadId":unrelated}))
                    .await?;
                if child_state["thread"]["status"]["type"] == "active"
                    && other_state["thread"]["status"]["type"] == "active"
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await??;
        let mut watch = local_agent_model::CodexTaskTreeWatch::new(&parent)?;
        assert!(
            !watch.is_quiet(&h.client).await?,
            "completed parent is not a quiet tree while its child works"
        );
        local_agent_model::stop_codex_task_tree(&h.client, &parent, root_turn).await?;
        assert!(
            !watch.is_quiet(&h.client).await?,
            "one quiet observation is not sufficient"
        );
        assert!(watch.is_quiet(&h.client).await?);
        let child_state = h
            .client
            .request("thread/read", json!({"threadId":child}))
            .await?;
        assert!(matches!(
            child_state["thread"]["status"]["type"].as_str(),
            Some("idle" | "notLoaded")
        ));
        let child_turns = h
            .client
            .request(
                "thread/turns/list",
                json!({"threadId":child,"limit":1,"sortDirection":"desc","itemsView":"notLoaded"}),
            )
            .await?;
        assert_eq!(child_turns["data"][0]["status"], "interrupted");
        let other_state = h
            .client
            .request("thread/read", json!({"threadId":unrelated}))
            .await?;
        assert_eq!(
            other_state["thread"]["status"]["type"], "active",
            "unrelated task must remain running"
        );
        let parent_turns = h
            .client
            .request(
                "thread/turns/list",
                json!({"threadId":parent,"limit":1,"sortDirection":"desc","itemsView":"notLoaded"}),
            )
            .await?;
        assert_eq!(parent_turns["data"][0]["status"], "completed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let cleanup =
        local_agent_model::stop_codex_task_tree(&h.client, &unrelated, &unrelated_turn).await;
    h.finish().await?;
    result?;
    cleanup?;
    Ok(())
}
