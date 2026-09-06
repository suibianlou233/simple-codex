use std::time::Duration;

use std::collections::VecDeque;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    ApiKey, FinishReason, Message, ModelAdapter, ModelCapabilities, ModelError, ModelEvent,
    ModelEventStream, ModelRequest, ReasoningMode, Role, ToolCallDelta, ToolExchange, Usage,
};

const MAX_ERROR_BODY_BYTES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatDialect {
    Standard,
    DeepSeek,
    Qwen,
}

#[derive(Debug, Clone)]
pub struct ChatCompletionsConfig {
    pub base_url: String,
    pub model: String,
    pub dialect: ChatDialect,
    pub timeout: Duration,
}

pub struct ChatCompletionsAdapter {
    client: Client,
    endpoint: Url,
    model: String,
    dialect: ChatDialect,
    api_key: Option<ApiKey>,
}

impl ChatCompletionsAdapter {
    pub fn new(config: ChatCompletionsConfig, api_key: Option<ApiKey>) -> Result<Self, ModelError> {
        let endpoint = chat_completions_url(&config.base_url)?;
        if config.model.trim().is_empty() {
            return Err(ModelError::InvalidEndpoint(
                "model name must not be empty".to_owned(),
            ));
        }
        let client = Client::builder()
            .timeout(config.timeout)
            // A configured endpoint must not silently send the request to a
            // different host through an HTTP redirect.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            endpoint,
            model: config.model,
            dialect: config.dialect,
            api_key,
        })
    }

    fn request_body(&self, request: &ModelRequest) -> Value {
        let mut messages = request
            .messages
            .iter()
            .map(chat_message)
            .collect::<Vec<_>>();
        for exchange in &request.tool_history {
            messages.extend(tool_exchange_messages(exchange));
        }
        let mut body = Map::from_iter([
            ("model".to_owned(), Value::String(self.model.clone())),
            ("messages".to_owned(), Value::Array(messages)),
            ("stream".to_owned(), Value::Bool(true)),
            (
                "stream_options".to_owned(),
                json!({ "include_usage": true }),
            ),
        ]);
        if let Some(maximum) = request.max_output_tokens {
            body.insert("max_tokens".to_owned(), Value::from(maximum));
        }
        if !request.tools.is_empty() {
            body.insert(
                "tools".to_owned(),
                Value::Array(
                    request
                        .tools
                        .iter()
                        .map(|tool| {
                            json!({
                                "type": "function",
                                "function": {
                                    "name": tool.name,
                                    "description": tool.description,
                                    "parameters": tool.parameters
                                }
                            })
                        })
                        .collect(),
                ),
            );
            body.insert("tool_choice".to_owned(), Value::String("auto".to_owned()));
        }
        apply_reasoning_control(&mut body, self.dialect, request.reasoning);
        Value::Object(body)
    }
}

