use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::CommandRequest;
use crate::{
    ActionIntent, Capability, CapabilitySet, EditMatch, Evidence, TextEdit, ToolFailure,
    ToolHandler, ToolInvocation, ToolOutcome, ToolRegistry, ToolRegistryError, ToolSpec, Workspace,
    validate_command_request,
};

const DEFAULT_LIST_LIMIT: usize = 200;
const DEFAULT_SEARCH_LIMIT: usize = 100;
const MAX_LIST_LIMIT: usize = 2_000;
const MAX_SEARCH_LIMIT: usize = 500;
const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 60_000;
const MIN_COMMAND_TIMEOUT_MS: u64 = 1_000;
const MAX_COMMAND_TIMEOUT_MS: u64 = 120_000;
const MAX_EDITS: usize = 128;

/// Product-owned wording appended to action tool descriptions.
///
/// This changes only model-visible guidance. It never changes grants or the
/// real sandbox boundary, which must be enforced by the action gate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodingToolGuidance {
    pub action_behavior: String,
    pub command_scope: String,
}

/// Builds the standard coding-tool registry used by the desktop runtime.
pub fn coding_tool_registry(
    workspace: Arc<Workspace>,
    guidance: &CodingToolGuidance,
) -> Result<ToolRegistry, ToolRegistryError> {
    let mut registry = ToolRegistry::new();
    for handler in coding_tool_handlers(workspace, guidance) {
        registry.register_shared(handler)?;
    }
    Ok(registry)
}

/// Returns handlers separately for hosts that combine built-ins and plugins.
#[must_use]
pub fn coding_tool_handlers(
    workspace: Arc<Workspace>,
    guidance: &CodingToolGuidance,
) -> Vec<Arc<dyn ToolHandler>> {
    let read = CapabilitySet::from([Capability::ReadWorkspace]);
    let mut write = CapabilitySet::from([Capability::WriteWorkspace]);
    let mut process = CapabilitySet::from([Capability::SpawnProcess]);
    if workspace.allows_outside_workspace() {
        write.insert(Capability::WriteOutsideWorkspace);
        process.insert(Capability::WriteOutsideWorkspace);
    }
    vec![
        Arc::new(CodingTool::new(
            CodingToolKind::ListFiles,
            workspace.clone(),
            ToolSpec::read_only(
                "list_files",
                "List project files. Read-only and automatically allowed.",
                json!({
                    "type": "object",
                    "properties": {
                        "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIST_LIMIT}
                    },
                    "additionalProperties": false
                }),
                read.clone(),
                true,
            ),
        )),
        Arc::new(CodingTool::new(
            CodingToolKind::ReadFile,
            workspace.clone(),
            ToolSpec::read_only(
                "read_file",
                "Read one UTF-8 project file and return its SHA-256. Read-only and automatically allowed.",
                json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                    "additionalProperties": false
                }),
                read.clone(),
                true,
            ),
        )),
        Arc::new(CodingTool::new(
            CodingToolKind::SearchText,
            workspace.clone(),
            ToolSpec::read_only(
                "search_text",
                "Search literal text in project files. Read-only and automatically allowed.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "limit": {"type": "integer", "minimum": 1, "maximum": MAX_SEARCH_LIMIT}
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
                read,
                true,
            ),
        )),
        Arc::new(CodingTool::new(
            CodingToolKind::WriteFile,
            workspace.clone(),
            ToolSpec::action(
                "write_file",
                action_description(
                    "Propose creating or replacing a complete text file. Existing files require the SHA-256 returned by read_file.",
                    &guidance.action_behavior,
                ),
                json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"},
                        "expected_sha256": {"type": ["string", "null"]}
                    },
                    "required": ["path", "content", "expected_sha256"],
                    "additionalProperties": false
                }),
                write.clone(),
            ),
        )),
        Arc::new(CodingTool::new(
            CodingToolKind::ContinueWriteFile,
            workspace.clone(),
            ToolSpec::action(
                "continue_write_file",
                action_description(
                    "Continue a preserved write_file prefix with only its exact missing suffix. Set complete=true only when the file is finished.",
                    &guidance.action_behavior,
                ),
                json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"},
                        "complete": {"type": "boolean"},
                        "expected_sha256": {"type": ["string", "null"]}
                    },
                    "required": ["path", "content", "complete", "expected_sha256"],
                    "additionalProperties": false
                }),
                write.clone(),
            ),
        )),
        Arc::new(CodingTool::new(
            CodingToolKind::ApplyEdits,
            workspace.clone(),
            ToolSpec::action(
                "apply_edits",
                action_description(
                    "Propose precise text replacements in an existing file. All edits use the same hash-checked snapshot.",
                    &guidance.action_behavior,
                ),
                json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "expected_sha256": {"type": "string"},
                        "edits": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": MAX_EDITS,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "old_text": {"type": "string", "minLength": 1},
                                    "new_text": {"type": "string"},
                                    "occurrence": {"type": "integer", "minimum": 1}
                                },
                                "required": ["old_text", "new_text"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["path", "expected_sha256", "edits"],
                    "additionalProperties": false
                }),
                write,
            ),
        )),
        Arc::new(CodingTool::new(
            CodingToolKind::RunCommand,
            workspace,
            ToolSpec::action(
                "run_command",
                command_description(guidance),
                json!({
                    "type": "object",
                    "properties": {
                        "program": {"type": "string"},
                        "args": {"type": "array", "items": {"type": "string"}},
                        "cwd": {"type": "string"},
                        "timeout_ms": {
                            "type": "integer",
                            "minimum": MIN_COMMAND_TIMEOUT_MS,
                            "maximum": MAX_COMMAND_TIMEOUT_MS
                        }
                    },
                    "required": ["program"],
                    "additionalProperties": false
                }),
                process,
            ),
        )),
    ]
}

