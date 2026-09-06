use local_agent_sandbox::{
    ExecutionTarget, PermissionProfile, SandboxCommandRequest, SandboxCommandResponse,
    SandboxHealth, SandboxRuntime, SandboxRuntimeError, run_workspace_command,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::Workspace;
use crate::command::{
    CommandError, CommandExecutionBoundary, CommandExecutionPolicy, CommandOutput, CommandRequest,
    HostCwdScope, ProcessBoundary, execute_host_command, validate_command_request,
};

/// Secret-free facts needed to explain and reproduce an execution decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandExecutionMetadata {
    pub policy: CommandExecutionPolicy,
    pub permission_profile: PermissionProfile,
    pub permission_profile_hash: String,
    pub sandbox_health: Option<SandboxHealth>,
}

pub fn command_execution_metadata(
    policy: CommandExecutionPolicy,
) -> Result<CommandExecutionMetadata, CommandError> {
    let permission_profile = permission_profile(policy);
    let permission_profile_hash = permission_profile.stable_hash()?;
    let sandbox_health = matches!(permission_profile, PermissionProfile::Managed(_))
        .then(|| SandboxRuntime::current().health().clone());
    Ok(CommandExecutionMetadata {
        policy,
        permission_profile,
        permission_profile_hash,
        sandbox_health,
    })
}

pub(crate) async fn execute_command(
    workspace: &Workspace,
    request: &CommandRequest,
    cancellation: CancellationToken,
    policy: CommandExecutionPolicy,
) -> Result<CommandOutput, CommandError> {
    validate_command_request(request)?;
    let metadata = command_execution_metadata(policy)?;
    let runtime = SandboxRuntime::current();
    match runtime.plan(metadata.permission_profile) {
        Ok(ExecutionTarget::Host) => {
            let (cwd_scope, execution_boundary) = match policy {
                CommandExecutionPolicy::ApprovedHost => (
                    HostCwdScope::Workspace,
                    CommandExecutionBoundary::ApprovedHost,
                ),
                CommandExecutionPolicy::SystemFullAccess => (
                    HostCwdScope::System,
                    CommandExecutionBoundary::SystemFullAccess,
                ),
                CommandExecutionPolicy::WorkspaceSandbox => {
                    return Err(CommandError::Sandbox(
                        SandboxRuntimeError::UnsupportedPolicy,
                    ));
                }
            };
            execute_host_command(
                workspace,
                request,
                cancellation,
                cwd_scope,
                execution_boundary,
                metadata.permission_profile_hash,
            )
            .await
        }
        Ok(ExecutionTarget::ManagedSandbox { .. }) => {
            let sandbox_request = SandboxCommandRequest {
                workspace_root: workspace.root().to_path_buf(),
                program: request.program.clone(),
                args: request.args.clone(),
                cwd: request.cwd.clone(),
                timeout_ms: request.timeout_ms,
                response_path: Default::default(),
                cancellation_path: Default::default(),
            };
            let cancellation_for_runner = cancellation.clone();
            let response = tokio::task::spawn_blocking(move || {
                run_workspace_command(sandbox_request, move || {
                    cancellation_for_runner.is_cancelled()
                })
            })
            .await
            .map_err(|error| CommandError::SandboxCommandFailed {
                kind: "runner_join_failed".to_owned(),
                message: error.to_string(),
            })??;
            match response {
                SandboxCommandResponse::Completed(output) => Ok(CommandOutput {
                    exit_code: output.exit_code,
                    stdout: output.stdout,
                    stderr: output.stderr,
                    truncated: output.truncated,
                    process_boundary: ProcessBoundary::WindowsJobObject,
                    execution_boundary: CommandExecutionBoundary::WorkspaceSandbox,
                    permission_profile_hash: metadata.permission_profile_hash,
                }),
                SandboxCommandResponse::Failed { kind, message } => match kind.as_str() {
                    "cancelled" => Err(CommandError::Cancelled),
                    "timed_out" | "timedout" => Err(CommandError::TimedOut),
                    _ => Err(CommandError::SandboxCommandFailed { kind, message }),
                },
            }
        }
        Err(error) => Err(CommandError::Sandbox(error)),
    }
}

const fn permission_profile(policy: CommandExecutionPolicy) -> PermissionProfile {
    match policy {
        CommandExecutionPolicy::WorkspaceSandbox => PermissionProfile::workspace_write(),
        CommandExecutionPolicy::ApprovedHost | CommandExecutionPolicy::SystemFullAccess => {
            PermissionProfile::disabled()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::command_execution_metadata;
    use crate::CommandExecutionPolicy;
    use local_agent_sandbox::PermissionProfile;

    #[test]
    fn workspace_policy_resolves_to_managed_profile_and_health() {
        let metadata = command_execution_metadata(CommandExecutionPolicy::WorkspaceSandbox)
            .expect("metadata should be deterministic");
        assert_eq!(
            metadata.permission_profile,
            PermissionProfile::workspace_write()
        );
        assert!(metadata.sandbox_health.is_some());
    }

    #[test]
    fn approved_host_is_explicitly_unsandboxed() {
        let metadata = command_execution_metadata(CommandExecutionPolicy::ApprovedHost)
            .expect("metadata should be deterministic");
        assert_eq!(metadata.permission_profile, PermissionProfile::disabled());
        assert!(metadata.sandbox_health.is_none());
    }
}