#[async_trait]
impl ModelAdapter for ChatCompletionsAdapter {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            streaming: true,
            reasoning_control: self.dialect != ChatDialect::Standard,
            tool_calls: true,
        }
    }

    async fn stream(
        &self,
        request: ModelRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelEventStream, ModelError> {
        let reasoning = request.reasoning;
        let mut pending = self
            .client
            .post(self.endpoint.clone())
            .json(&self.request_body(&request));
        if let Some(api_key) = &self.api_key {
            pending = pending.bearer_auth(api_key.expose());
        }
        let pending = pending.send();
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(ModelError::Cancelled),
            response = pending => response?,
        };
        let status = response.status();
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read error response".to_owned());
            return Err(ModelError::Http {
                status: status.as_u16(),
                message: truncate_utf8(&message, MAX_ERROR_BODY_BYTES),
            });
        }

        let source = response.bytes_stream().eventsource();
        let stream = futures_util::stream::unfold(
            (
                source,
                cancellation,
                reasoning,
                None,
                false,
                VecDeque::<Result<ModelEvent, ModelError>>::new(),
            ),
            |(
                mut source,
                cancellation,
                reasoning,
                mut finish_reason,
                mut completed,
                mut pending,
            )| async move {
                loop {
                    if let Some(item) = pending.pop_front() {
                        return Some((
                            item,
                            (
                                source,
                                cancellation,
                                reasoning,
                                finish_reason,
                                completed,
                                pending,
                            ),
                        ));
                    }
                    if completed {
                        return None;
                    }
                    let next = tokio::select! {
                        () = cancellation.cancelled() => {
                            completed = true;
                            return Some((
                                Err(ModelError::Cancelled),
                                (source, cancellation, reasoning, finish_reason, completed, pending),
                            ));
                        }
                        next = source.next() => next,
                    };
                    match next {
                        None => {
                            completed = true;
                            return finish_reason.map(|reason| {
                                (
                                    Ok(ModelEvent::Completed(reason)),
                                    (
                                        source,
                                        cancellation,
                                        reasoning,
                                        finish_reason,
                                        completed,
                                        pending,
                                    ),
                                )
                            });
                        }
                        Some(Err(error)) => {
                            completed = true;
                            return Some((
                                Err(ModelError::InvalidStream(error.to_string())),
                                (
                                    source,
                                    cancellation,
                                    reasoning,
                                    finish_reason,
                                    completed,
                                    pending,
                                ),
                            ));
                        }
                        Some(Ok(event)) if event.data.trim() == "[DONE]" => {
                            completed = true;
                            let reason = finish_reason.unwrap_or(FinishReason::Stop);
                            return Some((
                                Ok(ModelEvent::Completed(reason)),
                                (
                                    source,
                                    cancellation,
                                    reasoning,
                                    finish_reason,
                                    completed,
                                    pending,
                                ),
                            ));
                        }
                        Some(Ok(event)) => match parse_chunk(&event.data, reasoning) {
                            Ok(parsed) => {
                                if let Some(reason) = parsed.finish_reason {
                                    finish_reason = Some(reason);
                                }
                                pending.extend(parsed.events.into_iter().map(Ok));
                            }
                            Err(error) => {
                                completed = true;
                                return Some((
                                    Err(error),
                                    (
                                        source,
                                        cancellation,
                                        reasoning,
                                        finish_reason,
                                        completed,
                                        pending,
                                    ),
                                ));
                            }
                        },
                    }
                }
            },
        );
        Ok(Box::pin(stream))
    }
}

#[derive(Debug)]
struct ParsedChunk {
    events: Vec<ModelEvent>,
    finish_reason: Option<FinishReason>,
}

fn parse_chunk(data: &str, reasoning: ReasoningMode) -> Result<ParsedChunk, ModelError> {
    let chunk: ChatChunk =
        serde_json::from_str(data).map_err(|error| ModelError::InvalidStream(error.to_string()))?;
    let mut events = Vec::new();
    if let Some(usage) = chunk.usage {
        let reasoning_tokens = usage
            .completion_tokens_details
            .and_then(|details| details.reasoning_tokens)
            .unwrap_or_default();
        if reasoning == ReasoningMode::Off && reasoning_tokens > 0 {
            return Err(ModelError::UnexpectedReasoning);
        }
        events.push(ModelEvent::Usage(Usage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            reasoning_tokens,
        }));
    }
    let Some(choice) = chunk.choices.into_iter().next() else {
        return Ok(ParsedChunk {
            events,
            finish_reason: None,
        });
    };
    if reasoning == ReasoningMode::Off
        && choice
            .delta
            .reasoning_content
            .as_deref()
            .is_some_and(|content| !content.is_empty())
    {
        return Err(ModelError::UnexpectedReasoning);
    }
    if let Some(content) = choice.delta.content.filter(|content| !content.is_empty()) {
        events.push(ModelEvent::TextDelta(content));
    }
    if !choice.delta.tool_calls.is_empty() {
        events.push(ModelEvent::ToolCallDelta(
            choice
                .delta
                .tool_calls
                .into_iter()
                .map(|call| ToolCallDelta {
                    index: call.index,
                    id: call.id,
                    name: call
                        .function
                        .as_ref()
                        .and_then(|function| function.name.clone()),
                    arguments: call
                        .function
                        .and_then(|function| function.arguments)
                        .unwrap_or_default(),
                })
                .collect(),
        ));
    }
    Ok(ParsedChunk {
        events,
        finish_reason: choice.finish_reason.as_deref().map(parse_finish_reason),
    })
}