fn action_description(base: &str, guidance: &str) -> String {
    if guidance.trim().is_empty() {
        base.to_owned()
    } else {
        format!("{base} {}", guidance.trim())
    }
}

fn command_description(guidance: &CodingToolGuidance) -> String {
    let mut description = String::from(
        "Run an explicit program and argument array. No shell is added implicitly and arguments are never composed into a shell string.",
    );
    if !guidance.command_scope.trim().is_empty() {
        description.push(' ');
        description.push_str(guidance.command_scope.trim());
    }
    if !guidance.action_behavior.trim().is_empty() {
        description.push(' ');
        description.push_str(guidance.action_behavior.trim());
    }
    description
}

#[derive(Debug, Clone, Copy)]
enum CodingToolKind {
    ListFiles,
    ReadFile,
    SearchText,
    WriteFile,
    ContinueWriteFile,
    ApplyEdits,
    RunCommand,
}

struct CodingTool {
    kind: CodingToolKind,
    workspace: Arc<Workspace>,
    spec: ToolSpec,
}

impl CodingTool {
    fn new(kind: CodingToolKind, workspace: Arc<Workspace>, spec: ToolSpec) -> Self {
        Self {
            kind,
            workspace,
            spec,
        }
    }

    fn action_intent(&self, invocation: ToolInvocation, input: Value) -> ToolOutcome {
        ToolOutcome::ActionIntent(ActionIntent::new(
            invocation.call_id,
            self.spec.name(),
            input,
            self.spec.required_capabilities().clone(),
            invocation.idempotency_key,
        ))
    }
}

