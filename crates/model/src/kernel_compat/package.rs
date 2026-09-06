use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{CodexKernelConfig, ResponsesGatewayConfig};

const UPSTREAM: &str = "2b7c279735d0d096cf7b34fe98938f46792f4d4f";
const ADAPTER: &str = "simple-slim-v1";
const DATA_CONTRACT: &str = "simple-slim-project-memory-v1";
const CANDIDATE_UPSTREAM: &str = "28327355b861ab6cc76b01c7248663eb1be440cf";
const CANDIDATE_ADAPTER: &str = "official-283-candidate-v1";
const CANDIDATE_DATA: &str = "simple-upstream-283-isolated-v1";
const CANDIDATE_CAPABILITIES: &[&str] = &[
    "stdio-thread-turn",
    "approval-interrupt",
    "paginated-history",
    "code-mode",
    "official-provider-config",
    "strict-config",
    "isolated-candidate-only",
];
const CAPABILITIES: &[&str] = &[
    "stdio-thread-turn",
    "approval-interrupt",
    "paginated-history",
    "code-mode",
    "simple-gateway-env",
    "strict-config",
    "project-memory-scope",
    "memory-forget",
];

#[derive(Debug, Error)]
pub enum KernelPackageError {
    #[error("无法读取内核版本包：{0}")]
    Io(#[from] std::io::Error),
    #[error("内核版本清单格式无效：{0}")]
    Json(#[from] serde_json::Error),
    #[error("内核版本包不兼容：{0}")]
    Incompatible(&'static str),
    #[error("内核组件校验失败：{0}")]
    Integrity(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    id: String,
    upstream_revision: String,
    adapter: String,
    data_contract: String,
    target: String,
    provenance: String,
    capabilities: Vec<String>,
    files: BTreeMap<String, String>,
}

/// Frozen manifest and hashes. A selected package is checked again immediately
/// before launch; editing a selection file does not replace a running kernel.
#[derive(Debug, Clone)]
pub struct KernelPackage {
    root: PathBuf,
    manifest: Manifest,
}

impl KernelPackage {
    pub fn load(path: &Path) -> Result<Self, KernelPackageError> {
        let path = fs::canonicalize(path)?;
        let bytes = fs::read(&path)?;
        if bytes.len() > 128 * 1024 {
            return Err(KernelPackageError::Incompatible("清单超过大小限制"));
        }
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        let legacy = manifest.adapter == ADAPTER
            && manifest.upstream_revision == UPSTREAM
            && manifest.data_contract == DATA_CONTRACT
            && manifest.provenance == "legacy-modified-import";
        let candidate = manifest.adapter == CANDIDATE_ADAPTER
            && manifest.upstream_revision == CANDIDATE_UPSTREAM
            && manifest.data_contract == CANDIDATE_DATA
            && manifest.provenance == "pinned-upstream-with-recorded-patches";
        if manifest.schema_version != 1 || !(legacy || candidate) {
            return Err(KernelPackageError::Incompatible(
                "需要已实现并验证的版本适配器",
            ));
        }
        let target = if cfg!(all(windows, target_arch = "x86_64")) {
            "x86_64-pc-windows-msvc"
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            "x86_64-unknown-linux-gnu"
        } else {
            return Err(KernelPackageError::Incompatible("尚未登记的平台"));
        };
        if manifest.target != target
            || (candidate && !cfg!(windows))
            || manifest.id.is_empty()
            || manifest.id.len() > 100
            || !manifest
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return Err(KernelPackageError::Incompatible("平台或来源记录无效"));
        }
        let required_capabilities = if legacy {
            CAPABILITIES
        } else {
            CANDIDATE_CAPABILITIES
        };
        if required_capabilities
            .iter()
            .any(|required| !manifest.capabilities.iter().any(|c| c == required))
        {
            return Err(KernelPackageError::Incompatible("缺少当前 Simple 必需能力"));
        }
        let root = path
            .parent()
            .ok_or(KernelPackageError::Incompatible("清单没有父目录"))?
            .to_owned();
        let package = Self { root, manifest };
        if candidate && !package.manifest.files.contains_key("changes.patch") {
            return Err(KernelPackageError::Integrity("changes.patch".into()));
        }
        for required in [
            binary("codex-app-server"),
            binary("codex-code-mode-host"),
            binary("apply_patch"),
            "LICENSE".into(),
            "NOTICE".into(),
            "MODIFICATIONS.md".into(),
            "PATCHES.json".into(),
        ] {
            if !package.manifest.files.contains_key(&required) {
                return Err(KernelPackageError::Integrity(required));
            }
        }
        package.verify()?;
        Ok(package)
    }

    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub(crate) fn adapter(&self) -> super::KernelAdapter {
        // load() validates the profile; never infer compatibility from semver.
        if self.manifest.adapter == ADAPTER {
            super::KernelAdapter::SlimV1
        } else {
            super::KernelAdapter::Upstream283
        }
    }

    /// Experimental packages are usable by isolated backend acceptance, not by
    /// the current desktop persistence/memory contract. Never silently downgrade.
    pub fn require_desktop_compatible(&self) -> Result<(), KernelPackageError> {
        if self.manifest.adapter != ADAPTER {
            return Err(KernelPackageError::Incompatible(
                "该官方内核仍是隔离验收候选版；项目记忆、模型切换和历史迁移未验收，不能替换当前桌面内核",
            ));
        }
        Ok(())
    }

    pub fn executable(&self) -> PathBuf {
        self.root.join(binary("codex-app-server"))
    }

    pub fn verify(&self) -> Result<(), KernelPackageError> {
        for (name, expected) in &self.manifest.files {
            // A version package is flat. Reject traversal and Windows ADS even
            // when validating a Windows manifest from another platform.
            if name.is_empty()
                || name == "."
                || name == ".."
                || name.contains(['/', '\\', ':'])
                || expected.len() != 64
                || !expected.bytes().all(|c| c.is_ascii_hexdigit())
            {
                return Err(KernelPackageError::Integrity(name.clone()));
            }
            let path = self.root.join(name);
            let meta = fs::symlink_metadata(&path)?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err(KernelPackageError::Integrity(name.clone()));
                }
            }
            if !meta.is_file()
                || meta.file_type().is_symlink()
                || fs::canonicalize(&path)?.parent() != Some(self.root.as_path())
            {
                return Err(KernelPackageError::Integrity(name.clone()));
            }
            let mut file = fs::File::open(path)?;
            let mut hash = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected) {
                return Err(KernelPackageError::Integrity(name.clone()));
            }
        }
        Ok(())
    }
}