fn apply_reasoning_control(
    body: &mut Map<String, Value>,
    dialect: ChatDialect,
    reasoning: ReasoningMode,
) {
    match dialect {
        ChatDialect::Standard => {}
        ChatDialect::DeepSeek => {
            let mode = if reasoning == ReasoningMode::Off {
                "disabled"
            } else {
                "enabled"
            };
            body.insert("thinking".to_owned(), json!({ "type": mode }));
            if reasoning != ReasoningMode::Off {
                body.insert(
                    "reasoning_effort".to_owned(),
                    Value::String(
                        if reasoning == ReasoningMode::Low {
                            "low"
                        } else {
                            "high"
                        }
                        .to_owned(),
                    ),
                );
            }
        }
        ChatDialect::Qwen => {
            body.insert(
                "enable_thinking".to_owned(),
                Value::Bool(reasoning != ReasoningMode::Off),
            );
        }
    }
}

fn chat_message(message: &Message) -> Value {
    let role = match message.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    json!({ "role": role, "content": message.content })
}

fn tool_exchange_messages(exchange: &ToolExchange) -> Vec<Value> {
    let mut messages = vec![json!({
        "role": "assistant",
        "content": Value::Null,
        "tool_calls": exchange.calls.iter().map(|call| json!({
            "id": call.id,
            "type": "function",
            "function": { "name": call.name, "arguments": call.arguments }
        })).collect::<Vec<_>>()
    })];
    messages.extend(exchange.results.iter().map(|result| {
        json!({
            "role": "tool",
            "tool_call_id": result.tool_call_id,
            "content": result.content
        })
    }));
    messages
}

fn parse_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        "tool_calls" | "function_call" => FinishReason::ToolCalls,
        _ => FinishReason::Other,
    }
}

fn chat_completions_url(base_url: &str) -> Result<Url, ModelError> {
    let mut url = Url::parse(base_url.trim())
        .map_err(|error| ModelError::InvalidEndpoint(error.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ModelError::InvalidEndpoint(format!(
            "unsupported URL scheme `{}`",
            url.scheme()
        )));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ModelError::InvalidEndpoint(
            "credentials, query strings, and fragments are not allowed".to_owned(),
        ));
    }
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/chat/completions") {
        url.set_path(&format!("{path}/chat/completions"));
    }
    Ok(url)
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut boundary = maximum_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…", &value[..boundary])
}

#[derive(Debug, Deserialize)]
struct ChatChunk {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    delta: ChatDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatToolCallDelta>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<ChatFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct ChatFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    completion_tokens_details: Option<CompletionTokenDetails>,
}

#[derive(Debug, Deserialize)]
struct CompletionTokenDetails {
    reasoning_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::StreamExt;
    use serde_json::Value;
    use tokio_util::sync::CancellationToken;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::{
        ApiKey, ChatCompletionsAdapter, ChatCompletionsConfig, ChatDialect, FinishReason, Message,
        ModelAdapter, ModelEvent, ModelRequest, ReasoningMode, Role, ToolDefinition,
    };

    fn request() -> ModelRequest {
        ModelRequest {
            messages: vec![Message {
                role: Role::User,
                content: "只回复一个字：快".to_owned(),
            }],
            reasoning: ReasoningMode::Off,
            max_output_tokens: Some(64),
            tools: Vec::new(),
            tool_history: Vec::new(),
        }
    }

    fn adapter(server: &MockServer, dialect: ChatDialect) -> ChatCompletionsAdapter {
        ChatCompletionsAdapter::new(
            ChatCompletionsConfig {
                base_url: format!("{}/v1", server.uri()),
                model: "test-model".to_owned(),
                dialect,
                timeout: Duration::from_secs(5),
            },
            Some(ApiKey::new("not-a-real-secret").expect("test key should be accepted")),
        )
        .expect("adapter should be created")
    }

