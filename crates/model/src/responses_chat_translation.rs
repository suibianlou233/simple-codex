use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io;
use std::pin::Pin;

use axum::body::Bytes;
use eventsource_stream::Event;
use eventsource_stream::EventStreamError;
use eventsource_stream::Eventsource;
use futures_util::Stream;
use futures_util::StreamExt;
use reqwest::Response;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;

use crate::ChatDialect;

type UpstreamEvents =
    Pin<Box<dyn Stream<Item = Result<Event, EventStreamError<reqwest::Error>>> + Send + 'static>>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolKind {
    Function,
    Custom,
    NamespacedFunction { namespace: String, name: String },
}

pub(crate) struct PreparedChatRequest {
    pub body: Value,
    tool_kinds: HashMap<String, ToolKind>,
    reasoning_enabled: bool,
}

pub(crate) fn prepare_chat_request(
    request: &Map<String, Value>,
    upstream_model: &str,
    dialect: ChatDialect,
) -> Result<PreparedChatRequest, String> {
    if request.get("stream").and_then(Value::as_bool) != Some(true) {
        return Err("Simple 嵌入式模型网关只接受流式 Responses 请求".to_owned());
    }

    let mut messages = Vec::new();
    if let Some(instructions) = request
        .get("instructions")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        messages.push(json!({"role": "system", "content": instructions}));
    }
    let input = request
        .get("input")
        .and_then(Value::as_array)
        .ok_or_else(|| "Responses 请求缺少 input 数组".to_owned())?;
    for item in input {
        translate_input_item(item, &mut messages)?;
    }

    let (tools, tool_kinds) = translate_tools(request.get("tools"))?;
    let reasoning_effort = request
        .get("reasoning")
        .and_then(|value| value.get("effort"))
        .and_then(Value::as_str);
    let reasoning_enabled = reasoning_effort.is_some_and(|effort| effort != "none");

    let mut body = Map::from_iter([
        ("model".to_owned(), Value::String(upstream_model.to_owned())),
        ("messages".to_owned(), Value::Array(messages)),
        ("stream".to_owned(), Value::Bool(true)),
        ("stream_options".to_owned(), json!({"include_usage": true})),
    ]);
    if !tools.is_empty() {
        body.insert("tools".to_owned(), Value::Array(tools));
        body.insert(
            "tool_choice".to_owned(),
            request
                .get("tool_choice")
                .cloned()
                .unwrap_or_else(|| Value::String("auto".to_owned())),
        );
        if let Some(parallel) = request.get("parallel_tool_calls").and_then(Value::as_bool) {
            body.insert("parallel_tool_calls".to_owned(), Value::Bool(parallel));
        }
    }
    if let Some(maximum) = request.get("max_output_tokens").and_then(Value::as_u64) {
        body.insert("max_tokens".to_owned(), Value::from(maximum));
    }
    if let Some(format) = request
        .get("text")
        .and_then(|value| value.get("format"))
        .filter(|value| !value.is_null())
    {
        body.insert("response_format".to_owned(), translate_text_format(format)?);
    }
    apply_reasoning(&mut body, dialect, reasoning_effort, reasoning_enabled);

    Ok(PreparedChatRequest {
        body: Value::Object(body),
        tool_kinds,
        reasoning_enabled,
    })
}