fn binary(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.into()
    }
}

#[derive(Debug, Clone)]
pub enum KernelSelection {
    /// Existing installations remain usable while being imported. This path
    /// does not claim checksum/provenance validation.
    Legacy(PathBuf),
    Package(KernelPackage),
}

impl KernelSelection {
    pub fn history_layout(&self) -> super::CodexHistoryLayout {
        match self {
            Self::Legacy(_) => super::KernelAdapter::SlimV1,
            Self::Package(package) => package.adapter(),
        }
        .history_layout()
    }

    pub fn supports_native_memory(&self) -> bool {
        !self.is_upstream_preview()
    }

    pub fn default_history_home(&self, task_id: &str) -> String {
        let key = if self.is_upstream_preview() {
            format!("upstream-283:task:{task_id}")
        } else {
            "simple-native-history-v2".to_owned()
        };
        format!("{:x}", Sha256::digest(key.as_bytes()))
    }

    /// This profile is a desktop preview with isolated history, not a feature-
    /// equivalent in-place upgrade of the legacy distribution.
    pub fn is_upstream_preview(&self) -> bool {
        matches!(self, Self::Package(package) if package.manifest.adapter == CANDIDATE_ADAPTER)
    }

    pub fn executable(&self) -> PathBuf {
        match self {
            Self::Legacy(path) => path.clone(),
            Self::Package(package) => package.executable(),
        }
    }