#[async_trait]
impl ToolHandler for CodingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    async fn invoke(&self, invocation: ToolInvocation) -> Result<ToolOutcome, ToolFailure> {
        if invocation.cancellation.is_cancelled() {
            return Err(ToolFailure::new("工具调用已取消"));
        }
        match self.kind {
            CodingToolKind::ListFiles => {
                let args: ListFilesArguments = decode(invocation.input.arguments.clone())?;
                let limit = bounded_limit(args.limit, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT)?;
                let files = self
                    .workspace
                    .list_files(limit)
                    .map_err(workspace_failure)?;
                Ok(ToolOutcome::Evidence(Evidence::new(
                    json!({"files": files}),
                )))
            }
            CodingToolKind::ReadFile => {
                let args: ReadFileArguments = decode(invocation.input.arguments.clone())?;
                let file = self
                    .workspace
                    .read_file(&args.path)
                    .map_err(workspace_failure)?;
                let value = serde_json::to_value(file).map_err(serialization_failure)?;
                Ok(ToolOutcome::Evidence(Evidence::new(value)))
            }
            CodingToolKind::SearchText => {
                let args: SearchTextArguments = decode(invocation.input.arguments.clone())?;
                let limit = bounded_limit(args.limit, DEFAULT_SEARCH_LIMIT, MAX_SEARCH_LIMIT)?;
                let matches = self
                    .workspace
                    .search_text(&args.query, limit)
                    .map_err(workspace_failure)?;
                Ok(ToolOutcome::Evidence(Evidence::new(
                    json!({"matches": matches}),
                )))
            }
            CodingToolKind::WriteFile => {
                let args: WriteFileArguments = decode(invocation.input.arguments.clone())?;
                let preview = self
                    .workspace
                    .preview_write(&args.path, args.content, args.expected_sha256.as_deref())
                    .map_err(workspace_failure)?;
                let input = serde_json::to_value(preview).map_err(serialization_failure)?;
                Ok(self.action_intent(invocation, input))
            }
            CodingToolKind::ContinueWriteFile => {
                let args: ContinueWriteFileArguments = decode(invocation.input.arguments.clone())?;
                if args.path.trim().is_empty() {
                    return Err(ToolFailure::new("续写路径不能为空"));
                }
                let input = serde_json::to_value(args).map_err(serialization_failure)?;
                Ok(self.action_intent(invocation, input))
            }
            CodingToolKind::ApplyEdits => {
                let args: ApplyEditsArguments = decode(invocation.input.arguments.clone())?;
                if args.edits.is_empty() || args.edits.len() > MAX_EDITS {
                    return Err(ToolFailure::new("增量修改数量超出允许范围"));
                }
                let edits = args
                    .edits
                    .into_iter()
                    .map(|edit| TextEdit {
                        old_text: edit.old_text,
                        new_text: edit.new_text,
                        match_selection: edit.occurrence.map_or(EditMatch::Unique, |one_based| {
                            EditMatch::Occurrence { one_based }
                        }),
                    })
                    .collect::<Vec<_>>();
                let preview = self
                    .workspace
                    .preview_edits(&args.path, &args.expected_sha256, &edits)
                    .map_err(workspace_failure)?;
                let input = serde_json::to_value(preview).map_err(serialization_failure)?;
                Ok(self.action_intent(invocation, input))
            }
            CodingToolKind::RunCommand => {
                let args: RunCommandArguments = decode(invocation.input.arguments.clone())?;
                let timeout_ms = args.timeout_ms.unwrap_or(DEFAULT_COMMAND_TIMEOUT_MS);
                if !(MIN_COMMAND_TIMEOUT_MS..=MAX_COMMAND_TIMEOUT_MS).contains(&timeout_ms) {
                    return Err(ToolFailure::new("命令超时时间超出允许范围"));
                }
                let request = CommandRequest {
                    program: args.program,
                    args: args.args,
                    cwd: args.cwd,
                    timeout_ms,
                };
                validate_command_request(&request).map_err(command_failure)?;
                self.workspace
                    .resolve_directory(&request.cwd)
                    .map_err(workspace_failure)?;
                let input = serde_json::to_value(request).map_err(serialization_failure)?;
                Ok(self.action_intent(invocation, input))
            }
        }
    }
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, ToolFailure> {
    serde_json::from_value(value)
        .map_err(|error| ToolFailure::new(format!("工具参数无效：{error}")))
}