fn translate_input_item(item: &Value, messages: &mut Vec<Value>) -> Result<(), String> {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "Responses input item 缺少 type".to_owned())?;
    match item_type {
        "message" => {
            let role = item
                .get("role")
                .and_then(Value::as_str)
                .ok_or_else(|| "Responses message 缺少 role".to_owned())?;
            let content = translate_message_content(
                item.get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "Responses message 缺少 content 数组".to_owned())?,
            )?;
            messages.push(json!({"role": role, "content": content}));
        }
        "agent_message" => {
            let content = translate_message_content(
                item.get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "agent_message 缺少 content 数组".to_owned())?,
            )?;
            messages.push(json!({"role": "assistant", "content": content}));
        }
        "function_call" | "custom_tool_call" => {
            let call_id = required_string(item, "call_id", item_type)?;
            let name = required_string(item, "name", item_type)?;
            let chat_name = item.get("namespace").and_then(Value::as_str).map_or_else(
                || name.to_owned(),
                |namespace| flatten_tool_name(namespace, name),
            );
            let arguments = if item_type == "function_call" {
                required_string(item, "arguments", item_type)?.to_owned()
            } else {
                json!({"input": required_string(item, "input", item_type)?}).to_string()
            };
            messages.push(json!({
                "role": "assistant",
                "content": Value::Null,
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": {"name": chat_name, "arguments": arguments}
                }]
            }));
        }
        "function_call_output" | "custom_tool_call_output" => {
            let call_id = required_string(item, "call_id", item_type)?;
            let output = item
                .get("output")
                .ok_or_else(|| format!("{item_type} 缺少 output"))?;
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": render_tool_output(output)
            }));
        }
        "local_shell_call" => {
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .or_else(|| item.get("id").and_then(Value::as_str))
                .ok_or_else(|| "local_shell_call 缺少 call_id".to_owned())?;
            let action = item
                .get("action")
                .ok_or_else(|| "local_shell_call 缺少 action".to_owned())?;
            messages.push(json!({
                "role": "assistant",
                "content": Value::Null,
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": {"name": "local_shell", "arguments": action.to_string()}
                }]
            }));
        }
        "reasoning" | "compaction" | "context_compaction" => {}
        unsupported => {
            return Err(format!(
                "当前 Chat Completions 网关不支持 Responses input 类型：{unsupported}"
            ));
        }
    }
    Ok(())
}

fn translate_message_content(content: &[Value]) -> Result<Value, String> {
    let mut translated = Vec::new();
    let mut all_text = true;
    for part in content {
        let part_type = part
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "消息内容缺少 type".to_owned())?;
        match part_type {
            "input_text" | "output_text" => translated.push(json!({
                "type": "text",
                "text": required_string(part, "text", part_type)?
            })),
            "input_image" => {
                all_text = false;
                translated.push(json!({
                    "type": "image_url",
                    "image_url": {
                        "url": required_string(part, "image_url", part_type)?,
                        "detail": part.get("detail").and_then(Value::as_str).unwrap_or("auto")
                    }
                }));
            }
            unsupported => {
                return Err(format!(
                    "当前 Chat Completions 网关不支持消息内容类型：{unsupported}"
                ));
            }
        }
    }
    if all_text {
        Ok(Value::String(
            translated
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    } else {
        Ok(Value::Array(translated))
    }
}

fn translate_tools(
    tools: Option<&Value>,
) -> Result<(Vec<Value>, HashMap<String, ToolKind>), String> {
    let Some(tools) = tools else {
        return Ok((Vec::new(), HashMap::new()));
    };
    let tools = tools
        .as_array()
        .ok_or_else(|| "Responses tools 必须是数组".to_owned())?;
    let mut translated = Vec::with_capacity(tools.len());
    let mut kinds = HashMap::with_capacity(tools.len());
    for tool in tools {
        let tool_type = tool
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "Responses tool 缺少 type".to_owned())?;
        match tool_type {
            "function" => push_chat_tool(
                &mut translated,
                &mut kinds,
                required_string(tool, "name", "tool")?.to_owned(),
                tool.get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                tool.get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"})),
                ToolKind::Function,
            )?,
            "custom" => push_chat_tool(
                &mut translated,
                &mut kinds,
                required_string(tool, "name", "tool")?.to_owned(),
                tool.get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                json!({
                    "type": "object",
                    "properties": {"input": {"type": "string"}},
                    "required": ["input"],
                    "additionalProperties": false
                }),
                ToolKind::Custom,
            )?,
            "namespace" => {
                let namespace = required_string(tool, "name", "namespace")?;
                let namespace_description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let namespace_tools = tool
                    .get("tools")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "Responses namespace 缺少 tools 数组".to_owned())?;
                for nested in namespace_tools {
                    let nested_type = nested
                        .get("type")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "Responses namespace tool 缺少 type".to_owned())?;
                    if nested_type != "function" {
                        return Err(format!(
                            "当前 Chat Completions 网关不支持 namespace 内的工具类型：{nested_type}"
                        ));
                    }
                    let name = required_string(nested, "name", "namespace tool")?;
                    let child_description = nested
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let description = match (namespace_description, child_description) {
                        ("", child) => child.to_owned(),
                        (parent, "") => parent.to_owned(),
                        (parent, child) => format!("{parent}\n\n{child}"),
                    };
                    push_chat_tool(
                        &mut translated,
                        &mut kinds,
                        flatten_tool_name(namespace, name),
                        description,
                        nested
                            .get("parameters")
                            .cloned()
                            .unwrap_or_else(|| json!({"type": "object"})),
                        ToolKind::NamespacedFunction {
                            namespace: namespace.to_owned(),
                            name: name.to_owned(),
                        },
                    )?;
                }
            }
            unsupported => {
                return Err(format!(
                    "当前 Chat Completions 网关不支持工具类型：{unsupported}"
                ));
            }
        }
    }
    Ok((translated, kinds))
}

