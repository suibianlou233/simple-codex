use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{NetworkPermission, PermissionProfile};

/// Setup marker version used by the Simple Windows sandbox.
pub const CURRENT_WINDOWS_SETUP_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxBackend {
    WindowsSimple,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SandboxHealth {
    Ready {
        backend: SandboxBackend,
        setup_version: u32,
    },
    NeedsSetup {
        backend: SandboxBackend,
        expected_setup_version: u32,
    },
    Drifted {
        backend: SandboxBackend,
        expected_setup_version: u32,
        installed_setup_version: Option<u32>,
    },
    UnsupportedPlatform,
}

impl SandboxHealth {
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    #[must_use]
    pub const fn backend(&self) -> Option<SandboxBackend> {
        match self {
            Self::Ready { backend, .. }
            | Self::NeedsSetup { backend, .. }
            | Self::Drifted { backend, .. } => Some(*backend),
            Self::UnsupportedPlatform => None,
        }
    }

    #[must_use]
    pub const fn status_name(&self) -> &'static str {
        match self {
            Self::Ready { .. } => "ready",
            Self::NeedsSetup { .. } => "needs_setup",
            Self::Drifted { .. } => "drifted",
            Self::UnsupportedPlatform => "unsupported_platform",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxCapability {
    pub backend: SandboxBackend,
    pub filesystem_enforced: bool,
    pub network_enforced: bool,
    pub child_processes_inherit_boundary: bool,
}

impl SandboxCapability {
    #[must_use]
    pub const fn windows_simple() -> Self {
        Self {
            backend: SandboxBackend::WindowsSimple,
            filesystem_enforced: true,
            network_enforced: false,
            child_processes_inherit_boundary: true,
        }
    }

    #[must_use]
    pub const fn can_enforce(self, profile: PermissionProfile) -> bool {
        match profile {
            PermissionProfile::Managed(profile) => {
                let network_supported = match profile.network {
                    NetworkPermission::Allowed => true,
                    NetworkPermission::Denied => self.network_enforced,
                };
                self.filesystem_enforced
                    && self.child_processes_inherit_boundary
                    && network_supported
            }
            PermissionProfile::Disabled => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionTarget {
    ManagedSandbox { backend: SandboxBackend },
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SandboxRuntimeError {
    #[error("当前设备的项目沙箱不可用：{status}")]
    Unavailable {
        status: &'static str,
        health: SandboxHealth,
    },
    #[error("当前沙箱后端不能完整执行请求的权限策略")]
    UnsupportedPolicy,
}

/// Pure planner used by the action gate and command runtime.
///
/// It never falls back from a managed profile to host execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxRuntime {
    health: SandboxHealth,
    capability: Option<SandboxCapability>,
}

impl SandboxRuntime {
    #[must_use]
    pub const fn new(health: SandboxHealth, capability: Option<SandboxCapability>) -> Self {
        Self { health, capability }
    }

    #[must_use]
    pub fn current() -> Self {
        if cfg!(windows) {
            return Self::new(
                crate::windows::current_windows_health(),
                Some(SandboxCapability::windows_simple()),
            );
        }
        Self::new(SandboxHealth::UnsupportedPlatform, None)
    }

    #[must_use]
    pub fn health(&self) -> &SandboxHealth {
        &self.health
    }

    pub fn plan(&self, profile: PermissionProfile) -> Result<ExecutionTarget, SandboxRuntimeError> {
        if profile == PermissionProfile::Disabled {
            return Ok(ExecutionTarget::Host);
        }
        if !self.health.is_ready() {
            return Err(SandboxRuntimeError::Unavailable {
                status: self.health.status_name(),
                health: self.health.clone(),
            });
        }
        let Some(capability) = self.capability else {
            return Err(SandboxRuntimeError::UnsupportedPolicy);
        };
        if !capability.can_enforce(profile) {
            return Err(SandboxRuntimeError::UnsupportedPolicy);
        }
        Ok(ExecutionTarget::ManagedSandbox {
            backend: capability.backend,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CURRENT_WINDOWS_SETUP_VERSION, ExecutionTarget, SandboxBackend, SandboxCapability,
        SandboxHealth, SandboxRuntime, SandboxRuntimeError,
    };
    use crate::PermissionProfile;

    #[test]
    fn unavailable_managed_profile_fails_closed() {
        let runtime = SandboxRuntime::new(
            SandboxHealth::NeedsSetup {
                backend: SandboxBackend::WindowsSimple,
                expected_setup_version: CURRENT_WINDOWS_SETUP_VERSION,
            },
            Some(SandboxCapability::windows_simple()),
        );
        let result = runtime.plan(PermissionProfile::workspace_write());

        assert!(matches!(
            result,
            Err(SandboxRuntimeError::Unavailable {
                status: "needs_setup",
                ..
            })
        ));
    }

    #[test]
    fn disabled_profile_is_explicit_host_execution() {
        let runtime = SandboxRuntime::new(SandboxHealth::UnsupportedPlatform, None);
        assert_eq!(
            runtime.plan(PermissionProfile::disabled()),
            Ok(ExecutionTarget::Host)
        );
    }

    #[test]
    fn ready_backend_accepts_supported_managed_profile() {
        let runtime = SandboxRuntime::new(
            SandboxHealth::Ready {
                backend: SandboxBackend::WindowsSimple,
                setup_version: CURRENT_WINDOWS_SETUP_VERSION,
            },
            Some(SandboxCapability::windows_simple()),
        );
        assert_eq!(
            runtime.plan(PermissionProfile::workspace_write()),
            Ok(ExecutionTarget::ManagedSandbox {
                backend: SandboxBackend::WindowsSimple,
            })
        );
    }

    #[test]
    fn ready_backend_without_complete_boundary_rejects_policy() {
        let runtime = SandboxRuntime::new(
            SandboxHealth::Ready {
                backend: SandboxBackend::WindowsSimple,
                setup_version: CURRENT_WINDOWS_SETUP_VERSION,
            },
            Some(SandboxCapability {
                backend: SandboxBackend::WindowsSimple,
                filesystem_enforced: true,
                network_enforced: false,
                child_processes_inherit_boundary: true,
            }),
        );
        assert_eq!(
            runtime.plan(PermissionProfile::read_only()),
            Err(SandboxRuntimeError::UnsupportedPolicy)
        );
    }
}
