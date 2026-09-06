//! Compatibility profile for the imported, modified 2b7c279 kernel.
//! These settings are NOT an official Codex protocol guarantee.
use std::{path::Path, process::Command};

use serde_json::{Value, json};

use crate::{CodexKernelError, CodexProjectMemory, ResponsesGatewayHandle};

const INITIAL_CONFIG: &str = r#"# Managed by Simple; compatibility profile: simple-slim-v1.
[features]
code_mode = true
code_mode_host = true
code_mode_interrupt = true
memories = true

[memories]
generate_memories = true
use_memories = true
dedicated_tools = true
disable_on_external_context = true
"#;

pub(crate) fn prepare_home(home: &Path) -> Result<(), CodexKernelError> {
    if home.join("simple-kernel-contract").exists() {
        return Err(crate::KernelPackageError::Incompatible(
            "历史内核不能打开其他内核的数据槽；请恢复对应旧数据副本",
        )
        .into());
    }
    let path = home.join("config.toml");
    if !path.exists() {
        std::fs::write(path, INITIAL_CONFIG).map_err(CodexKernelError::PrepareConfig)?;
    }
    Ok(())
}

pub(crate) fn configure(
    command: &mut Command,
    arguments: &[std::ffi::OsString],
    memory: Option<&CodexProjectMemory>,
    gateway: &ResponsesGatewayHandle,
) -> Result<(), CodexKernelError> {
    command.arg("--strict-config");
    // Native restricted-token backend; not full read/network isolation.
    #[cfg(windows)]
    command.args(["-c", "windows.sandbox=\"unelevated\""]);
    command.args(arguments);
    if let Some(memory) = memory {
        memory.configure_command(command)?;
    }
    gateway.configure_codex_command(command);
    Ok(())
}

pub(crate) fn feature_configuration() -> Value {
    let keys = [
        "features.code_mode",
        "features.code_mode_host",
        "features.code_mode_interrupt",
        "features.memories",
        "memories.generate_memories",
        "memories.use_memories",
        "memories.dedicated_tools",
        "memories.disable_on_external_context",
    ];
    json!({
        "edits": keys.map(|key| json!({"keyPath": key, "value": true, "mergeStrategy": "replace"})),
        "filePath": null, "expectedVersion": null, "reloadUserConfig": true
    })
}
