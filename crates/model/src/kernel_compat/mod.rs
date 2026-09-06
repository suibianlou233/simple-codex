//! Simple-owned boundary for versioned Codex distributions. Upstream source is
//! not a dependency of this crate: integration happens only through stdio RPC.
mod codex_features;
mod codex_kernel;
mod codex_kernel_event;
#[cfg(test)]
mod codex_model_revisions_tests;
mod codex_process;
mod codex_project_memory;
mod codex_session;
mod codex_submission;
mod codex_task_control;
mod history_layout;
mod package;
pub use codex_features::CodexFeatureBridge;
pub use codex_features::CodexFeatureError;
pub use codex_features::LocalMcpServerConfig;
pub use codex_kernel::CodexKernelClient;
pub use codex_kernel::CodexKernelConfig;
pub use codex_kernel::CodexKernelError;
pub use codex_kernel::CodexKernelEvents;
pub use codex_kernel::CodexKernelProcess;
pub use codex_kernel::CodexKernelWireMessage;
pub use codex_kernel_event::CodexApprovalKind;
pub use codex_kernel_event::CodexApprovalRequest;
pub use codex_kernel_event::CodexKernelEvent;
pub use codex_kernel_event::CodexMessagePhase;
pub use codex_kernel_event::CodexThreadItem;
pub use codex_kernel_event::CodexTurnStatus;
pub use codex_project_memory::CodexProjectMemory;
pub use codex_session::CodexHistoryItem;
pub use codex_session::CodexHistoryTurn;
pub use codex_session::CodexRpc;
pub use codex_session::CodexSessionBridge;
pub use codex_session::CodexSessionError;
pub use codex_session::CodexThreadSnapshot;
pub use codex_session::{CodexForkPoint, CodexThreadOptions};
pub use codex_submission::find_codex_submission;
pub use codex_submission::{CodexStartReceipt, start_codex_turn_recovering};
pub use codex_task_control::CodexTaskTreeWatch;
pub use codex_task_control::find_codex_ancestor;
pub use codex_task_control::stop_codex_task_tree;
pub use codex_task_control::{CodexChildOutcome, CodexChildStatus};
pub use history_layout::CodexHistoryLayout;
pub(crate) mod slim_v1;
mod upstream_283;

pub use package::{KernelPackage, KernelPackageError, KernelSelection};

/// Add a profile only after its wire/configuration/data contracts are checked.
/// The transport never guesses a profile from an executable's filename/version.
#[derive(Debug, Clone, Copy)]
pub(crate) enum KernelAdapter {
    SlimV1,
    Upstream283,
}

impl KernelAdapter {
    pub(crate) fn history_layout(self) -> CodexHistoryLayout {
        match self {
            Self::SlimV1 | Self::Upstream283 => CodexHistoryLayout::STATE_V5,
        }
    }

    pub(crate) fn validate_model_alias(
        self,
        current: &str,
        next: &str,
    ) -> Result<(), CodexKernelError> {
        if matches!(self, Self::Upstream283) && current != next {
            return Err(KernelPackageError::Incompatible(
                "候选官方内核尚未验收动态模型切换，请使用独立实例",
            )
            .into());
        }
        Ok(())
    }

    pub(crate) fn prepare_home(
        self,
        home: &std::path::Path,
    ) -> Result<(), crate::CodexKernelError> {
        match self {
            Self::SlimV1 => slim_v1::prepare_home(home),
            Self::Upstream283 => upstream_283::prepare_home(home),
        }
    }

    pub(crate) fn configure(
        self,
        command: &mut std::process::Command,
        arguments: &[std::ffi::OsString],
        memory: Option<&crate::CodexProjectMemory>,
        gateway: &crate::ResponsesGatewayHandle,
        home: &std::path::Path,
    ) -> Result<(), crate::CodexKernelError> {
        match self {
            Self::SlimV1 => slim_v1::configure(command, arguments, memory, gateway),
            Self::Upstream283 => upstream_283::configure(command, arguments, memory, gateway, home),
        }
    }

    pub(crate) fn feature_configuration(self) -> Option<serde_json::Value> {
        match self {
            Self::SlimV1 => Some(slim_v1::feature_configuration()),
            Self::Upstream283 => None,
        }
    }
}