fn flatten_tool_name(namespace: &str, name: &str) -> String {
    format!("{namespace}__{name}")
}

fn push_chat_tool(
    translated: &mut Vec<Value>,
    kinds: &mut HashMap<String, ToolKind>,
    chat_name: String,
    description: String,
    parameters: Value,
    kind: ToolKind,
) -> Result<(), String> {
    if kinds.insert(chat_name.clone(), kind).is_some() {
        return Err(format!("Responses 工具扁平化后名称冲突：{chat_name}"));
    }
    translated.push(json!({
        "type": "function",
        "function": {
            "name": chat_name,
            "description": description,
            "parameters": parameters
        }
    }));
    Ok(())
}

fn translate_text_format(format: &Value) -> Result<Value, String> {
    if format.get("type").and_then(Value::as_str) != Some("json_schema") {
        return Err("只支持 Responses json_schema 文本格式".to_owned());
    }
    Ok(json!({
        "type": "json_schema",
        "json_schema": {
            "name": format.get("name").and_then(Value::as_str).unwrap_or("simple_response"),
            "strict": format.get("strict").and_then(Value::as_bool).unwrap_or(false),
            "schema": format.get("schema").cloned().unwrap_or_else(|| json!({}))
        }
    }))
}

fn apply_reasoning(
    body: &mut Map<String, Value>,
    dialect: ChatDialect,
    effort: Option<&str>,
    enabled: bool,
) {
    match dialect {
        ChatDialect::Standard => {}
        ChatDialect::DeepSeek => {
            body.insert(
                "thinking".to_owned(),
                json!({"type": if enabled { "enabled" } else { "disabled" }}),
            );
            if enabled {
                body.insert(
                    "reasoning_effort".to_owned(),
                    Value::String(effort.unwrap_or("high").to_owned()),
                );
            }
        }
        ChatDialect::Qwen => {
            body.insert("enable_thinking".to_owned(), Value::Bool(enabled));
        }
    }
}

fn required_string<'a>(value: &'a Value, field: &str, owner: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{owner} 缺少 {field}"))
}

