use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{Capability, CapabilitySet};

const MAX_TOOL_NAME_BYTES: usize = 128;

/// How the kernel may schedule a tool handler.
///
/// `Action` handlers prepare an [`ActionIntent`]; they never execute the
/// described side effect themselves. Actions are serialized so that their
/// approval, persistence, and execution order remains unambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolExecution {
    ReadOnly { parallel_safe: bool },
    Action { serial: bool },
}

impl ToolExecution {
    #[must_use]
    pub const fn read_only(parallel_safe: bool) -> Self {
        Self::ReadOnly { parallel_safe }
    }

    #[must_use]
    pub const fn action() -> Self {
        Self::Action { serial: true }
    }

    #[must_use]
    pub const fn is_read_only(self) -> bool {
        matches!(self, Self::ReadOnly { .. })
    }

    #[must_use]
    pub const fn can_run_in_parallel(self) -> bool {
        matches!(
            self,
            Self::ReadOnly {
                parallel_safe: true
            }
        )
    }
}

/// Model-visible metadata and kernel scheduling policy for one tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    name: String,
    description: String,
    input_schema: Value,
    required_capabilities: CapabilitySet,
    execution: ToolExecution,
}

impl ToolSpec {
    #[must_use]
    pub fn read_only(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        required_capabilities: CapabilitySet,
        parallel_safe: bool,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            required_capabilities,
            execution: ToolExecution::read_only(parallel_safe),
        }
    }

    /// Creates an action tool. Action handlers are always serialized.
    #[must_use]
    pub fn action(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        required_capabilities: CapabilitySet,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            required_capabilities,
            execution: ToolExecution::action(),
        }
    }

    pub fn validate(&self) -> Result<(), ToolSpecError> {
        if self.name.is_empty()
            || self.name.len() > MAX_TOOL_NAME_BYTES
            || !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(ToolSpecError::InvalidName);
        }
        if self.description.trim().is_empty() {
            return Err(ToolSpecError::EmptyDescription);
        }
        let Some(schema) = self.input_schema.as_object() else {
            return Err(ToolSpecError::InvalidInputSchema);
        };
        if schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err(ToolSpecError::InputSchemaMustDescribeObject);
        }
        if schema
            .get("properties")
            .is_some_and(|value| !value.is_object())
        {
            return Err(ToolSpecError::InvalidInputSchema);
        }
        if self.execution.is_read_only()
            && self
                .required_capabilities
                .iter()
                .any(Capability::is_effectful)
        {
            return Err(ToolSpecError::EffectfulCapabilityOnReadOnlyTool);
        }
        if !self.execution.is_read_only() && self.required_capabilities.is_empty() {
            return Err(ToolSpecError::ActionMissingCapability);
        }
        Ok(())
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[must_use]
    pub fn input_schema(&self) -> &Value {
        &self.input_schema
    }

    #[must_use]
    pub fn required_capabilities(&self) -> &CapabilitySet {
        &self.required_capabilities
    }

    #[must_use]
    pub const fn execution(&self) -> ToolExecution {
        self.execution
    }

    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.execution.is_read_only()
    }

    #[must_use]
    pub const fn can_run_in_parallel(&self) -> bool {
        self.execution.can_run_in_parallel()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolSpecError {
    #[error("工具名称无效")]
    InvalidName,
    #[error("工具说明不能为空")]
    EmptyDescription,
    #[error("工具输入结构必须是 JSON 对象")]
    InvalidInputSchema,
    #[error("工具输入结构必须声明 type=object")]
    InputSchemaMustDescribeObject,
    #[error("只读工具不能声明可能产生副作用的能力")]
    EffectfulCapabilityOnReadOnlyTool,
    #[error("副作用工具必须声明至少一项能力")]
    ActionMissingCapability,
}

/// Untrusted arguments proposed by a model for a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInput {
    pub arguments: Value,
}

impl ToolInput {
    #[must_use]
    pub const fn new(arguments: Value) -> Self {
        Self { arguments }
    }
}

/// A model-proposed invocation. The call ID is retained for durable correlation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: ToolInput,
}

impl ToolCall {
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input: ToolInput::new(arguments),
        }
    }
}

/// Handler input supplied by the router after name and shape validation.
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    pub call_id: String,
    pub input: ToolInput,
    pub idempotency_key: String,
    pub cancellation: CancellationToken,
}

/// Per-dispatch controls owned by the turn engine.
#[derive(Debug, Clone)]
pub struct ToolDispatchContext {
    pub idempotency_key: String,
    pub cancellation: CancellationToken,
}

