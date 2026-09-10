use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::body::Bytes;
use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::header::ACCEPT;
use axum::http::header::AUTHORIZATION;
use axum::http::header::CACHE_CONTROL;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::post;
#[cfg(test)]
use reqwest::Client;
use reqwest::Url;
use serde_json::Value;
use serde_json::json;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ApiKey;
use crate::ChatDialect;
use crate::responses_chat_translation::prepare_chat_request;
use crate::responses_chat_translation::translated_response_body;
use crate::responses_tool_translation::ResponsesToolTranslation;

const DEFAULT_MAX_REQUEST_BYTES: usize = 48 * 1024 * 1024;
const DEEPSEEK_VISION_MODEL: &str = "deepseek-v4-flash-vision-exp";
const SIMPLE_MODEL_GATEWAY_URL_ENV_VAR: &str = "SIMPLE_MODEL_GATEWAY_URL";
const SIMPLE_MODEL_GATEWAY_TOKEN_ENV_VAR: &str = "SIMPLE_MODEL_GATEWAY_TOKEN";
const SIMPLE_MODEL_ALIAS_ENV_VAR: &str = "SIMPLE_MODEL_ALIAS";

#[path = "responses_gateway_routes.rs"]
mod routes;

#[cfg(test)]
#[path = "responses_gateway_routes_tests.rs"]
mod route_tests;

/// Configuration owned by Simple for one immutable model route in a gateway.
///
/// The upstream credential never appears in the returned gateway handle. The
/// embedded Codex process receives only [`ResponsesGatewayHandle::base_url`]
/// and [`ResponsesGatewayHandle::client_token`].
#[derive(Clone, Debug)]
pub struct ResponsesGatewayConfig {
    pub upstream_base_url: String,
    pub upstream_model: String,
    pub codex_model_alias: String,
    pub upstream_api_key: Option<ApiKey>,
    pub upstream_protocol: ResponsesGatewayUpstream,
    pub upstream_timeout: Duration,
    pub max_request_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResponsesGatewayUpstream {
    #[default]
    Responses,
    /// Native Responses, with unsupported custom tools wrapped as functions.
    DeepSeekResponses,
    ChatCompletions {
        dialect: ChatDialect,
    },
}

impl ResponsesGatewayConfig {
    /// Capability of the Simple route, including native DeepSeek image routing.
    pub fn supports_images(&self) -> bool {
        self.upstream_protocol == ResponsesGatewayUpstream::DeepSeekResponses
            || self
                .upstream_model
                .trim()
                .eq_ignore_ascii_case(DEEPSEEK_VISION_MODEL)
    }

    pub fn with_model_alias(mut self, alias: impl Into<String>) -> Self {
        self.codex_model_alias = alias.into();
        self
    }

    pub fn new(
        upstream_base_url: impl Into<String>,
        upstream_model: impl Into<String>,
        upstream_api_key: Option<ApiKey>,
    ) -> Self {
        let upstream_model = upstream_model.into();
        Self {
            upstream_base_url: upstream_base_url.into(),
            codex_model_alias: upstream_model.clone(),
            upstream_model,
            upstream_api_key,
            upstream_protocol: ResponsesGatewayUpstream::Responses,
            upstream_timeout: Duration::from_secs(120),
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
        }
    }

    pub fn with_chat_completions(mut self, dialect: ChatDialect) -> Self {
        self.upstream_protocol = ResponsesGatewayUpstream::ChatCompletions { dialect };
        self
    }

