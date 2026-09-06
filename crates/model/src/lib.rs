//! Provider-neutral model protocol and independently implemented adapters.

mod chat_completions;
mod gateway;
mod kernel_compat;
mod protocol;
mod proxy_policy;
mod responses_chat_translation;
mod responses_gateway;
mod responses_tool_translation;
mod secret;

pub use chat_completions::ChatCompletionsAdapter;
pub use chat_completions::ChatCompletionsConfig;
pub use chat_completions::ChatDialect;
pub use gateway::ModelGateway;
pub use kernel_compat::*;
pub use protocol::FinishReason;
pub use protocol::Message;
pub use protocol::ModelAdapter;
pub use protocol::ModelCapabilities;
pub use protocol::ModelError;
pub use protocol::ModelEvent;
pub use protocol::ModelEventStream;
pub use protocol::ModelRequest;
pub use protocol::ReasoningMode;
pub use protocol::Role;
pub use protocol::ToolCall;
pub use protocol::ToolCallDelta;
pub use protocol::ToolDefinition;
pub use protocol::ToolExchange;
pub use protocol::ToolResult;
pub use protocol::Usage;
pub use responses_gateway::GatewayClientToken;
pub use responses_gateway::ResponsesGatewayConfig;
pub use responses_gateway::ResponsesGatewayError;
pub use responses_gateway::ResponsesGatewayHandle;
pub use responses_gateway::ResponsesGatewayUpstream;
pub use secret::ApiKey;