fn render_tool_output(output: &Value) -> String {
    match output {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

pub(crate) fn translated_response_body(
    upstream: Response,
    prepared: PreparedChatRequest,
) -> axum::body::Body {
    let source: UpstreamEvents = Box::pin(upstream.bytes_stream().eventsource());
    let translator = ChatStreamTranslator::new(prepared.tool_kinds, prepared.reasoning_enabled);
    let state = StreamState {
        source,
        translator,
        pending: VecDeque::from([Ok(sse(json!({
            "type": "response.created",
            "response": {"id": "simple_pending"}
        })))]),
        finished: false,
    };
    axum::body::Body::from_stream(futures_util::stream::unfold(
        state,
        |mut state| async move {
            loop {
                if let Some(item) = state.pending.pop_front() {
                    return Some((item, state));
                }
                if state.finished {
                    return None;
                }
                match state.source.next().await {
                    Some(Ok(event)) if event.data.trim() == "[DONE]" => {
                        state
                            .pending
                            .extend(state.translator.finish().into_iter().map(Ok));
                        state.finished = true;
                    }
                    Some(Ok(event)) => match serde_json::from_str::<Value>(&event.data) {
                        Ok(chunk) => match state.translator.push_chunk(&chunk) {
                            Ok(frames) => state.pending.extend(frames.into_iter().map(Ok)),
                            Err(message) => {
                                state.pending.push_back(Ok(failed_sse(&message)));
                                state.finished = true;
                            }
                        },
                        Err(_) => {
                            state.pending.push_back(Ok(failed_sse(
                                "上游 Chat Completions 返回了无效 JSON 流",
                            )));
                            state.finished = true;
                        }
                    },
                    Some(Err(error)) => {
                        let (code, message) =
                            crate::responses_gateway::upstream_stream_failure(&error);
                        state
                            .pending
                            .push_back(Ok(failed_sse_with_code(code, message)));
                        state.finished = true;
                    }
                    None => {
                        state
                            .pending
                            .extend(state.translator.finish().into_iter().map(Ok));
                        state.finished = true;
                    }
                }
            }
        },
    ))
}

struct StreamState {
    source: UpstreamEvents,
    translator: ChatStreamTranslator,
    pending: VecDeque<Result<Bytes, io::Error>>,
    finished: bool,
}

#[derive(Default)]
struct ToolAccumulator {
    call_id: String,
    name: String,
    arguments: String,
}

struct ChatStreamTranslator {
    response_id: String,
    message_id: String,
    reasoning_id: String,
    text: String,
    reasoning_text: String,
    tools: BTreeMap<usize, ToolAccumulator>,
    tool_kinds: HashMap<String, ToolKind>,
    usage: ChatUsage,
    reasoning_enabled: bool,
    reasoning_started: bool,
    message_started: bool,
    finished: bool,
}

#[derive(Default)]
struct ChatUsage {
    input_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
}

impl ChatStreamTranslator {
    fn new(tool_kinds: HashMap<String, ToolKind>, reasoning_enabled: bool) -> Self {
        Self {
            response_id: format!("resp_simple_{}", Uuid::new_v4().simple()),
            message_id: format!("msg_simple_{}", Uuid::new_v4().simple()),
            reasoning_id: format!("rs_simple_{}", Uuid::new_v4().simple()),
            text: String::new(),
            reasoning_text: String::new(),
            tools: BTreeMap::new(),
            tool_kinds,
            usage: ChatUsage::default(),
            reasoning_enabled,
            reasoning_started: false,
            message_started: false,
            finished: false,
        }
    }

    fn push_chunk(&mut self, chunk: &Value) -> Result<Vec<Bytes>, String> {
        if let Some(usage) = chunk.get("usage") {
            self.usage.input_tokens = usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(self.usage.input_tokens);
            self.usage.output_tokens = usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(self.usage.output_tokens);
            self.usage.reasoning_tokens = usage
                .get("completion_tokens_details")
                .and_then(|details| details.get("reasoning_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(self.usage.reasoning_tokens);
        }
        let Some(delta) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
        else {
            return Ok(Vec::new());
        };
        let mut frames = Vec::new();
        if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
            if !self.reasoning_enabled && !reasoning.is_empty() {
                return Err("已关闭推理，但上游模型返回了推理内容".to_owned());
            }
            if self.message_started && !reasoning.is_empty() {
                return Err("上游模型在回答正文开始后又返回了推理内容".to_owned());
            }
            if !reasoning.is_empty() {
                if !self.reasoning_started {
                    frames.push(sse(json!({
                        "type": "response.output_item.added",
                        "output_index": 0,
                        "item": {
                            "id": self.reasoning_id,
                            "type": "reasoning",
                            "summary": [],
                            "content": [],
                            "encrypted_content": null
                        }
                    })));
                    self.reasoning_started = true;
                }
                self.reasoning_text.push_str(reasoning);
                frames.push(sse(json!({
                    "type": "response.reasoning_text.delta",
                    "content_index": 0,
                    "delta": reasoning
                })));
            }
        }
        if let Some(content) = delta.get("content").and_then(Value::as_str)
            && !content.is_empty()
        {
            if let Some(done) = self.finish_reasoning() {
                frames.push(done);
            }
            if !self.message_started {
                frames.push(sse(json!({
                    "type": "response.output_item.added",
                    "output_index": usize::from(self.reasoning_started),
                    "item": {
                        "id": self.message_id,
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": ""}]
                    }
                })));
                self.message_started = true;
            }
            self.text.push_str(content);
            frames.push(sse(json!({
                "type": "response.output_text.delta",
                "item_id": self.message_id,
                "output_index": usize::from(self.reasoning_started),
                "content_index": 0,
                "delta": content
            })));
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let accumulator = self.tools.entry(index).or_default();
                if let Some(call_id) = call.get("id").and_then(Value::as_str) {
                    accumulator.call_id.push_str(call_id);
                }
                if let Some(name) = call
                    .get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
                {
                    accumulator.name.push_str(name);
                }
                if let Some(arguments) = call
                    .get("function")
                    .and_then(|function| function.get("arguments"))
                    .and_then(Value::as_str)
                {
                    accumulator.arguments.push_str(arguments);
                }
            }
        }
        Ok(frames)
    }

    fn finish(&mut self) -> Vec<Bytes> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        let mut frames = Vec::new();
        if let Some(done) = self.finish_reasoning() {
            frames.push(done);
        }
        if !self.text.is_empty() {
            frames.push(sse(json!({
                "type": "response.output_item.done",
                "output_index": usize::from(self.reasoning_started),
                "item": {
                    "id": self.message_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": self.text}]
                }
            })));
        }
        for (index, tool) in &self.tools {
            let call_id = if tool.call_id.is_empty() {
                format!("call_simple_{}", Uuid::new_v4().simple())
            } else {
                tool.call_id.clone()
            };
            let item_id = format!("fc_simple_{}", Uuid::new_v4().simple());
            let item = match self.tool_kinds.get(&tool.name) {
                Some(ToolKind::Custom) => json!({
                    "id": item_id,
                    "type": "custom_tool_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": tool.name,
                    "input": custom_tool_input(&tool.arguments)
                }),
                Some(ToolKind::NamespacedFunction { namespace, name }) => json!({
                    "id": item_id,
                    "type": "function_call",
                    "status": "completed",
                    "call_id": call_id,
                    "namespace": namespace,
                    "name": name,
                    "arguments": tool.arguments
                }),
                _ => json!({
                    "id": item_id,
                    "type": "function_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": tool.name,
                    "arguments": tool.arguments
                }),
            };
            frames.push(sse(json!({
                "type": "response.output_item.done",
                "output_index": index
                    + usize::from(self.reasoning_started)
                    + usize::from(!self.text.is_empty()),
                "item": item
            })));
        }
        let total_tokens = self
            .usage
            .input_tokens
            .saturating_add(self.usage.output_tokens);
        frames.push(sse(json!({
            "type": "response.completed",
            "response": {
                "id": self.response_id,
                "usage": {
                    "input_tokens": self.usage.input_tokens,
                    "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0},
                    "output_tokens": self.usage.output_tokens,
                    "output_tokens_details": {"reasoning_tokens": self.usage.reasoning_tokens},
                    "total_tokens": total_tokens
                }
            }
        })));
        frames
    }

    fn finish_reasoning(&mut self) -> Option<Bytes> {
        if !self.reasoning_started || self.reasoning_text.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.reasoning_text);
        Some(sse(json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "id": self.reasoning_id,
                "type": "reasoning",
                "summary": [],
                "content": [{"type": "reasoning_text", "text": text}],
                "encrypted_content": null
            }
        })))
    }
}