    pub fn with_deepseek_responses(mut self) -> Self {
        self.upstream_protocol = ResponsesGatewayUpstream::DeepSeekResponses;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.upstream_timeout = timeout;
        self
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct GatewayClientToken(String);

impl GatewayClientToken {
    /// Exposes the short-lived loopback credential so the Simple launcher can
    /// pass it directly to the embedded Codex child. Never persist this value.
    pub fn expose_for_child(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for GatewayClientToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GatewayClientToken([REDACTED])")
    }
}

#[derive(Debug, Error)]
pub enum ResponsesGatewayError {
    #[error("模型网关配置无效：{0}")]
    InvalidConfiguration(String),
    #[error("无法监听本机模型网关端口")]
    Bind(#[source] io::Error),
    #[error("无法创建模型网关 HTTP 客户端")]
    Client(#[source] reqwest::Error),
    #[error("模型网关任务异常结束")]
    Join(#[source] tokio::task::JoinError),
    #[error("模型网关服务异常结束")]
    Serve(#[source] io::Error),
}

pub struct ResponsesGatewayHandle {
    base_url: String,
    client_token: GatewayClientToken,
    model_alias: String,
    supports_images: bool,
    routes: Arc<routes::Routes>,
    shutdown: CancellationToken,
    task: Option<JoinHandle<Result<(), io::Error>>>,
}

impl ResponsesGatewayHandle {
    pub async fn start(config: ResponsesGatewayConfig) -> Result<Self, ResponsesGatewayError> {
        let supports_images = config.supports_images();
        let model_alias = config.codex_model_alias.trim().to_owned();
        let routes = Arc::new(routes::Routes::new(config.clone())?);
        let client_token = GatewayClientToken(format!(
            "{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        ));
        let state = Arc::new(GatewayState {
            routes: Arc::clone(&routes),
            client_token: client_token.clone(),
        });
        let app = Router::new()
            .route("/v1/responses", post(forward_responses))
            .layer(DefaultBodyLimit::max(config.max_request_bytes))
            .with_state(state);
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(ResponsesGatewayError::Bind)?;
        let address = listener.local_addr().map_err(ResponsesGatewayError::Bind)?;
        let base_url = format!("http://127.0.0.1:{}/v1", address.port());
        let shutdown = CancellationToken::new();
        let shutdown_signal = shutdown.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal.cancelled_owned())
                .await
        });

        Ok(Self {
            base_url,
            client_token,
            model_alias,
            supports_images,
            routes,
            shutdown,
            task: Some(task),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn model_alias(&self) -> &str {
        &self.model_alias
    }

    pub(crate) fn supports_images(&self) -> bool {
        self.supports_images
    }

    pub fn client_token(&self) -> &GatewayClientToken {
        &self.client_token
    }

    /// Registers an immutable model revision without restarting the native
    /// history owner. Unknown revisions fail closed; existing ones cannot change.
    pub fn register_model(
        &self,
        config: ResponsesGatewayConfig,
    ) -> Result<(), ResponsesGatewayError> {
        self.routes.register(config)
    }

    /// Applies the complete secret-minimized launch contract to the embedded
    /// Codex child without returning a loggable map containing the token.
    pub fn configure_codex_command(&self, command: &mut std::process::Command) {
        crate::proxy_policy::preserve_local_gateway_routing(command);
        command
            .env(SIMPLE_MODEL_GATEWAY_URL_ENV_VAR, &self.base_url)
            .env(
                SIMPLE_MODEL_GATEWAY_TOKEN_ENV_VAR,
                self.client_token.expose_for_child(),
            )
            .env(SIMPLE_MODEL_ALIAS_ENV_VAR, &self.model_alias)
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY");
    }

    pub async fn shutdown(mut self) -> Result<(), ResponsesGatewayError> {
        self.shutdown.cancel();
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        task.await
            .map_err(ResponsesGatewayError::Join)?
            .map_err(ResponsesGatewayError::Serve)
    }
}

impl fmt::Debug for ResponsesGatewayHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponsesGatewayHandle")
            .field("base_url", &self.base_url)
            .field("client_token", &self.client_token)
            .field("model_alias", &self.model_alias)
            .finish_non_exhaustive()
    }
}

impl Drop for ResponsesGatewayHandle {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

struct GatewayState {
    routes: Arc<routes::Routes>,
    client_token: GatewayClientToken,
}

// Inspect only structured Responses input, never strings, tool schemas or arguments.
// History and tool screenshots count: sending them to a text model would lose context.
fn input_contains_images(payload: &serde_json::Map<String, Value>) -> bool {
    fn image_parts(value: &Value) -> bool {
        value
            .as_array()
            .is_some_and(|parts| parts.iter().any(|part| part["type"] == "input_image"))
    }
    payload
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| match item.get("type").and_then(Value::as_str) {
                    Some("message") | None => image_parts(&item["content"]),
                    Some("function_call_output" | "custom_tool_call_output") => {
                        image_parts(&item["output"])
                    }
                    _ => false,
                })
        })
}

fn request_model<'a>(
    model: &'a str,
    protocol: ResponsesGatewayUpstream,
    payload: &serde_json::Map<String, Value>,
) -> &'a str {
    if protocol == ResponsesGatewayUpstream::DeepSeekResponses && input_contains_images(payload) {
        DEEPSEEK_VISION_MODEL
    } else {
        model
    }
}

async fn forward_responses(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !authorized(&headers, &state.client_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let mut payload = match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(payload)) => payload,
        Ok(_) | Err(_) => return StatusCode::UNPROCESSABLE_ENTITY.into_response(),
    };
    let Some(alias) = payload.get("model").and_then(Value::as_str) else {
        return (StatusCode::UNPROCESSABLE_ENTITY, "缺少模型配置版本").into_response();
    };
    // Pin the complete route for this request before any asynchronous I/O.
    let state = match state.routes.get(alias) {
        Ok(Some(route)) => route,
        Ok(None) => {
            return (StatusCode::UNPROCESSABLE_ENTITY, "模型配置版本不可用").into_response();
        }
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let upstream_model = request_model(&state.upstream_model, state.upstream_protocol, &payload);
    let tool_translation = if state.upstream_protocol == ResponsesGatewayUpstream::DeepSeekResponses
    {
        match ResponsesToolTranslation::prepare(&mut payload) {
            Ok(translation) => Some(translation),
            Err(error) => return (StatusCode::UNPROCESSABLE_ENTITY, error).into_response(),
        }
    } else {
        None
    };
    let prepared_chat = match state.upstream_protocol {
        ResponsesGatewayUpstream::Responses | ResponsesGatewayUpstream::DeepSeekResponses => {
            payload.insert("model".to_owned(), Value::String(upstream_model.to_owned()));
            None
        }
        ResponsesGatewayUpstream::ChatCompletions { dialect } => {
            match prepare_chat_request(&payload, &state.upstream_model, dialect) {
                Ok(prepared) => Some(prepared),
                Err(error) => return (StatusCode::UNPROCESSABLE_ENTITY, error).into_response(),
            }
        }
    };
    let passthrough_body = Value::Object(payload);
    let request_body = prepared_chat
        .as_ref()
        .map(|prepared| &prepared.body)
        .unwrap_or(&passthrough_body);

    let mut request = state
        .client
        .post(state.upstream_url.clone())
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "text/event-stream")
        .json(request_body);
    if let Some(api_key) = state.upstream_api_key.as_ref() {
        request = request.bearer_auth(api_key.expose());
    }
    let upstream = match request.send().await {
        Ok(response) => response,
        Err(error) => return upstream_transport_error_response(error),
    };

    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let translated = (prepared_chat.is_some() || tool_translation.is_some()) && status.is_success();
    let response_body = match prepared_chat {
        Some(prepared) if status.is_success() => translated_response_body(upstream, prepared),
        None if status.is_success() && tool_translation.is_some() => match tool_translation {
            Some(translation) => translation.response_body(upstream),
            None => Body::from_stream(upstream.bytes_stream()),
        },
        Some(_) | None => Body::from_stream(upstream.bytes_stream()),
    };
    let mut response = Response::new(response_body);
    *response.status_mut() = status;
    if translated {
        response.headers_mut().insert(
            CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        response.headers_mut().insert(
            CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache"),
        );
    } else {
        copy_response_header(&upstream_headers, response.headers_mut(), CONTENT_TYPE);
        copy_response_header(&upstream_headers, response.headers_mut(), CACHE_CONTROL);
    }
    for name in ["x-request-id", "openai-processing-ms", "openai-version"] {
        if let Some(value) = upstream_headers.get(name) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    response
}

fn upstream_transport_error_response(error: reqwest::Error) -> Response {
    let (code, summary) = if error.is_timeout() {
        ("simple_upstream_timeout", "连接模型服务超时")
    } else if error.is_connect() {
        ("simple_upstream_connect_failed", "无法连接模型服务")
    } else if error.is_builder() {
        ("simple_upstream_request_invalid", "模型请求无法发送")
    } else {
        ("simple_upstream_transport_failed", "模型连接失败")
    };
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": {
                "type": "simple_gateway_error",
                "code": code,
                "message": format!("{summary}：{error}")
            }
        })),
    )
        .into_response()
}

