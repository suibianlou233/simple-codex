//! Wire-only compatibility: keep native Responses history and Codex tool routing.
//! DeepSeek supports custom apply_patch, but not arbitrary custom tools like exec.
use std::collections::HashSet;
use std::io;

use axum::body::Body;
use axum::body::Bytes;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

#[derive(Default)]
pub(crate) struct ResponsesToolTranslation {
    custom_names: HashSet<String>,
    custom_item_ids: HashSet<String>,
}

impl ResponsesToolTranslation {
    pub(crate) fn prepare(payload: &mut Map<String, Value>) -> Result<Self, String> {
        let mut translation = Self::default();
        if let Some(tools) = payload.get_mut("tools").and_then(Value::as_array_mut) {
            for tool in tools {
                if tool["type"] == "custom" && tool["name"] != "apply_patch" {
                    let name = required(tool, "name")?.to_owned();
                    translation.custom_names.insert(name.clone());
                    *tool = json!({
                        "type": "function", "name": name,
                        "description": format!("{}\nPass the complete raw tool input as the input string; do not execute or rewrite it yourself.", tool["description"].as_str().unwrap_or("")),
                        "parameters": {"type": "object", "properties": {
                            "input": {"type": "string", "description": "Complete raw custom tool input (including code and any pragma)."}
                        }, "required": ["input"], "additionalProperties": false}
                    });
                }
            }
        }
        // Identify outputs from the complete local history, not only current tools:
        // a tool can disappear from the active tool list between turns.
        let mut call_ids = HashSet::new();
        if let Some(input) = payload.get_mut("input").and_then(Value::as_array_mut) {
            for item in input.iter_mut() {
                if item["type"] == "custom_tool_call" && item["name"] != "apply_patch" {
                    let name = required(item, "name")?.to_owned();
                    call_ids.insert(required(item, "call_id")?.to_owned());
                    translation.custom_names.insert(name);
                    let raw = required(item, "input")?.to_owned();
                    item["type"] = json!("function_call");
                    item["arguments"] = json!(json!({"input": raw}).to_string());
                    if let Some(object) = item.as_object_mut() {
                        object.remove("input");
                    }
                }
            }
            for item in input {
                if item["type"] == "custom_tool_call_output"
                    && item["call_id"]
                        .as_str()
                        .is_some_and(|id| call_ids.contains(id))
                {
                    item["type"] = json!("function_call_output");
                }
            }
        }
        if let Some(choice) = payload.get_mut("tool_choice")
            && choice["type"] == "custom"
            && choice["name"]
                .as_str()
                .is_some_and(|name| translation.custom_names.contains(name))
        {
            choice["type"] = json!("function");
        }
        Ok(translation)
    }

    fn restore_item(&mut self, item: &mut Value, partial: bool) -> Result<(), String> {
        if item["type"] != "function_call"
            || !item["name"]
                .as_str()
                .is_some_and(|name| self.custom_names.contains(name))
        {
            return Ok(());
        }
        if let Some(id) = item["id"].as_str() {
            self.custom_item_ids.insert(id.to_owned());
        }
        let arguments = item["arguments"].as_str().unwrap_or("");
        let input = if partial && arguments.is_empty() {
            String::new()
        } else {
            let wrapper: Value = serde_json::from_str(arguments)
                .map_err(|_| "模型返回的自定义工具参数不是有效 JSON".to_owned())?;
            required(&wrapper, "input")?.to_owned()
        };
        item["type"] = json!("custom_tool_call");
        item["input"] = json!(input);
        if let Some(object) = item.as_object_mut() {
            object.remove("arguments");
        }
        Ok(())
    }

    fn event(&mut self, mut event: Value) -> Result<Option<Value>, String> {
        let kind = event["type"].as_str().unwrap_or("").to_owned();
        match kind.as_str() {
            "response.output_item.added" | "response.output_item.done" => {
                self.restore_item(&mut event["item"], kind.ends_with("added"))?;
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                if event["item_id"]
                    .as_str()
                    .is_some_and(|id| self.custom_item_ids.contains(id))
                {
                    // JSON-escaped chunks are not raw custom input. Codex dispatches
                    // the validated complete input from output_item.done instead.
                    return Ok(None);
                }
            }
            "response.completed" | "response.incomplete" => {
                if let Some(items) = event["response"]["output"].as_array_mut() {
                    for item in items {
                        self.restore_item(item, false)?;
                    }
                }
            }
            _ => {}
        }
        Ok(Some(event))
    }