    pub fn configuration(
        &self,
        home: PathBuf,
        gateway: ResponsesGatewayConfig,
    ) -> CodexKernelConfig {
        match self {
            Self::Legacy(path) => CodexKernelConfig::new(path, home, gateway),
            Self::Package(package) => {
                let mut config = CodexKernelConfig::new(package.executable(), home, gateway);
                config.package = Some(package.clone());
                config
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, Manifest) {
        let root = tempfile::tempdir().expect("fixture");
        let mut files = BTreeMap::new();
        for name in [
            binary("codex-app-server"),
            binary("codex-code-mode-host"),
            binary("apply_patch"),
            "LICENSE".into(),
            "NOTICE".into(),
            "MODIFICATIONS.md".into(),
            "PATCHES.json".into(),
        ] {
            fs::write(root.path().join(&name), name.as_bytes()).expect("component");
            files.insert(
                name.clone(),
                format!("{:x}", Sha256::digest(name.as_bytes())),
            );
        }
        let manifest = Manifest {
            schema_version: 1,
            id: "fixture-1".into(),
            upstream_revision: UPSTREAM.into(),
            adapter: ADAPTER.into(),
            data_contract: DATA_CONTRACT.into(),
            provenance: "legacy-modified-import".into(),
            target: if cfg!(windows) {
                "x86_64-pc-windows-msvc"
            } else {
                "x86_64-unknown-linux-gnu"
            }
            .into(),
            capabilities: CAPABILITIES.iter().map(|s| (*s).into()).collect(),
            files,
        };
        let path = root.path().join("kernel.json");
        write_manifest(&path, &manifest);
        (root, path, manifest)
    }

    fn write_manifest(path: &Path, manifest: &Manifest) {
        fs::write(path, serde_json::to_vec(manifest).expect("json")).expect("manifest");
    }

    #[test]
    fn frozen_selection_rejects_modified_components() {
        let (root, path, _) = fixture();
        let package = KernelPackage::load(&path).expect("valid package");
        assert_eq!(package.id(), "fixture-1");
        fs::write(root.path().join(binary("apply_patch")), b"replacement").expect("tamper");
        assert!(matches!(
            package.verify(),
            Err(KernelPackageError::Integrity(_))
        ));
    }

    #[test]
    fn legacy_history_identity_and_layout_do_not_change_during_adapter_cleanup() {
        let (_root, path, _) = fixture();
        for selection in [
            KernelSelection::Legacy(PathBuf::from("fixture.exe")),
            KernelSelection::Package(KernelPackage::load(&path).expect("package")),
        ] {
            assert!(selection.supports_native_memory());
            let home = format!("{:x}", Sha256::digest(b"simple-native-history-v2"));
            assert_eq!(selection.default_history_home("task-a"), home);
            assert_eq!(selection.default_history_home("task-b"), home);
            assert_eq!(selection.history_layout().database_name, "state_5.sqlite");
        }
    }

    #[test]
    fn incompatible_contract_and_missing_capability_are_rejected() {
        let (_root, path, original) = fixture();
        for field in ["adapter", "data", "revision", "capability", "helper"] {
            let mut m = original.clone();
            match field {
                "adapter" => m.adapter = "upstream-v99".into(),
                "data" => m.data_contract = "incompatible-history".into(),
                "revision" => m.upstream_revision = "unknown".into(),
                "capability" => m.capabilities.clear(),
                _ => {
                    m.files.remove(&binary("apply_patch"));
                }
            }
            write_manifest(&path, &m);
            assert!(KernelPackage::load(&path).is_err(), "{field}");
        }
    }

    #[test]
    fn package_cannot_reference_files_outside_its_directory() {
        let (_root, path, mut m) = fixture();
        m.files.insert("../outside".into(), "0".repeat(64));
        write_manifest(&path, &m);
        assert!(matches!(
            KernelPackage::load(&path),
            Err(KernelPackageError::Integrity(_))
        ));
    }

    #[test]
    #[cfg(windows)]
    fn candidate_package_never_claims_desktop_compatibility() {
        let (_root, path, mut m) = fixture();
        m.adapter = CANDIDATE_ADAPTER.into();
        m.upstream_revision = CANDIDATE_UPSTREAM.into();
        m.data_contract = CANDIDATE_DATA.into();
        m.provenance = "pinned-upstream-with-recorded-patches".into();
        m.capabilities = CANDIDATE_CAPABILITIES.iter().map(|s| (*s).into()).collect();
        std::fs::write(
            path.parent().expect("parent").join("changes.patch"),
            "fixture",
        )
        .expect("patch");
        m.files.insert(
            "changes.patch".into(),
            format!("{:x}", Sha256::digest(b"fixture")),
        );
        write_manifest(&path, &m);
        let package = KernelPackage::load(&path).expect("candidate package");
        assert!(package.require_desktop_compatible().is_err());
        let selection = KernelSelection::Package(package);
        assert!(!selection.supports_native_memory());
        assert_eq!(
            selection.default_history_home("task-a"),
            format!("{:x}", Sha256::digest(b"upstream-283:task:task-a"))
        );
        assert_ne!(
            selection.default_history_home("task-a"),
            selection.default_history_home("task-b")
        );
        assert_eq!(selection.history_layout().database_name, "state_5.sqlite");
    }
}