/// Keep stream failure categories without exposing URL, headers or response data.
pub(crate) fn upstream_stream_failure(
    error: &eventsource_stream::EventStreamError<reqwest::Error>,
) -> (&'static str, &'static str) {
    match error {
        eventsource_stream::EventStreamError::Transport(error) if error.is_timeout() => (
            "simple_upstream_stream_timeout",
            "模型响应超时：已连接模型服务，但响应流未在配置时限内完成。请检查已有工具结果后再决定是否重试。",
        ),
        eventsource_stream::EventStreamError::Transport(_) => (
            "simple_upstream_stream_disconnected",
            "模型响应流连接中断，请检查网络后重试。",
        ),
        _ => (
            "simple_upstream_stream_invalid",
            "模型服务返回了无法解析的事件流。",
        ),
    }
}

fn copy_response_header(
    source: &HeaderMap,
    destination: &mut HeaderMap,
    name: axum::http::HeaderName,
) {
    if let Some(value) = source.get(&name) {
        destination.insert(name, value.clone());
    }
}

fn authorized(headers: &HeaderMap, expected: &GatewayClientToken) -> bool {
    let Some(value) = headers.get(AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_eq(token.as_bytes(), expected.0.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let difference = left
        .iter()
        .zip(right.iter())
        .fold(0_u8, |accumulator, (left, right)| {
            accumulator | (left ^ right)
        });
    difference == 0
}

fn responses_url(base_url: &str) -> Result<Url, ResponsesGatewayError> {
    let mut url = validated_upstream_url(base_url)?;
    let path = url.path().trim_end_matches('/');
    let responses_path = if path.ends_with("/responses") {
        path.to_owned()
    } else if path.is_empty() {
        "/responses".to_owned()
    } else {
        format!("{path}/responses")
    };
    url.set_path(&responses_path);
    Ok(url)
}

fn chat_completions_url(base_url: &str) -> Result<Url, ResponsesGatewayError> {
    let mut url = validated_upstream_url(base_url)?;
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/chat/completions") {
        url.set_path(&format!("{path}/chat/completions"));
    }
    Ok(url)
}

fn validated_upstream_url(base_url: &str) -> Result<Url, ResponsesGatewayError> {
    let url = Url::parse(base_url.trim()).map_err(|_| {
        ResponsesGatewayError::InvalidConfiguration("模型接口地址格式错误".to_owned())
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ResponsesGatewayError::InvalidConfiguration(
            "模型接口地址必须使用 http 或 https 并包含主机名".to_owned(),
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ResponsesGatewayError::InvalidConfiguration(
            "模型接口地址不能包含凭据、查询参数或片段".to_owned(),
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use axum::extract::State;
    use axum::http::HeaderMap;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::post;

    use super::*;

    #[test]
    fn image_routing_ignores_text_and_other_provider_routes() {
        let fake = json!({"input":[{"role":"user","content":[{"type":"input_text","text":"{\"type\":\"input_image\"}"}]}],"tools":[{"type":"input_image"}]});
        assert!(!input_contains_images(fake.as_object().unwrap()));
        let picture = json!({"input":[{"role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,fixture"}]}]});
        assert_eq!(
            request_model(
                "other-model",
                ResponsesGatewayUpstream::Responses,
                picture.as_object().unwrap()
            ),
            "other-model"
        );
        assert!(
            !ResponsesGatewayConfig::new("http://localhost", "other-model", None).supports_images()
        );
        assert!(
            ResponsesGatewayConfig::new("http://localhost", "deepseek-v4-flash", None)
                .with_deepseek_responses()
                .supports_images()
        );
    }

    #[tokio::test]
    async fn deepseek_auto_vision_preserves_images_and_returns_to_text_without_mutating_route() {
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let gateway = ResponsesGatewayHandle::start(
            ResponsesGatewayConfig::new(
                server.uri(),
                "deepseek-v4-flash",
                Some(ApiKey::new("fixture-key").unwrap()),
            )
            .with_model_alias("same-session-route")
            .with_deepseek_responses(),
        )
        .await
        .unwrap();
        let cases = [
            (
                json!([{"role":"user","content":[{"type":"input_text","text":"hello"}]}]),
                "deepseek-v4-flash",
            ),
            (
                json!([{"role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,fixture"}]}]),
                DEEPSEEK_VISION_MODEL,
            ),
            (
                json!([{"type":"function_call_output","call_id":"browser","output":[{"type":"input_image","image_url":"data:image/png;base64,screenshot"}]}]),
                DEEPSEEK_VISION_MODEL,
            ),
            (
                json!([{"type":"custom_tool_call_output","call_id":"browser","output":[{"type":"input_image","image_url":"data:image/png;base64,custom"}]}]),
                DEEPSEEK_VISION_MODEL,
            ),
            (
                json!([{"role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,history"}]},{"role":"assistant","content":[{"type":"output_text","text":"seen"}]},{"role":"user","content":[{"type":"input_text","text":"follow up"}]}]),
                DEEPSEEK_VISION_MODEL,
            ),
            (
                json!([{"role":"user","content":"text after image-free context"}]),
                "deepseek-v4-flash",
            ),
        ];
        for (input, model) in cases {
            let expected_input = input.clone();
            Mock::given(method("POST")).and(path("/responses"))
                .and(header("authorization", "Bearer fixture-key"))
                .and(body_partial_json(json!({"model":model,"input":expected_input})))
                .respond_with(ResponseTemplate::new(200).insert_header("content-type", "text/event-stream")
                    .set_body_string("data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n"))
                .expect(1).mount(&server).await;
            let response = Client::new()
                .post(format!("{}/responses", gateway.base_url()))
                .bearer_auth(gateway.client_token().expose_for_child())
                .json(&json!({"model":"same-session-route","stream":true,"input":input}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let _ = response.text().await.unwrap();
        }
        server.verify().await;
        gateway.shutdown().await.unwrap();
    }

    #[derive(Clone)]
    struct UpstreamState {
        calls: Arc<AtomicUsize>,
        expected_key: String,
    }

    async fn upstream(
        State(state): State<UpstreamState>,
        headers: HeaderMap,
        body: Bytes,
    ) -> impl IntoResponse {
        state.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some(format!("Bearer {}", state.expected_key).as_str())
        );
        let payload: Value = serde_json::from_slice(&body).expect("valid gateway JSON");
        assert_eq!(payload["model"], "real-model");
        (
            StatusCode::OK,
            [(
                CONTENT_TYPE,
                axum::http::HeaderValue::from_static("text/event-stream"),
            )],
            "data: {\"type\":\"response.completed\"}\n\n",
        )
    }

    async fn start_upstream() -> (String, Arc<AtomicUsize>, JoinHandle<Result<(), io::Error>>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind fake upstream");
        let port = listener.local_addr().expect("fake upstream address").port();
        let state = UpstreamState {
            calls: Arc::clone(&calls),
            expected_key: "upstream-secret".to_owned(),
        };
        let app = Router::new()
            .route("/v1/responses", post(upstream))
            .with_state(state);
        let task = tokio::spawn(async move { axum::serve(listener, app).await });
        (format!("http://127.0.0.1:{port}/v1"), calls, task)
    }

    async fn chat_upstream(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer upstream-secret")
        );
        let payload: Value = serde_json::from_slice(&body).expect("valid chat JSON");
        assert_eq!(payload["model"], "deepseek-chat");
        assert_eq!(payload["messages"][0]["role"], "system");
        assert_eq!(payload["messages"][1]["content"], "inspect");
        assert_eq!(payload["tools"][0]["function"]["name"], "read_file");
        assert_eq!(payload["thinking"]["type"], "disabled");
        (
            StatusCode::OK,
            [(
                CONTENT_TYPE,
                axum::http::HeaderValue::from_static("text/event-stream"),
            )],
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\",\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"README.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":4}}\n\n",
                "data: [DONE]\n\n"
            ),
        )
    }

    async fn start_chat_upstream() -> (String, JoinHandle<Result<(), io::Error>>) {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind fake chat upstream");
        let port = listener.local_addr().expect("chat upstream address").port();
        let app = Router::new().route("/v1/chat/completions", post(chat_upstream));
        let task = tokio::spawn(async move { axum::serve(listener, app).await });
        (format!("http://127.0.0.1:{port}/v1"), task)
    }

    #[tokio::test]
    async fn deepseek_native_responses_round_trips_custom_exec() {
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let raw = "text('中文');";
        let item = json!({"id":"i","type":"function_call","name":"exec","call_id":"c","arguments":json!({"input":raw}).to_string()});
        let frames = [
            json!({"type":"response.output_item.added","item":{"id":"i","type":"function_call","name":"exec","call_id":"c","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","item_id":"i","delta":"{\"input\":"}),
            json!({"type":"response.output_item.done","item":item}),
            json!({"type":"response.completed","response":{"output":[item],"usage":{"input_tokens":3,"output_tokens":4}}}),
        ].iter().map(|value| format!("data: {value}\n\n")).collect::<String>();
        Mock::given(method("POST"))
            .and(path("/responses"))
            .and(body_partial_json(
                json!({"model":"real-model","tools":[{"type":"function","name":"exec"}]}),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(frames),
            )
            .expect(1)
            .mount(&server)
            .await;
        let gateway = ResponsesGatewayHandle::start(
            ResponsesGatewayConfig::new(server.uri(), "real-model", None)
                .with_model_alias("alias")
                .with_deepseek_responses(),
        )
        .await
        .expect("valid test fixture");
        let response = Client::new().post(format!("{}/responses", gateway.base_url()))
            .bearer_auth(gateway.client_token().expose_for_child())
            .json(&json!({"model":"alias","stream":true,"input":[],"tools":[{"type":"custom","name":"exec"}]}))
            .send().await.expect("valid test fixture");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.expect("valid test fixture");
        let events = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|line| serde_json::from_str::<Value>(line).expect("valid test fixture"))
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 3);
        assert_eq!(events[1]["item"]["type"], "custom_tool_call");
        assert_eq!(events[1]["item"]["input"], raw);
        assert_eq!(events[2]["response"]["usage"]["output_tokens"], 4);
        gateway.shutdown().await.expect("valid test fixture");
    }

    #[tokio::test]
    async fn requires_ephemeral_client_token_and_rewrites_model() {
        let (upstream_url, calls, upstream_task) = start_upstream().await;
        let gateway = ResponsesGatewayHandle::start(
            ResponsesGatewayConfig::new(
                upstream_url,
                "real-model",
                Some(ApiKey::new("upstream-secret").expect("valid key")),
            )
            .with_model_alias("codex-alias"),
        )
        .await
        .expect("start gateway");
        let endpoint = format!("{}/responses", gateway.base_url());
        let client = Client::new();

        let unauthorized = client
            .post(&endpoint)
            .json(&serde_json::json!({"model": "codex-alias", "stream": true}))
            .send()
            .await
            .expect("unauthorized request");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let authorized = client
            .post(&endpoint)
            .bearer_auth(gateway.client_token().expose_for_child())
            .json(&serde_json::json!({"model": "codex-alias", "stream": true}))
            .send()
            .await
            .expect("authorized request");
        assert_eq!(authorized.status(), StatusCode::OK);
        assert_eq!(
            authorized.text().await.expect("SSE body"),
            "data: {\"type\":\"response.completed\"}\n\n"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let mut command = std::process::Command::new("embedded-codex");
        command.env("OPENAI_API_KEY", "must-be-removed");
        gateway.configure_codex_command(&mut command);
        let child_environment = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            child_environment[SIMPLE_MODEL_GATEWAY_URL_ENV_VAR].as_deref(),
            Some(gateway.base_url())
        );
        assert_eq!(
            child_environment[SIMPLE_MODEL_GATEWAY_TOKEN_ENV_VAR].as_deref(),
            Some(gateway.client_token().expose_for_child())
        );
        assert_eq!(
            child_environment[SIMPLE_MODEL_ALIAS_ENV_VAR].as_deref(),
            Some("codex-alias")
        );
        assert_eq!(child_environment["OPENAI_API_KEY"], None);

        gateway.shutdown().await.expect("shutdown gateway");
        upstream_task.abort();
    }

    #[tokio::test]
    async fn translates_codex_responses_over_authenticated_http_gateway() {
        let (upstream_url, upstream_task) = start_chat_upstream().await;
        let gateway = ResponsesGatewayHandle::start(
            ResponsesGatewayConfig::new(
                upstream_url,
                "deepseek-chat",
                Some(ApiKey::new("upstream-secret").expect("valid key")),
            )
            .with_model_alias("simple-alias")
            .with_chat_completions(ChatDialect::DeepSeek),
        )
        .await
        .expect("start chat gateway");

        let response = Client::new()
            .post(format!("{}/responses", gateway.base_url()))
            .bearer_auth(gateway.client_token().expose_for_child())
            .json(&serde_json::json!({
                "model": "simple-alias",
                "instructions": "system",
                "input": [{
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "inspect"}]
                }],
                "tools": [{
                    "type": "function",
                    "name": "read_file",
                    "description": "read",
                    "parameters": {"type": "object"}
                }],
                "tool_choice": "auto",
                "parallel_tool_calls": false,
                "reasoning": {"effort": "none"},
                "stream": true
            }))
            .send()
            .await
            .expect("gateway response");
        assert_eq!(response.status(), StatusCode::OK);
        let stream = response.text().await.expect("translated SSE");
        assert!(stream.contains("response.output_text.delta"));
        assert!(stream.contains("\"type\":\"function_call\""));
        assert!(stream.contains("README.md"));
        assert!(stream.contains("response.completed"));
        assert!(stream.contains("\"total_tokens\":14"));

        gateway.shutdown().await.expect("shutdown gateway");
        upstream_task.abort();
    }

    #[tokio::test]
    async fn transport_failures_return_a_diagnostic_error_body() {
        let gateway = ResponsesGatewayHandle::start(ResponsesGatewayConfig::new(
            "http://127.0.0.1:1",
            "model",
            None,
        ))
        .await
        .expect("start gateway");

        let response = Client::new()
            .post(format!("{}/responses", gateway.base_url()))
            .bearer_auth(gateway.client_token().expose_for_child())
            .json(&serde_json::json!({
                "model": "model",
                "input": "hello",
                "stream": true
            }))
            .send()
            .await
            .expect("gateway response");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let payload: Value = response.json().await.expect("diagnostic JSON");
        assert_eq!(payload["error"]["code"], "simple_upstream_connect_failed");
        assert!(
            payload["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("127.0.0.1"))
        );

        gateway.shutdown().await.expect("shutdown gateway");
    }

    #[test]
    fn secrets_are_redacted_and_unsafe_urls_are_rejected() {
        let key = ApiKey::new("upstream-secret").expect("valid key");
        let config = ResponsesGatewayConfig::new("https://models.example/v1", "model", Some(key));
        assert!(!format!("{config:?}").contains("upstream-secret"));

        for invalid in [
            "ftp://models.example/v1",
            "https://user:pass@models.example/v1",
            "https://models.example/v1?token=secret",
            "https://models.example/v1#fragment",
        ] {
            assert!(
                responses_url(invalid).is_err(),
                "accepted unsafe URL: {invalid}"
            );
        }
    }

    #[test]
    fn client_token_debug_is_redacted() {
        let token = GatewayClientToken("child-secret".to_owned());
        let rendered = format!("{token:?}");
        assert!(rendered.contains("REDACTED"));
        assert!(!rendered.contains("child-secret"));
    }
}