    pub(crate) fn response_body(self, upstream: reqwest::Response) -> Body {
        let source = Box::pin(upstream.bytes_stream().eventsource());
        Body::from_stream(futures_util::stream::unfold(
            (source, self, false),
            |(mut source, mut translation, finished)| async move {
                if finished {
                    return None;
                }
                loop {
                    let next = source.next().await;
                    let result = match next {
                        Some(Ok(event)) => serde_json::from_str::<Value>(&event.data)
                            .map_err(|_| "模型 Responses 流包含无效 JSON".to_owned())
                            .and_then(|value| translation.event(value)),
                        Some(Err(error)) => {
                            let (code, message) =
                                crate::responses_gateway::upstream_stream_failure(&error);
                            let event = json!({"type":"response.failed", "response":{"status":"failed", "error":{"type":"simple_gateway_error", "code":code, "message":message}}});
                            return Some((Ok(frame(event)), (source, translation, true)));
                        }
                        None => {
                            return Some((
                                Err(io::Error::other("模型 Responses 流缺少终态")),
                                (source, translation, true),
                            ));
                        }
                    };
                    match result {
                        Ok(None) => continue,
                        Ok(Some(event)) => {
                            let terminal = matches!(
                                event["type"].as_str(),
                                Some(
                                    "response.completed"
                                        | "response.incomplete"
                                        | "response.failed"
                                        | "error"
                                )
                            );
                            return Some((Ok(frame(event)), (source, translation, terminal)));
                        }
                        Err(message) => {
                            let event = json!({"type":"response.failed", "response":{"status":"failed", "error":{"code":"simple_tool_translation_failed", "message":message}}});
                            return Some((Ok(frame(event)), (source, translation, true)));
                        }
                    }
                }
            },
        ))
    }
}

fn required<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("自定义工具缺少字符串字段 {field}"))
}

fn frame(value: Value) -> Bytes {
    Bytes::from(format!(
        "event: {}\ndata: {value}\n\n",
        value["type"].as_str().unwrap_or("message")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_exec_and_history_but_preserves_patch_and_native_functions() {
        let mut request = json!({"tools":[{"type":"custom","name":"exec","description":"JS"},{"type":"custom","name":"apply_patch"},{"type":"function","name":"wait"}],
            "input":[{"type":"custom_tool_call","name":"exec","call_id":"a","input":"text('中文\\n');"},{"type":"custom_tool_call_output","call_id":"a","output":"ok"},{"type":"custom_tool_call","name":"apply_patch","call_id":"b","input":"patch"},{"type":"custom_tool_call_output","call_id":"b","output":"ok"}],
            "tool_choice":{"type":"custom","name":"exec"}});
        ResponsesToolTranslation::prepare(request.as_object_mut().expect("valid test fixture"))
            .expect("valid test fixture");
        assert_eq!(request["tools"][0]["type"], "function");
        assert_eq!(request["tools"][1]["type"], "custom");
        assert_eq!(request["tools"][2]["name"], "wait");
        assert_eq!(
            request["input"][0]["arguments"],
            json!({"input":"text('中文\\n');"}).to_string()
        );
        assert!(request["input"][0].get("input").is_none());
        assert_eq!(request["input"][1]["type"], "function_call_output");
        assert_eq!(request["input"][3]["type"], "custom_tool_call_output");
        assert_eq!(request["tool_choice"]["type"], "function");
    }

    #[test]
    fn restores_completed_input_and_suppresses_json_deltas() {
        let mut request = json!({"tools":[{"type":"custom","name":"exec"}]});
        let mut adapter =
            ResponsesToolTranslation::prepare(request.as_object_mut().expect("valid test fixture"))
                .expect("valid test fixture");
        let added = adapter.event(json!({"type":"response.output_item.added","item":{"id":"i","type":"function_call","call_id":"c","name":"exec","arguments":""}})).expect("valid test fixture").expect("valid test fixture");
        assert_eq!(added["item"]["input"], "");
        assert!(adapter.event(json!({"type":"response.function_call_arguments.delta","item_id":"i","delta":"{\"in"})).expect("valid test fixture").is_none());
        let raw = "// @exec: {}\ntext('中文');";
        let item = json!({"id":"i","type":"function_call","call_id":"c","name":"exec","arguments":json!({"input":raw}).to_string()});
        let done = adapter
            .event(json!({"type":"response.output_item.done","item":item}))
            .expect("valid test fixture")
            .expect("valid test fixture");
        assert_eq!(done["item"]["type"], "custom_tool_call");
        assert_eq!(done["item"]["input"], raw);
        assert_eq!(done["item"]["call_id"], "c");
        let complete = adapter.event(json!({"type":"response.completed","response":{"output":[item],"usage":{"input_tokens":42}}})).expect("valid test fixture").expect("valid test fixture");
        assert_eq!(complete["response"]["output"][0]["input"], raw);
        assert_eq!(complete["response"]["usage"]["input_tokens"], 42);
    }

    #[test]
    fn invalid_wrapped_input_fails_closed() {
        let mut adapter = ResponsesToolTranslation {
            custom_names: HashSet::from(["exec".to_owned()]),
            ..Default::default()
        };
        for arguments in ["not JSON", "{}", "{\"input\":42}"] {
            assert!(adapter.event(json!({"type":"response.output_item.done","item":{"type":"function_call","name":"exec","arguments":arguments}})).is_err());
        }
    }
}