fn custom_tool_input(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|value| {
            value
                .get("input")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| arguments.to_owned())
}

fn sse(value: Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}

fn failed_sse(message: &str) -> Bytes {
    failed_sse_with_code("simple_gateway_translation_failed", message)
}

fn failed_sse_with_code(code: &str, message: &str) -> Bytes {
    sse(json!({
        "type": "response.failed",
        "response": {
            "error": {
                "type": "simple_gateway_error",
                "code": code,
                "message": message
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codex_request() -> Map<String, Value> {
        serde_json::from_value::<Map<String, Value>>(json!({
            "model": "simple-alias",
            "instructions": "You are a coding agent.",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "read it"}]},
                {"type": "function_call", "call_id": "old_call", "name": "read_file", "arguments": "{\"path\":\"a\"}"},
                {"type": "function_call_output", "call_id": "old_call", "output": "contents"}
            ],
            "tools": [
                {"type": "function", "name": "read_file", "description": "read", "parameters": {"type": "object"}},
                {"type": "custom", "name": "apply_patch", "description": "patch"}
            ],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "reasoning": {"effort": "high"},
            "stream": true
        }))
        .expect("object")
    }

    #[test]
    fn translates_responses_request_to_deepseek_chat_completions() {
        let prepared =
            prepare_chat_request(&codex_request(), "deepseek-chat", ChatDialect::DeepSeek)
                .expect("translation");
        assert_eq!(prepared.body["model"], "deepseek-chat");
        assert_eq!(prepared.body["messages"][0]["role"], "system");
        assert_eq!(
            prepared.body["messages"][2]["tool_calls"][0]["id"],
            "old_call"
        );
        assert_eq!(prepared.body["messages"][3]["tool_call_id"], "old_call");
        assert_eq!(prepared.body["tools"][1]["function"]["name"], "apply_patch");
        assert_eq!(prepared.body["thinking"]["type"], "enabled");
        assert_eq!(prepared.body["reasoning_effort"], "high");
    }

    #[test]
    fn flattens_namespace_tools_for_chat_and_restores_namespace_on_output() {
        let mut request = codex_request();
        request.insert(
            "tools".to_owned(),
            json!([
                {
                    "type": "namespace",
                    "name": "clock",
                    "description": "Time tools",
                    "tools": [{
                        "type": "function",
                        "name": "sleep",
                        "description": "Wait",
                        "parameters": {
                            "type": "object",
                            "properties": {"duration_ms": {"type": "number"}},
                            "required": ["duration_ms"]
                        }
                    }]
                }
            ]),
        );
        request.insert(
            "input".to_owned(),
            json!([
                {
                    "type": "function_call",
                    "call_id": "old_sleep",
                    "namespace": "clock",
                    "name": "sleep",
                    "arguments": "{\"duration_ms\":1}"
                },
                {"type": "function_call_output", "call_id": "old_sleep", "output": "done"}
            ]),
        );

        let prepared = prepare_chat_request(&request, "model", ChatDialect::Standard)
            .expect("namespace translation");
        assert_eq!(
            prepared.body["tools"][0]["function"]["name"],
            "clock__sleep"
        );
        assert_eq!(
            prepared.body["messages"][1]["tool_calls"][0]["function"]["name"],
            "clock__sleep"
        );
        let mut translator =
            ChatStreamTranslator::new(prepared.tool_kinds, prepared.reasoning_enabled);
        translator
            .push_chunk(&json!({
                "choices": [{"delta": {"tool_calls": [{
                    "index": 0,
                    "id": "new_sleep",
                    "function": {
                        "name": "clock__sleep",
                        "arguments": "{\"duration_ms\":2}"
                    }
                }]}}]
            }))
            .expect("namespace tool chunk");
        let rendered = translator
            .finish()
            .into_iter()
            .map(|frame| String::from_utf8_lossy(&frame).into_owned())
            .collect::<String>();
        assert!(rendered.contains("\"namespace\":\"clock\""));
        assert!(rendered.contains("\"name\":\"sleep\""));
        assert!(!rendered.contains("\"name\":\"clock__sleep\""));
    }

    #[test]
    fn frames_deepseek_reasoning_before_the_assistant_message() {
        let prepared = prepare_chat_request(&codex_request(), "model", ChatDialect::DeepSeek)
            .expect("translation");
        let mut translator =
            ChatStreamTranslator::new(prepared.tool_kinds, prepared.reasoning_enabled);
        let reasoning_frames = translator
            .push_chunk(&json!({
                "choices": [{"delta": {"reasoning_content": "inspect"}}]
            }))
            .expect("reasoning chunk");
        assert_eq!(reasoning_frames.len(), 2);
        assert!(
            String::from_utf8_lossy(&reasoning_frames[0]).contains("response.output_item.added")
        );
        assert!(
            String::from_utf8_lossy(&reasoning_frames[1]).contains("response.reasoning_text.delta")
        );

        let text_frames = translator
            .push_chunk(&json!({
                "choices": [{"delta": {"content": "answer"}}]
            }))
            .expect("answer chunk");
        assert_eq!(text_frames.len(), 3);
        assert!(String::from_utf8_lossy(&text_frames[0]).contains("response.output_item.done"));
        assert!(String::from_utf8_lossy(&text_frames[0]).contains("reasoning_text"));
        assert!(String::from_utf8_lossy(&text_frames[1]).contains("response.output_item.added"));
        assert!(String::from_utf8_lossy(&text_frames[2]).contains("response.output_text.delta"));
    }

    #[test]
    fn translates_text_and_function_or_custom_tool_streams() {
        let prepared = prepare_chat_request(&codex_request(), "model", ChatDialect::Standard)
            .expect("translation");
        let mut translator =
            ChatStreamTranslator::new(prepared.tool_kinds, prepared.reasoning_enabled);
        let frames = translator
            .push_chunk(&json!({
                "choices": [{"delta": {
                    "content": "done",
                    "tool_calls": [
                        {"index": 0, "id": "call_1", "function": {"name": "read_file", "arguments": "{\"path\":"}},
                        {"index": 1, "id": "call_2", "function": {"name": "apply_patch", "arguments": "{\"input\":\"***"}}
                    ]
                }}]
            }))
            .expect("first chunk");
        assert!(String::from_utf8_lossy(&frames[0]).contains("response.output_item.added"));
        assert!(String::from_utf8_lossy(&frames[1]).contains("response.output_text.delta"));
        translator
            .push_chunk(&json!({
                "choices": [{"delta": {"tool_calls": [
                    {"index": 0, "function": {"arguments": "\"a\"}"}},
                    {"index": 1, "function": {"arguments": " PATCH\"}"}}
                ]}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 4, "completion_tokens_details": {"reasoning_tokens": 1}}
            }))
            .expect("second chunk");
        let rendered = translator
            .finish()
            .into_iter()
            .map(|frame| String::from_utf8_lossy(&frame).into_owned())
            .collect::<String>();
        assert!(rendered.contains("\"type\":\"function_call\""));
        assert!(rendered.contains("\"arguments\":\"{\\\"path\\\":\\\"a\\\"}\""));
        assert!(rendered.contains("\"type\":\"custom_tool_call\""));
        assert!(rendered.contains("\"input\":\"*** PATCH\""));
        assert!(rendered.contains("\"input_tokens\":10"));
        assert!(rendered.contains("\"reasoning_tokens\":1"));
        assert!(rendered.contains("response.completed"));
    }

    #[test]
    fn rejects_unsupported_or_non_streaming_requests() {
        let mut request = codex_request();
        request.insert("stream".to_owned(), Value::Bool(false));
        assert!(prepare_chat_request(&request, "model", ChatDialect::Standard).is_err());

        let mut request = codex_request();
        request.insert(
            "input".to_owned(),
            json!([{"type": "web_search_call", "status": "completed"}]),
        );
        assert!(prepare_chat_request(&request, "model", ChatDialect::Standard).is_err());
    }
}