fn bounded_limit(
    requested: Option<usize>,
    default: usize,
    maximum: usize,
) -> Result<usize, ToolFailure> {
    let limit = requested.unwrap_or(default);
    if limit == 0 || limit > maximum {
        return Err(ToolFailure::new("结果数量超出允许范围"));
    }
    Ok(limit)
}

fn workspace_failure(error: crate::WorkspaceError) -> ToolFailure {
    ToolFailure::new(error.to_string())
}

fn command_failure(error: crate::CommandError) -> ToolFailure {
    ToolFailure::new(error.to_string())
}

fn serialization_failure(_error: serde_json::Error) -> ToolFailure {
    ToolFailure::new("工具结果无法序列化")
}

#[derive(Deserialize)]
struct ListFilesArguments {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ReadFileArguments {
    path: String,
}

#[derive(Deserialize)]
struct SearchTextArguments {
    query: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct WriteFileArguments {
    path: String,
    content: String,
    expected_sha256: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ContinueWriteFileArguments {
    path: String,
    content: String,
    complete: bool,
    expected_sha256: Option<String>,
}

#[derive(Deserialize)]
struct EditArguments {
    old_text: String,
    new_text: String,
    occurrence: Option<usize>,
}

#[derive(Deserialize)]
struct ApplyEditsArguments {
    path: String,
    expected_sha256: String,
    edits: Vec<EditArguments>,
}

#[derive(Deserialize)]
struct RunCommandArguments {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_command_cwd")]
    cwd: String,
    timeout_ms: Option<u64>,
}

fn default_command_cwd() -> String {
    ".".to_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{CodingToolGuidance, DEFAULT_COMMAND_TIMEOUT_MS, coding_tool_registry};
    use crate::{Capability, ToolCall, ToolExecution, ToolOutcome, ToolRouter, Workspace};

    fn router() -> (tempfile::TempDir, ToolRouter) {
        let directory = tempdir().expect("temporary directory");
        fs::write(directory.path().join("demo.txt"), "before\n").expect("fixture");
        let workspace = Arc::new(Workspace::open(directory.path()).expect("workspace"));
        let registry =
            coding_tool_registry(workspace, &CodingToolGuidance::default()).expect("registry");
        (directory, ToolRouter::new(Arc::new(registry)))
    }

    #[test]
    fn standard_registry_preserves_the_user_visible_tool_set() {
        let (_directory, router) = router();
        assert_eq!(
            router
                .registry()
                .specs()
                .map(|spec| spec.name())
                .collect::<Vec<_>>(),
            vec![
                "apply_edits",
                "continue_write_file",
                "list_files",
                "read_file",
                "run_command",
                "search_text",
                "write_file",
            ]
        );
        assert_eq!(
            router
                .registry()
                .spec("run_command")
                .map(|spec| spec.execution()),
            Some(ToolExecution::Action { serial: true })
        );
        assert_eq!(
            router.registry().spec("run_command").map(|spec| spec
                .required_capabilities()
                .contains(Capability::SpawnProcess)),
            Some(true)
        );
    }

    #[test]
    fn system_scope_declares_outside_workspace_write_authority() {
        let directory = tempdir().expect("temporary directory");
        let workspace = Arc::new(Workspace::open_system(directory.path()).expect("workspace"));
        let registry =
            coding_tool_registry(workspace, &CodingToolGuidance::default()).expect("registry");

        for name in [
            "write_file",
            "continue_write_file",
            "apply_edits",
            "run_command",
        ] {
            assert_eq!(
                registry.spec(name).map(|spec| spec
                    .required_capabilities()
                    .contains(Capability::WriteOutsideWorkspace)),
                Some(true),
                "{name} must disclose system-scope write authority"
            );
        }
    }

    #[tokio::test]
    async fn read_handlers_return_evidence() {
        let (_directory, router) = router();
        let outcome = router
            .dispatch(ToolCall::new(
                "call-read",
                "read_file",
                json!({"path": "demo.txt"}),
            ))
            .await;
        let Ok(ToolOutcome::Evidence(evidence)) = outcome else {
            panic!("read_file should return evidence");
        };
        assert_eq!(evidence.value["content"], "before\n");
        assert!(evidence.value["sha256"].is_string());
    }

    #[tokio::test]
    async fn write_handler_prepares_preview_without_changing_the_file() {
        let (directory, router) = router();
        let current = fs::read(directory.path().join("demo.txt")).expect("fixture");
        let hash = crate::hash_bytes(&current);
        let outcome = router
            .dispatch(ToolCall::new(
                "call-write",
                "write_file",
                json!({
                    "path": "demo.txt",
                    "content": "after\n",
                    "expected_sha256": hash
                }),
            ))
            .await;
        let Ok(ToolOutcome::ActionIntent(intent)) = outcome else {
            panic!("write_file should return an action intent");
        };
        assert_eq!(intent.action, "write_file");
        assert_eq!(intent.tool_call_id, "call-write");
        assert_eq!(intent.idempotency_key, "call-write");
        assert_eq!(
            intent.required_capabilities,
            crate::CapabilitySet::from([Capability::WriteWorkspace])
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("demo.txt")).expect("unchanged file"),
            "before\n"
        );
        assert_eq!(intent.input["new_content"], "after\n");
    }

    #[tokio::test]
    async fn command_handler_validates_but_never_spawns() {
        let (_directory, router) = router();
        let outcome = router
            .dispatch(ToolCall::new(
                "call-command",
                "run_command",
                json!({"program": "definitely-not-started", "args": [], "cwd": "."}),
            ))
            .await;
        let Ok(ToolOutcome::ActionIntent(intent)) = outcome else {
            panic!("run_command should return an action intent");
        };
        assert_eq!(intent.input["program"], "definitely-not-started");
        assert_eq!(intent.input["timeout_ms"], DEFAULT_COMMAND_TIMEOUT_MS);
    }

    #[tokio::test]
    async fn edit_and_continuation_handlers_only_prepare_action_intents() {
        let (directory, router) = router();
        let original = fs::read(directory.path().join("demo.txt")).expect("fixture");
        let hash = crate::hash_bytes(&original);

        let edit = router
            .dispatch(ToolCall::new(
                "call-edit",
                "apply_edits",
                json!({
                    "path": "demo.txt",
                    "expected_sha256": hash,
                    "edits": [{"old_text": "before", "new_text": "after"}]
                }),
            ))
            .await;
        let continuation = router
            .dispatch(ToolCall::new(
                "call-continuation",
                "continue_write_file",
                json!({
                    "path": "demo.txt",
                    "content": "missing suffix",
                    "complete": true,
                    "expected_sha256": hash
                }),
            ))
            .await;

        assert!(matches!(edit, Ok(ToolOutcome::ActionIntent(_))));
        assert!(matches!(continuation, Ok(ToolOutcome::ActionIntent(_))));
        assert_eq!(
            fs::read(directory.path().join("demo.txt")).expect("unchanged file"),
            original
        );
    }

    #[tokio::test]
    async fn sensitive_paths_are_rejected_before_an_intent_is_created() {
        let (directory, router) = router();
        fs::write(directory.path().join(".env"), "SECRET=value").expect("fixture");
        let outcome = router
            .dispatch(ToolCall::new(
                "call-sensitive",
                "write_file",
                json!({
                    "path": ".env",
                    "content": "changed",
                    "expected_sha256": null
                }),
            ))
            .await;
        assert!(outcome.is_err());
    }
}
