//! Desktop ownership for the official-kernel preview. Never migrate old tasks.
use super::*;

pub(crate) const PREVIEW_NOTICE: &str = "官方 28327355 内核预览：自动长期记忆/遗忘未迁移；同一任务更换模型后需重启。Windows 复合命令失败判定及 V8 内部防护仍有已知差异。此实例使用独立数据，不是旧版的完整等价替换。";

pub(crate) struct Startup {
    pub data: PathBuf,
    pub selection: Result<KernelSelection, String>,
    pub preview: bool,
}

impl Startup {
    pub fn resolve(base: &Path, resources: &Path) -> Result<Self, DesktopError> {
        let selection = resolve_codex_selection(resources).map_err(command_error);
        // An explicit invalid package fails before any database is opened.
        if cfg!(feature = "bundled-official-kernel")
            || std::env::var_os("SIMPLE_CODEX_KERNEL_MANIFEST").is_some()
        {
            selection
                .as_ref()
                .map_err(|e| DesktopError::KernelSelection(e.clone()))?;
        }
        let preview = selection
            .as_ref()
            .is_ok_and(KernelSelection::is_upstream_preview);
        let data = if preview {
            base.join("kernel-slots").join("official-283-v1")
        } else {
            base.to_owned()
        };
        // Refuse redirected preview data; don't accidentally recover another installation.
        if preview {
            for path in data.ancestors().collect::<Vec<_>>().into_iter().rev() {
                let metadata = match std::fs::symlink_metadata(path) {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error.into()),
                };
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(DesktopError::UnsafeMemoryPath);
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(DesktopError::UnsafeMemoryPath);
                }
            }
        }
        std::fs::create_dir_all(&data)?;
        Ok(Self {
            data,
            selection,
            preview,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "bundled-official-kernel")]
    #[test]
    fn bundled_missing_package_never_opens_legacy_data() {
        let root = tempfile::tempdir().expect("fixture");
        let data = root.path().join("data");
        let resources = root.path().join("resources");
        std::fs::create_dir(&resources).expect("resources");
        std::fs::write(resources.join("codex-app-server.exe"), b"legacy decoy").expect("decoy");
        assert!(Startup::resolve(&data, &resources).is_err());
        assert!(!data.exists(), "must fail before opening old data");
    }

    #[cfg(feature = "bundled-official-kernel")]
    #[test]
    fn bundled_package_selects_isolated_data_and_rejects_corruption() {
        let root = tempfile::tempdir().expect("fixture");
        let resources = root.path().join("resources");
        let relative = "kernels/official-283-windows-candidate-1";
        let destination = resources.join(relative);
        std::fs::create_dir_all(&destination).expect("resources");
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../kernels/packages/official-283-windows-candidate-1");
        for entry in std::fs::read_dir(&source).expect("fixed package") {
            let entry = entry.expect("package entry");
            std::fs::copy(entry.path(), destination.join(entry.file_name())).expect("copy fixture");
        }
        let base = root.path().join("data");
        let startup = Startup::resolve(&base, &resources).expect("bundled startup");
        assert!(startup.preview);
        assert_eq!(startup.data, base.join("kernel-slots/official-283-v1"));
        assert!(startup.selection.expect("selection").is_upstream_preview());
        assert!(!base.join("local-agent.db").exists());
        std::fs::write(destination.join("NOTICE"), b"modified fixture").expect("corrupt fixture");
        let unopened = root.path().join("unopened-data");
        assert!(Startup::resolve(&unopened, &resources).is_err());
        assert!(!unopened.exists());
    }

    #[test]
    fn settings_import_keeps_history_and_credential_mutations_separate() {
        let root = tempfile::tempdir().expect("fixture");
        let source = root.path().join("old.db");
        let mut old = Storage::open(&source).expect("old db");
        let profile = local_agent_storage::NewModelProfile {
            profile_id: Uuid::new_v4().to_string(),
            name: "fixture".into(),
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "fixture-model".into(),
            dialect: "standard".into(),
            credential_ref: "old-fixture-key-ref".into(),
            max_output_tokens: None,
            context_window_tokens: None,
            timeout_ms: 30000,
            is_default: true,
            created_at_ms: 1,
            updated_at_ms: 1,
        };
        old.upsert_model_profile(profile.clone())
            .expect("source config");
        old.create_project(local_agent_storage::NewProject {
            project_id: "original-project".into(),
            root: "original-fixture-root".into(),
            name: "original fixture".into(),
        })
        .expect("original project");
        old.create_task(local_agent_storage::NewTask {
            task_id: "original-task".into(),
            project_id: "original-project".into(),
            title: "must stay in original data".into(),
            created_at_ms: 1,
        })
        .expect("original task");
        let slot = root.path().join("preview");
        std::fs::create_dir(&slot).expect("slot");
        let state =
            DesktopState::open(&slot.join("new.db"), Err("fixture".into())).expect("preview state");
        let secrets = Arc::new(crate::secrets::MemorySecretStore::new());
        secrets
            .set(&profile.credential_ref, "fake-fixture-secret")
            .expect("dummy key");
        state.lock().expect("state").secret_store = secrets.clone();
        import_profiles(&state, &source).expect("import");
        import_profiles(&state, &source).expect("idempotent");
        let runtime = state.lock().expect("state");
        let copied = runtime.storage.list_model_profiles().expect("profiles");
        assert_eq!(copied.len(), 1);
        assert_ne!(copied[0].credential_ref, profile.credential_ref);
        assert_ne!(copied[0].profile_id, profile.profile_id);
        assert_eq!(
            secrets.get(&copied[0].credential_ref).expect("copy"),
            "fake-fixture-secret"
        );
        secrets
            .set(&copied[0].credential_ref, "changed-preview-fixture")
            .expect("edit copied key");
        assert_eq!(
            secrets.get(&profile.credential_ref).expect("old key"),
            "fake-fixture-secret"
        );
        assert_eq!(
            old.list_model_profiles().expect("old")[0].credential_ref,
            profile.credential_ref
        );
        assert!(runtime.storage.list_tasks().expect("tasks").is_empty());
        assert!(
            runtime
                .storage
                .list_projects()
                .expect("projects")
                .is_empty()
        );
        assert_eq!(old.list_tasks().expect("original tasks").len(), 1);
    }
}

