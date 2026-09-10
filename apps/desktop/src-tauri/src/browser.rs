//! Local browser owned by Simple. Remote webviews have no desktop IPC capability.
//! CDP is limited to our dedicated browser profile and never exposed to the model.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use tauri::{Manager, Webview, WebviewBuilder, WebviewUrl};
use tokio_tungstenite::tungstenite::Message;
#[path = "browser_access.rs"]
pub(crate) mod access;

#[derive(Default)]
pub struct BrowserState {
    sessions: AsyncMutex<HashMap<String, BrowserSession>>,
    viewport: Mutex<Option<(String, BrowserBounds)>>,
    access: Mutex<access::AccessState>,
    tool_gate: AsyncMutex<()>,
    turns: Mutex<HashMap<String, CancellationToken>>,
}

impl BrowserState {
    fn token(&self, turn: &str) -> Result<CancellationToken, String> {
        Ok(self
            .turns
            .lock()
            .map_err(|_| err("浏览器任务状态不可用"))?
            .entry(turn.to_owned())
            .or_default()
            .clone())
    }

    pub(super) fn cancel(&self, turn: &str) {
        if let Ok(token) = self.token(turn) {
            token.cancel();
        }
        if let Ok(mut access) = self.access.lock() {
            access.finish(turn);
        }
    }

    pub(super) fn finish(&self, turn: &str) {
        self.cancel(turn);
        if let Ok(mut access) = self.access.lock() {
            access.finish(turn);
        }
        if let Ok(mut turns) = self.turns.lock() {
            if let Some(token) = turns.remove(turn) {
                token.cancel();
            }
        }
    }
}

