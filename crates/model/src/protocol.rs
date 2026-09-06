use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningMode {
    #[default]
    Off,
    Low,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRequest {
    pub messages: Vec<Message>,
    pub reasoning: ReasoningMode,
    pub max_output_tokens: Option<u32>,
    pub tools: Vec<ToolDefinition>,
    pub tool_history: Vec<ToolExchange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExchange {
    pub calls: Vec<ToolCall>,
    pub results: Vec<ToolResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub streaming: bool,
    pub reasoning_control: bool,
    pub tool_calls: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelEvent {
    TextDelta(String),
    ToolCallDelta(Vec<ToolCallDelta>),
    Usage(Usage),
    Completed(FinishReason),
}

pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send + 'static>>;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("缺少模型凭据")]
    MissingCredential,
    #[error("模型接口地址无效：{0}")]
    InvalidEndpoint(String),
    #[error("模型请求失败：{0}")]
    Request(#[from] reqwest::Error),
    #[error("模型服务返回 HTTP {status}：{message}")]
    Http { status: u16, message: String },
    #[error("模型流式响应无效：{0}")]
    InvalidStream(String),
    #[error("已关闭推理，但模型仍返回了推理内容")]
    UnexpectedReasoning,
    #[error("模型请求已取消")]
    Cancelled,
}

#[async_trait]
pub trait ModelAdapter: Send + Sync {
    fn capabilities(&self) -> ModelCapabilities;

    async fn stream(
        &self,
        request: ModelRequest,
        cancellation: CancellationToken,
    ) -> Result<ModelEventStream, ModelError>;
}