    fn response() -> ResponseTemplate {
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"快\",\"reasoning_content\":null},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":1,\"completion_tokens_details\":{\"reasoning_tokens\":0}}}\n\n",
                "data: [DONE]\n\n"
            ))
    }

    #[tokio::test]
    async fn deepseek_explicitly_disables_thinking_and_streams_text() {
        let server = MockServer::start().await;
        let body: Value = serde_json::json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "只回复一个字：快"}],
            "stream": true,
            "stream_options": {"include_usage": true},
            "max_tokens": 64,
            "thinking": {"type": "disabled"}
        });
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer not-a-real-secret"))
            .and(body_json(body))
            .respond_with(response())
            .expect(1)
            .mount(&server)
            .await;

        let mut stream = adapter(&server, ChatDialect::DeepSeek)
            .stream(request(), CancellationToken::new())
            .await
            .expect("stream should start");
        let events = stream
            .by_ref()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("stream should be valid");
        assert!(events.contains(&ModelEvent::TextDelta("快".to_owned())));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ModelEvent::Usage(_)))
        );
    }

    #[tokio::test]
    async fn qwen_explicitly_disables_thinking() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_json(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "只回复一个字：快"}],
                "stream": true,
                "stream_options": {"include_usage": true},
                "max_tokens": 64,
                "enable_thinking": false
            })))
            .respond_with(response())
            .expect(1)
            .mount(&server)
            .await;

        let _stream = adapter(&server, ChatDialect::Qwen)
            .stream(request(), CancellationToken::new())
            .await
            .expect("stream should start");
    }

    #[tokio::test]
    async fn reasoning_content_is_rejected_when_disabled() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(concat!(
                        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hidden\"},\"finish_reason\":null}]}\n\n",
                        "data: [DONE]\n\n"
                    )),
            )
            .mount(&server)
            .await;
        let mut stream = adapter(&server, ChatDialect::DeepSeek)
            .stream(request(), CancellationToken::new())
            .await
            .expect("stream should start");
        let first = stream
            .next()
            .await
            .expect("stream should return a result")
            .expect_err("reasoning should be rejected");
        assert!(matches!(first, crate::ModelError::UnexpectedReasoning));
    }

    #[tokio::test]
    async fn tool_definitions_and_streamed_calls_round_trip() {
        let server = MockServer::start().await;
        let mut request = request();
        request.tools.push(ToolDefinition {
            name: "read_file".to_owned(),
            description: "Read a file".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        });
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_json(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "只回复一个字：快"}],
                "stream": true,
                "stream_options": {"include_usage": true},
                "max_tokens": 64,
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "description": "Read a file",
                        "parameters": {
                            "type": "object",
                            "properties": { "path": { "type": "string" } },
                            "required": ["path"]
                        }
                    }
                }],
                "tool_choice": "auto"
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(concat!(
                        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]},\"finish_reason\":null}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"正在读取\",\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"README.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":6,\"completion_tokens_details\":{\"reasoning_tokens\":0}}}\n\n",
                        "data: [DONE]\n\n"
                    )),
            )
            .expect(1)
            .mount(&server)
            .await;
        let mut stream = adapter(&server, ChatDialect::Standard)
            .stream(request, CancellationToken::new())
            .await
            .expect("stream should start");
        let events = stream
            .by_ref()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("tool stream should be valid");
        assert!(events.iter().any(|event| matches!(
            event,
            ModelEvent::ToolCallDelta(deltas) if deltas[0].id.as_deref() == Some("call_1")
        )));
        assert!(events.contains(&ModelEvent::TextDelta("正在读取".to_owned())));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ModelEvent::Usage(usage) if usage.input_tokens == 20))
        );
        assert!(events.contains(&ModelEvent::Completed(FinishReason::ToolCalls)));
    }
}