struct BrowserSession {
    label: String,
    port: u16,
    target: Option<String>,
    navigation_origin: Arc<Mutex<Option<String>>>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn checked_bounds(
    bounds: &BrowserBounds,
    width: f64,
    height: f64,
) -> Result<BrowserBounds, String> {
    if ![bounds.x, bounds.y, bounds.width, bounds.height]
        .iter()
        .all(|value| value.is_finite())
        || bounds.x < 0.0
        || bounds.y < 0.0
        || bounds.width < 1.0
        || bounds.height < 1.0
        || bounds.x >= width
        || bounds.y >= height
    {
        return Err(err("浏览器面板区域无效"));
    }
    Ok(BrowserBounds {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width.min(width - bounds.x),
        height: bounds.height.min(height - bounds.y),
    })
}

fn place_view(
    app: &AppHandle,
    view: &tauri::Webview,
    bounds: &BrowserBounds,
) -> Result<(), String> {
    let main = app.get_window("main").ok_or_else(|| err("主窗口不可用"))?;
    let size = main.inner_size().map_err(|_| err("主窗口尺寸不可用"))?;
    let bounds = checked_bounds(bounds, size.width as f64, size.height as f64)?;
    view.set_bounds(tauri::Rect {
        position: tauri::PhysicalPosition::new(bounds.x, bounds.y).into(),
        size: tauri::PhysicalSize::new(bounds.width, bounds.height).into(),
    })
    .map_err(|_| err("无法调整网页显示区域"))?;
    view.show().map_err(|_| err("无法显示网页"))
}

#[tauri::command]
pub async fn browser_viewport(
    window: Webview,
    app: AppHandle,
    project_id: String,
    bounds: Option<BrowserBounds>,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err(err("仅主窗口可以调整浏览器面板"));
    }
    let project = state
        .lock()
        .map_err(command_error)?
        .project_root(&project_id)
        .map_err(command_error)?;
    let key = project_key(&project)?;
    let browser = app.state::<BrowserState>();
    let sessions = browser.sessions.lock().await;
    let mut viewport = browser
        .viewport
        .lock()
        .map_err(|_| err("浏览器面板状态不可用"))?;
    if let Some(bounds) = bounds {
        let size = window
            .window()
            .inner_size()
            .map_err(|_| err("主窗口尺寸不可用"))?;
        let bounds = checked_bounds(&bounds, size.width as f64, size.height as f64)?;
        for (project, session) in sessions.iter() {
            if let Some(view) = app.get_webview(&session.label) {
                if project == &key {
                    place_view(&app, &view, &bounds)?;
                } else {
                    view.hide().map_err(|_| err("无法隐藏其他项目网页"))?;
                }
            }
        }
        *viewport = Some((key, bounds));
    } else if viewport
        .as_ref()
        .is_some_and(|(project, _)| project == &key)
    {
        if let Some(view) = sessions
            .get(&key)
            .and_then(|session| app.get_webview(&session.label))
        {
            view.hide().map_err(|_| err("无法收起网页"))?;
        }
        *viewport = None;
    }
    Ok(())
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserRequest {
    action: String,
    url: Option<String>,
    selector: Option<String>,
    text: Option<String>,
    delta: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserResult {
    url: String,
    text: Option<String>,
    attachment: Option<BackendAttachment>,
    #[serde(skip)]
    image: Option<String>,
}

fn err(message: &str) -> String {
    message.to_owned()
}

fn web_url(value: &str) -> Result<tauri::Url, String> {
    if value.len() > 8192 {
        return Err(err("网页地址过长"));
    }
    let url =
        tauri::Url::parse(value).map_err(|_| err("请输入完整的 http:// 或 https:// 网页地址"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || matches!(url.host_str(), Some("tauri.localhost" | "ipc.localhost"))
    {
        return Err(err("仅支持不含账号密码的 HTTP(S) 网页地址"));
    }
    Ok(url)
}

fn validate(request: &BrowserRequest) -> Result<(), String> {
    if !matches!(
        request.action.as_str(),
        "open"
            | "read"
            | "click"
            | "fill"
            | "scroll"
            | "screenshot"
            | "back"
            | "forward"
            | "reload"
            | "close"
    ) {
        return Err(err("不支持的浏览器操作"));
    }
    if request.action == "open" {
        web_url(request.url.as_deref().ok_or_else(|| err("缺少网页地址"))?)?;
    }
    if matches!(request.action.as_str(), "click" | "fill")
        && request
            .selector
            .as_ref()
            .is_none_or(|s| s.is_empty() || s.len() > 512)
    {
        return Err(err("需要有效的元素选择器（最多 512 字节）"));
    }
    if request.text.as_ref().is_some_and(|s| s.len() > 16_384)
        || request.delta.is_some_and(|d| !(-2000..=2000).contains(&d))
    {
        return Err(err("输入文字或滚动范围过大"));
    }
    if request.action == "fill" && request.text.is_none() {
        return Err(err("缺少要输入的文字"));
    }
    Ok(())
}

fn project_key(project: &Path) -> Result<String, String> {
    Ok(hash_bytes(
        fs::canonicalize(project)
            .map_err(|_| err("项目路径不可用"))?
            .to_string_lossy()
            .as_bytes(),
    ))
}

async fn cdp(session: &mut BrowserSession, method: &str, params: Value) -> Result<Value, String> {
    let operation = async {
        if session.target.is_none() {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(2))
                .build()
                .map_err(|_| err("浏览器连接初始化失败"))?;
            for _ in 0..20 {
                let response = client
                    .get(format!("http://127.0.0.1:{}/json/list", session.port))
                    .send()
                    .await;
                if let Ok(response) = response {
                    if let Ok(values) = response.json::<Vec<Value>>().await {
                        let pages: Vec<_> = values.iter().filter(|v| v["type"] == "page").collect();
                        // A dedicated profile has exactly one controlled page. Never guess a target.
                        if pages.len() == 1 {
                            if let Some(ws) = pages[0]["webSocketDebuggerUrl"].as_str() {
                                let parsed =
                                    tauri::Url::parse(ws).map_err(|_| err("浏览器调试地址无效"))?;
                                if parsed.scheme() != "ws"
                                    || parsed.host_str() != Some("127.0.0.1")
                                    || parsed.port() != Some(session.port)
                                    || !parsed.path().starts_with("/devtools/page/")
                                {
                                    return Err(err("拒绝连接非当前浏览器的调试地址"));
                                }
                                session.target = Some(ws.to_owned());
                                break;
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        let address = session
            .target
            .as_deref()
            .ok_or_else(|| err("浏览器尚未就绪，请稍后重试"))?;
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(12 * 1024 * 1024))
            .max_frame_size(Some(12 * 1024 * 1024));
        let (mut socket, _) =
            tokio_tungstenite::connect_async_with_config(address, Some(config), true)
                .await
                .map_err(|_| err("浏览器连接已失效，请关闭网页后重新打开"))?;
        socket
            .send(Message::Text(
                json!({"id":1,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|_| err("浏览器请求发送失败"))?;
        while let Some(message) = socket.next().await {
            let message = message.map_err(|_| err("浏览器连接中断"))?;
            if let Message::Text(text) = message {
                let value: Value =
                    serde_json::from_str(&text).map_err(|_| err("浏览器响应无效"))?;
                if value["id"] == 1 {
                    if value.get("error").is_some() {
                        return Err(err("浏览器拒绝了该操作"));
                    }
                    return Ok(value["result"].clone());
                }
            }
        }
        Err(err("浏览器响应中断"))
    };
    tokio::time::timeout(Duration::from_secs(12), operation)
        .await
        .map_err(|_| err("浏览器操作超时，未自动重试"))?
}

async fn evaluate(session: &mut BrowserSession, expression: String) -> Result<Value, String> {
    let result = cdp(
        session,
        "Runtime.evaluate",
        json!({"expression":expression,"returnByValue":true,"awaitPromise":false}),
    )
    .await?;
    if result.get("exceptionDetails").is_some() {
        return Err(err("页面已改变或元素无法操作，请重新读取页面"));
    }
    Ok(result["result"]["value"].clone())
}

fn script(request: &BrowserRequest, expected_url: &str) -> String {
    let args = json!({"action":request.action,"selector":request.selector,"text":request.text,"delta":request.delta,"expected":expected_url});
    format!(
        "(() => {{ const a = {args}; {} }})()",
        include_str!("browser_actions.js")
    )
}

async fn execute(
    app: &AppHandle,
    project: &Path,
    request: &BrowserRequest,
    expected: Option<&str>,
    cancelled: Option<&CancellationToken>,
    access_revision: Option<u64>,
) -> Result<BrowserResult, String> {
    validate(request)?;
    if !cfg!(windows) {
        return Err(err("当前内置浏览器仅支持 Windows"));
    }
    let key = project_key(project)?;
    let state = app.state::<BrowserState>();
    let mut sessions = state.sessions.lock().await;
    if cancelled.is_some_and(CancellationToken::is_cancelled) {
        return Err(err("任务已停止，未执行浏览器操作"));
    }
    if let Some(revision) = access_revision {
        access::recheck(
            app,
            project,
            expected
                .or(request.url.as_deref())
                .ok_or_else(|| err("缺少授权网页"))?,
            &request.action,
            revision,
        )?;
    }
    if request.action == "close" {
        if let Some(session) = sessions.get_mut(&key) {
            if let Some(window) = app.get_webview(&session.label) {
                window.close().map_err(|_| err("关闭网页失败"))?;
            }
            session.target = None;
        }
        return Ok(BrowserResult {
            url: String::new(),
            text: None,
            attachment: None,
            image: None,
        });
    }
    if !sessions.contains_key(&key) {
        if request.action != "open" {
            return Err(err("请先打开当前项目的网页"));
        }
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| err("浏览器端口不可用"))?;
        let port = listener
            .local_addr()
            .map_err(|_| err("浏览器端口不可用"))?
            .port();
        sessions.insert(
            key.clone(),
            BrowserSession {
                label: format!("simple-browser-{}", Uuid::new_v4().simple()),
                port,
                target: None,
                navigation_origin: Arc::new(Mutex::new(None)),
            },
        );
        drop(listener);
    }
    let session = sessions
        .get_mut(&key)
        .ok_or_else(|| err("浏览器状态不可用"))?;
    // A tool may navigate only within the approved origin. Cross-origin redirects
    // require a new open request and a new grant; explicit user navigation clears it.
    *session
        .navigation_origin
        .lock()
        .map_err(|_| err("导航权限不可用"))? = if access_revision.is_some() {
        Some(
            web_url(
                expected
                    .or(request.url.as_deref())
                    .ok_or_else(|| err("缺少授权网页"))?,
            )?
            .origin()
            .ascii_serialization(),
        )
    } else {
        None
    };
    if request.action == "open" {
        let url = web_url(request.url.as_deref().ok_or_else(|| err("缺少地址"))?)?;
        if let Some(window) = app.get_webview(&session.label) {
            window.navigate(url).map_err(|_| err("网页导航失败"))?;
        } else {
            session.target = None;
            let profile = app
                .path()
                .app_local_data_dir()
                .map_err(|_| err("浏览器数据目录不可用"))?
                .join("browser-profiles")
                .join(&key);
            // Separate profile/process prevents CDP from exposing the privileged main WebView.
            let navigation_origin = session.navigation_origin.clone();
            let builder = WebviewBuilder::new(&session.label, WebviewUrl::External(url))
                .data_directory(profile)
                .on_navigation(move |url| {
                    web_url(url.as_str()).is_ok()
                        && navigation_origin.lock().is_ok_and(|allowed| {
                            allowed
                                .as_ref()
                                .is_none_or(|site| *site == url.origin().ascii_serialization())
                        })
                })
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .on_download(|_, _| false);
            #[cfg(windows)]
            let builder = builder.additional_browser_args(&format!("--remote-debugging-address=127.0.0.1 --remote-debugging-port={} --no-first-run --disable-background-networking", session.port));
            let main = app.get_window("main").ok_or_else(|| err("主窗口不可用"))?;
            let size = main.inner_size().map_err(|_| err("主窗口尺寸不可用"))?;
            // Create offscreen until the trusted UI supplies the sidebar viewport.
            let view = main
                .add_child(
                    builder,
                    tauri::PhysicalPosition::new(size.width as f64 + 1.0, 0.0),
                    tauri::PhysicalSize::new(1.0, 1.0),
                )
                .map_err(|_| err("无法创建右侧浏览器"))?;
            view.hide().map_err(|_| err("无法初始化网页显示区域"))?;
        }
        if let Some(view) = app.get_webview(&session.label) {
            let viewport = state
                .viewport
                .lock()
                .map_err(|_| err("浏览器面板状态不可用"))?;
            if let Some((project, bounds)) =
                viewport.as_ref().filter(|(project, _)| project == &key)
            {
                let _ = project;
                place_view(app, &view, bounds)?;
            }
        }
        let _ = app.emit_to(
            "main",
            "simple-browser-open",
            json!({"projectPath":project.to_string_lossy()}),
        );
        return Ok(BrowserResult {
            url: request.url.clone().unwrap_or_default(),
            text: Some("导航已请求，请 read 检查加载结果。".into()),
            attachment: None,
            image: None,
        });
    }
    let window = app
        .get_webview(&session.label)
        .ok_or_else(|| err("网页已关闭，请重新打开"))?;
    let url = window
        .url()
        .map_err(|_| err("无法读取网页地址"))?
        .to_string();
    web_url(&url)?;
    if expected.is_some_and(|value| value != url) {
        return Err(err("网页地址在确认后发生变化，请重新请求操作"));
    }
    if request.action == "screenshot" {
        let before = cdp(session, "Page.getFrameTree", json!({})).await?;
        let captured = cdp(
            session,
            "Page.captureScreenshot",
            json!({"format":"png","captureBeyondViewport":false}),
        )
        .await?;
        let after = cdp(session, "Page.getFrameTree", json!({})).await?;
        if before["frameTree"]["frame"]["loaderId"] != after["frameTree"]["frame"]["loaderId"]
            || window.url().map_err(|_| err("网页已关闭"))?.as_str() != url
        {
            return Err(err("截图期间页面发生导航，请重新截图"));
        }
        let data = captured["data"]
            .as_str()
            .ok_or_else(|| err("浏览器未返回截图"))?;
        return Ok(BrowserResult {
            url,
            text: None,
            attachment: None,
            image: Some(format!("data:image/png;base64,{data}")),
        });
    }
    let result = evaluate(session, script(request, &url)).await?;
    if let Some(error) = result["error"].as_str() {
        return Err(error.to_owned());
    }
    Ok(BrowserResult {
        url,
        text: Some(result.to_string()),
        attachment: None,
        image: None,
    })
}

async fn current_url(app: &AppHandle, project: &Path) -> Result<String, String> {
    let state = app.state::<BrowserState>();
    let sessions = state.sessions.lock().await;
    let key = project_key(project)?;
    let session = sessions
        .get(&key)
        .ok_or_else(|| err("请先打开当前项目的网页"))?;
    Ok(app
        .get_webview(&session.label)
        .ok_or_else(|| err("网页已关闭"))?
        .url()
        .map_err(|_| err("无法读取网页地址"))?
        .to_string())
}

#[tauri::command]
pub async fn browser_status(
    window: Webview,
    app: AppHandle,
    project_id: String,
    state: State<'_, DesktopState>,
) -> Result<Option<String>, String> {
    if window.label() != "main" {
        return Err(err("仅主窗口可以查看浏览器状态"));
    }
    let project = state
        .lock()
        .map_err(command_error)?
        .project_root(&project_id)
        .map_err(command_error)?;
    let browser = app.state::<BrowserState>();
    let sessions = browser.sessions.lock().await;
    let Some(session) = sessions.get(&project_key(&project)?) else {
        return Ok(None);
    };
    app.get_webview(&session.label)
        .map(|window| {
            window
                .url()
                .map(|url| url.to_string())
                .map_err(|_| err("无法读取网页地址"))
        })
        .transpose()
}

#[tauri::command]
pub async fn browser_command(
    window: Webview,
    app: AppHandle,
    project_id: String,
    request: BrowserRequest,
    state: State<'_, DesktopState>,
) -> Result<BrowserResult, String> {
    if window.label() != "main" {
        return Err(err("仅主窗口可以操作浏览器"));
    }
    let (project, database) = {
        let runtime = state.lock().map_err(command_error)?;
        (
            runtime.project_root(&project_id).map_err(command_error)?,
            runtime.database_path.clone(),
        )
    };
    let mut result = execute(&app, &project, &request, None, None, None).await?;
    if let Some(image) = result.image.take() {
        let encoded = image
            .strip_prefix("data:image/png;base64,")
            .ok_or_else(|| err("截图编码无效"))?;
        if encoded.len() > 6 * 1024 * 1024 {
            return Err(err("截图过大，请缩小浏览器窗口后重试"));
        }
        let bytes = STANDARD.decode(encoded).map_err(|_| err("截图编码无效"))?;
        result.attachment = Some(
            image_attachments::save(&database, &project, "网页截图.png", &bytes)
                .map_err(command_error)?,
        );
    }
    Ok(result)
}

fn live_binding(
    runtime: &DesktopRuntime,
    instance: &str,
    params: &Value,
) -> Result<CodexTurnBinding, String> {
    let thread = params["threadId"]
        .as_str()
        .ok_or_else(|| err("缺少任务标识"))?;
    let turn = params["turnId"]
        .as_str()
        .ok_or_else(|| err("缺少回合标识"))?;
    let binding = runtime
        .codex_action_binding(thread, turn)
        .ok_or_else(|| err("浏览器请求不属于当前任务"))?;
    if !runtime.project_leases.contains_key(&binding.turn_id)
        || !runtime
            .codex_turn_owners
            .get(&binding.turn_id)
            .is_some_and(|owner| owner.instance_id == instance)
        || !runtime.core.snapshot().turns.iter().any(|turn| {
            turn.id.to_string() == binding.turn_id && turn.status == TurnStatus::Running
        })
    {
        return Err(err("任务已结束，浏览器请求已取消"));
    }
    Ok(binding)
}

fn audit(
    runtime: &Arc<Mutex<DesktopRuntime>>,
    binding: &CodexTurnBinding,
    call: &str,
    action: &str,
    phase: &str,
) -> Result<(), String> {
    runtime
        .lock()
        .map_err(|_| err("任务状态不可用"))?
        .storage
        .append_event(NewEvent {
            event_id: Uuid::new_v4().to_string(),
            task_id: binding.task_id.clone(),
            turn_id: Some(binding.turn_id.clone()),
            event_type: format!("browser_action_{phase}"),
            payload: json!({"call_id":call,"action":action}),
            created_at_ms: unix_time_ms().map_err(command_error)?,
        })
        .map(|_| ())
        .map_err(command_error)
}

pub(super) async fn handle_tool(
    app: AppHandle,
    runtime: Arc<Mutex<DesktopRuntime>>,
    client: CodexKernelClient,
    id: Value,
    params: Value,
) {
    let outcome = async {
        if params["tool"] != "simple_browser" || params.get("namespace").is_some_and(|v| !v.is_null()) { return Err(err("不支持的桌面工具")); }
        let request: BrowserRequest = serde_json::from_value(params["arguments"].clone()).map_err(|_| err("浏览器参数无效"))?;
        validate(&request)?;
        if request.action == "close" { return Err(err("请由用户关闭浏览器")); }
        let call = params["callId"].as_str().ok_or_else(|| err("缺少调用标识"))?;
        let browser = app.state::<BrowserState>();
        let (binding, project) = {
            let state = runtime.lock().map_err(|_| err("任务状态不可用"))?;
            let binding = live_binding(&state, client.instance_id(), &params)?;
            // Durable refusal of duplicate callbacks prevents repeated clicks after recovery.
            if state.storage.load_events(&binding.task_id).map_err(command_error)?.iter().any(|e| e.turn_id.as_deref() == Some(binding.turn_id.as_str()) && e.event_type == "browser_action_authorized" && e.payload["call_id"] == call) {
                return Err(err("该浏览器操作已处理，不会自动重复执行"));
            }
            if request.action == "screenshot" {
                let control = state.prepare_codex_control(&binding.task_id).map_err(command_error)?;
                if !control.gateway.supports_images() { return Err(err("当前模型不支持截图输入，请使用图片模型或 read 读取网页文字")); }
            }
            let root = state.task_project_root(parse_task_id(&binding.task_id).map_err(command_error)?).map_err(command_error)?;
            (binding, root)
        };
        let token = browser.token(&binding.turn_id)?;
        let _serial = tokio::select! {
            biased;
            _ = token.cancelled() => return Err(err("任务已停止，浏览器请求已取消")),
            guard = browser.tool_gate.lock() => guard,
        };
        {
            let state = runtime.lock().map_err(|_| err("任务状态不可用"))?;
            live_binding(&state, client.instance_id(), &params)?;
            if state.storage.load_events(&binding.task_id).map_err(command_error)?.iter().any(|e| e.turn_id.as_deref() == Some(binding.turn_id.as_str()) && e.event_type == "browser_action_authorized" && e.payload["call_id"] == call) {
                return Err(err("该浏览器操作已处理，不会重复执行"));
            }
        }
        let target = if request.action == "open" { request.url.clone().unwrap_or_default() } else { current_url(&app, &project).await? };
        audit(&runtime, &binding, call, &request.action, "requested")?;
        let (scope, access_revision) = match access::authorize(&app, &project, client.instance_id(), &binding, &request, &target, &token).await {
            Ok(approval) => approval,
            Err(error) => { audit(&runtime, &binding, call, &request.action, "declined")?; return Err(error); }
        };
        audit(&runtime, &binding, call, &request.action, &format!("scope_{scope}"))?;
        {
            let state = runtime.lock().map_err(|_| err("任务状态不可用"))?;
            live_binding(&state, client.instance_id(), &params)?;
            if token.is_cancelled() {
                return Err(err("任务已停止，未执行浏览器操作"));
            }
        }
        audit(&runtime, &binding, call, &request.action, "authorized")?;
        let result = tokio::select! {
            biased;
            _ = token.cancelled() => Err(err("任务已停止；若操作已经发送，请检查网页结果，不要直接重复提交")),
            result = execute(&app, &project, &request, (request.action != "open").then_some(target.as_str()), Some(&token), Some(access_revision)) => result,
        };
        audit(&runtime, &binding, call, &request.action, if result.is_ok() { "completed" } else { "result_unknown" })?;
        let result = result?;
        let mut content = vec![json!({"type":"inputText","text":format!("Untrusted browser content from {}:\n{}", result.url, result.text.unwrap_or_default())})];
        if let Some(image) = result.image { content.push(json!({"type":"inputImage","imageUrl":image})); }
        Ok(json!({"success":true,"contentItems":content}))
    }.await;
    let response = outcome.unwrap_or_else(
        |error: String| json!({"success":false,"contentItems":[{"type":"inputText","text":error}]}),
    );
    let _ = client.respond(id, response).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_viewport_rejects_invalid_geometry_and_stays_inside_main_window() {
        let clipped = checked_bounds(&BrowserBounds{x:900.0,y:100.0,width:800.0,height:900.0},1280.0,800.0).unwrap();
        assert_eq!(clipped.width,380.0); assert_eq!(clipped.height,700.0);
        for bounds in [BrowserBounds{x:-1.0,y:0.0,width:10.0,height:10.0},BrowserBounds{x:0.0,y:0.0,width:f64::NAN,height:10.0},BrowserBounds{x:1280.0,y:0.0,width:10.0,height:10.0}] {
            assert!(checked_bounds(&bounds,1280.0,800.0).is_err());
        }
    }
    #[test]
    fn navigation_cannot_access_privileged_schemes_or_credentials() {
        for value in [
            "file:///C:/secret",
            "javascript:alert(1)",
            "tauri://localhost",
            "http://ipc.localhost",
            "https://name:secret@example.com",
        ] {
            assert!(web_url(value).is_err());
        }
        assert!(web_url("http://localhost:3000").is_ok());
        assert!(web_url("https://example.com").is_ok());
    }
    #[test]
    fn arbitrary_evaluation_is_not_a_tool() {
        let request: BrowserRequest =
            serde_json::from_value(json!({"action":"evaluate","text":"danger"})).unwrap();
        assert!(validate(&request).is_err());
        assert!(
            serde_json::from_value::<BrowserRequest>(json!({"action":"read","script":"danger"}))
                .is_err()
        );
    }
    #[test]
    fn stopping_cancels_pending_operations_without_reusing_the_token() {
        let state = BrowserState::default();
        let token = state.token("turn").unwrap();
        state.cancel("turn");
        assert!(token.is_cancelled());
        assert!(state.token("turn").unwrap().is_cancelled());
        state.finish("turn");
        assert!(token.is_cancelled());
        assert!(!state.token("different-turn").unwrap().is_cancelled());
    }
}