/// One call and the durable controls assigned to it by the turn engine.
///
/// Batch dispatch accepts this type so callers can scope every idempotency key
/// by workspace, thread, turn, and step instead of falling back to a provider
/// call ID.
#[derive(Debug, Clone)]
pub struct ToolDispatchRequest {
    pub call: ToolCall,
    pub context: ToolDispatchContext,
}

impl ToolDispatchRequest {
    #[must_use]
    pub const fn new(call: ToolCall, context: ToolDispatchContext) -> Self {
        Self { call, context }
    }
}

impl ToolDispatchContext {
    #[must_use]
    pub fn new(idempotency_key: impl Into<String>, cancellation: CancellationToken) -> Self {
        Self {
            idempotency_key: idempotency_key.into(),
            cancellation,
        }
    }

    #[must_use]
    pub fn for_call(call: &ToolCall, cancellation: CancellationToken) -> Self {
        Self::new(call.id.clone(), cancellation)
    }
}

/// Read-only information that can be appended to the next model context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub value: Value,
}

impl Evidence {
    #[must_use]
    pub const fn new(value: Value) -> Self {
        Self { value }
    }
}

/// A proposed side effect. It is data for the action gate, not authorization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionIntent {
    pub tool_call_id: String,
    pub action: String,
    pub input: Value,
    pub required_capabilities: CapabilitySet,
    pub idempotency_key: String,
}

impl ActionIntent {
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<String>,
        action: impl Into<String>,
        input: Value,
        required_capabilities: CapabilitySet,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            action: action.into(),
            input,
            required_capabilities,
            idempotency_key: idempotency_key.into(),
        }
    }
}

/// A tool either reports evidence or proposes an action for the policy layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum ToolOutcome {
    Evidence(Evidence),
    ActionIntent(ActionIntent),
}

/// A handler-owned failure without routing details or secret-bearing inputs.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ToolFailure {
    message: String,
}

impl ToolFailure {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ToolExecution, ToolSpec, ToolSpecError};
    use crate::{Capability, CapabilitySet};

    #[test]
    fn read_only_specs_expose_parallel_and_capability_metadata() {
        let spec = ToolSpec::read_only(
            "read_file",
            "读取项目文件",
            json!({"type": "object"}),
            CapabilitySet::from([Capability::ReadWorkspace]),
            true,
        );

        assert_eq!(spec.validate(), Ok(()));
        assert!(spec.is_read_only());
        assert!(spec.can_run_in_parallel());
        assert_eq!(
            spec.execution(),
            ToolExecution::ReadOnly {
                parallel_safe: true
            }
        );
    }

    #[test]
    fn malformed_tool_metadata_is_rejected() {
        let invalid_name = ToolSpec::action(
            "write file",
            "写文件",
            json!({"type": "object"}),
            CapabilitySet::from([Capability::WriteWorkspace]),
        );
        assert_eq!(invalid_name.validate(), Err(ToolSpecError::InvalidName));

        let invalid_schema = ToolSpec::action(
            "write_file",
            "写文件",
            json!("not-an-object"),
            CapabilitySet::from([Capability::WriteWorkspace]),
        );
        assert_eq!(
            invalid_schema.validate(),
            Err(ToolSpecError::InvalidInputSchema)
        );
    }

    #[test]
    fn read_only_tools_cannot_hide_effectful_capabilities() {
        let spec = ToolSpec::read_only(
            "mislabelled",
            "错误标记",
            json!({"type": "object"}),
            CapabilitySet::from([Capability::WriteWorkspace]),
            false,
        );
        assert_eq!(
            spec.validate(),
            Err(ToolSpecError::EffectfulCapabilityOnReadOnlyTool)
        );
    }

    #[test]
    fn actions_are_serial_and_require_a_capability() {
        let spec = ToolSpec::action(
            "write_file",
            "写文件",
            json!({"type": "object"}),
            CapabilitySet::from([Capability::WriteWorkspace]),
        );
        assert_eq!(spec.validate(), Ok(()));
        assert_eq!(spec.execution(), ToolExecution::Action { serial: true });
        assert!(!spec.can_run_in_parallel());

        let missing = ToolSpec::action(
            "unsafe",
            "缺少能力",
            json!({"type": "object"}),
            CapabilitySet::new(),
        );
        assert_eq!(
            missing.validate(),
            Err(ToolSpecError::ActionMissingCapability)
        );
    }
}
