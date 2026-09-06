use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkAccess {
    #[default]
    Restricted,
    Enabled,
}

impl NetworkAccess {
    fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SandboxPolicy {
    #[serde(rename = "danger-full-access")]
    DangerFullAccess,
    #[serde(rename = "read-only")]
    ReadOnly {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        network_access: bool,
    },
    #[serde(rename = "external-sandbox")]
    ExternalSandbox {
        #[serde(default)]
        network_access: NetworkAccess,
    },
    #[serde(rename = "workspace-write")]
    WorkspaceWrite {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        writable_roots: Vec<PathBuf>,
        #[serde(default)]
        network_access: bool,
        #[serde(default)]
        exclude_tmpdir_env_var: bool,
        #[serde(default)]
        exclude_slash_tmp: bool,
    },
}

impl SandboxPolicy {
    pub fn new_read_only_policy() -> Self {
        Self::ReadOnly {
            network_access: false,
        }
    }

    pub fn new_workspace_write_policy() -> Self {
        Self::WorkspaceWrite {
            writable_roots: Vec::new(),
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        }
    }

    pub fn has_full_disk_read_access(&self) -> bool {
        true
    }

    pub fn has_full_network_access(&self) -> bool {
        match self {
            Self::DangerFullAccess => true,
            Self::ExternalSandbox { network_access } => network_access.is_enabled(),
            Self::ReadOnly { network_access } | Self::WorkspaceWrite { network_access, .. } => {
                *network_access
            }
        }
    }
}

pub fn parse_policy(value: &str) -> Result<SandboxPolicy> {
    match value {
        "read-only" => Ok(SandboxPolicy::new_read_only_policy()),
        "workspace-write" => Ok(SandboxPolicy::new_workspace_write_policy()),
        "danger-full-access" | "external-sandbox" => {
            anyhow::bail!("DangerFullAccess and ExternalSandbox are not supported for sandboxing")
        }
        other => {
            let parsed: SandboxPolicy = serde_json::from_str(other)?;
            if matches!(
                parsed,
                SandboxPolicy::DangerFullAccess | SandboxPolicy::ExternalSandbox { .. }
            ) {
                anyhow::bail!(
                    "DangerFullAccess and ExternalSandbox are not supported for sandboxing"
                );
            }
            Ok(parsed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_external_sandbox_json() {
        let payload = serde_json::to_string(&SandboxPolicy::ExternalSandbox {
            network_access: NetworkAccess::Enabled,
        })
        .expect("serialize policy");
        let error = parse_policy(&payload).expect_err("external policy must be rejected");
        assert!(error.to_string().contains("ExternalSandbox"));
    }

    #[test]
    fn workspace_policy_round_trips() {
        let policy = SandboxPolicy::WorkspaceWrite {
            writable_roots: Vec::new(),
            network_access: true,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        };
        let encoded = serde_json::to_string(&policy).expect("serialize policy");
        assert_eq!(parse_policy(&encoded).expect("parse policy"), policy);
    }
}
