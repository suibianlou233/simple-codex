use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Filesystem boundary requested from a managed sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemPermission {
    ReadOnly,
    WorkspaceWrite,
}

/// Network boundary requested from a managed sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPermission {
    Denied,
    Allowed,
}

/// The small set of managed profiles Simple promises to enforce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedPermissionProfile {
    pub file_system: FileSystemPermission,
    pub network: NetworkPermission,
}

/// Requested technical boundary for an action.
///
/// `Disabled` is intentionally explicit: host execution must never be
/// presented as a permissive sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "enforcement",
    content = "permissions",
    rename_all = "snake_case"
)]
pub enum PermissionProfile {
    Managed(ManagedPermissionProfile),
    Disabled,
}

impl PermissionProfile {
    #[must_use]
    pub const fn read_only() -> Self {
        Self::Managed(ManagedPermissionProfile {
            file_system: FileSystemPermission::ReadOnly,
            network: NetworkPermission::Denied,
        })
    }

    #[must_use]
    pub const fn workspace_write() -> Self {
        Self::Managed(ManagedPermissionProfile {
            file_system: FileSystemPermission::WorkspaceWrite,
            network: NetworkPermission::Allowed,
        })
    }

    #[must_use]
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    /// Stable, secret-free identity suitable for diagnostics and replay.
    pub fn stable_hash(self) -> Result<String, serde_json::Error> {
        let encoded = serde_json::to_vec(&self)?;
        let mut hasher = Sha256::new();
        hasher.update(encoded);
        Ok(format!("{:x}", hasher.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::PermissionProfile;

    #[test]
    fn permission_profile_hash_is_stable_and_distinguishes_host_access() {
        let first = PermissionProfile::workspace_write()
            .stable_hash()
            .expect("workspace profile should serialize");
        let second = PermissionProfile::workspace_write()
            .stable_hash()
            .expect("workspace profile should serialize again");
        let disabled = PermissionProfile::disabled()
            .stable_hash()
            .expect("disabled profile should serialize");

        assert_eq!(first, second);
        assert_ne!(first, disabled);
    }
}