/// First-run settings import only. Keys are copied within the OS credential
/// store to NEW references; editing a preview profile cannot change the old key.
pub(crate) fn import_profiles(state: &DesktopState, source: &Path) -> Result<(), DesktopError> {
    if !source.is_file() {
        return Ok(());
    }
    let mut runtime = state.lock()?;
    if !runtime.storage.list_model_profiles()?.is_empty() {
        return Ok(());
    }
    for profile in Storage::read_model_profiles_read_only(source)? {
        let id = Uuid::new_v4().to_string();
        let credential_ref = format!("model-profile:{id}");
        match runtime.secret_store.get(&profile.credential_ref) {
            Ok(secret) => runtime.secret_store.set(&credential_ref, &secret)?,
            Err(SecretStoreError::NotFound) => {}
            Err(error) => return Err(error.into()),
        }
        runtime
            .storage
            .upsert_model_profile(local_agent_storage::NewModelProfile {
                profile_id: id,
                name: profile.name,
                base_url: profile.base_url,
                model: profile.model,
                dialect: profile.dialect,
                credential_ref,
                max_output_tokens: profile.max_output_tokens,
                context_window_tokens: profile.context_window_tokens,
                timeout_ms: profile.timeout_ms,
                is_default: profile.is_default,
                created_at_ms: profile.created_at_ms,
                updated_at_ms: profile.updated_at_ms,
            })?;
    }
    Ok(())
}

pub(crate) fn require_native_memory(state: &DesktopState) -> Result<(), String> {
    if !state
        .selected_kernel()
        .map_err(command_error)?
        .supports_native_memory()
    {
        return Err(
            "此新内核实例暂不支持自动长期记忆、遗忘和原记忆区管理；旧版记忆没有被删除或迁移。"
                .into(),
        );
    }
    Ok(())
}
