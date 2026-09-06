mod capability;
mod coding;
mod command;
mod command_runtime;
mod protocol;
mod registry;
mod router;
mod workspace;

pub use capability::{Capability, CapabilitySet};
pub use coding::{CodingToolGuidance, coding_tool_handlers, coding_tool_registry};
pub use command::{
    CommandError, CommandErrorKind, CommandExecutionBoundary, CommandExecutionPolicy,
    CommandOutput, CommandRequest, ProcessBoundary, run_command, run_command_with_policy,
    validate_command_request, workspace_sandbox_available, workspace_sandbox_health,
};
pub use command_runtime::{CommandExecutionMetadata, command_execution_metadata};
pub use local_agent_sandbox::{PermissionProfile, SandboxBackend, SandboxHealth};
pub use protocol::{
    ActionIntent, Evidence, ToolCall, ToolDispatchContext, ToolDispatchRequest, ToolExecution,
    ToolFailure, ToolInput, ToolInvocation, ToolOutcome, ToolSpec, ToolSpecError,
};
pub use registry::{ToolHandler, ToolRegistry, ToolRegistryError};
pub use router::{ToolRouteError, ToolRouter};
pub use workspace::{
    EditMatch, FileContent, SearchMatch, TextEdit, Workspace, WorkspaceError, WritePreview,
    hash_bytes,
};
