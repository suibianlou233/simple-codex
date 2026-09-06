//! Historical Simple executor. New chats are owned by Codex, never this loop.
//! Kept only for explicitly legacy tasks and their durable approvals/recovery.
use super::{
    ActionPayload, ActionStatus, BackendAction, CodexApprovalActionKind, ContinuationRequest,
    ConversationMessage, ConversationRole, DesktopError, DesktopRuntime, DesktopToolContext,
    ModelContextOverrides, PermissionLevel, PreparedTurn, TaskBackend, ToolDisposition,
    TurnOutcome, action_status_name, adapter_for_profile, apply_tool_deltas,
    coding_tool_definitions, coding_tool_router, complete_tool_calls, elapsed_ms, emit_turn,
    ensure_permission_available, execute_authorized_action, execute_tool_call_with_drafts,
    format_tool_exchange_for_context, load_project_instruction, model_error_kind, open_workspace,
    parse_task_id, parse_turn_id, permission_system_prompt, public_model_error,
    recover_truncated_write_calls, should_execute_tool_calls, tool_result_error,
    tool_result_status, truncate_tool_result, unix_time_ms, validate_model_token_budget,
};
use futures_util::StreamExt;
use local_agent_context::{
    ApprovalKind, BudgetConfig, ContextBudgeter, ContextBuildInput, ContextBuilder, ContextMessage,
    ContextPacket, ContextRole, ContextSource, ContextWindowProfile, FileHashFact, PendingApproval,
    RecentError, RetentionPolicy, TrustLevel,
};
use local_agent_core::{AppCommand, AppEvent, TaskId, TurnEngine, TurnId, TurnStatus};
use local_agent_model::{
    FinishReason, Message as ModelMessage, ModelError, ModelEvent, ModelRequest, ReasoningMode,
    Role, ToolExchange, Usage,
};
use local_agent_storage::NewEvent;
use serde_json::json;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Legacy-only state is separate from native turn owners/projectors and RPC state.
#[derive(Default)]
pub(super) struct LegacyExecutionState {
    pub(super) continuation_sources: HashMap<String, String>,
    pub(super) turn_engines: HashMap<String, TurnEngine>,
    pub(super) pending_continuations: Vec<ContinuationRequest>,
}

impl DesktopRuntime {
    pub(super) fn prepare_continuation(
        &mut self,
        task_id: TaskId,
        reason: &str,
        source_turn_id: &str,
    ) -> Result<PreparedTurn, DesktopError> {
        // Legacy tasks created by the old entry point may have no in-memory tag
        // until reload. An explicitly native owner must never enter this loop.
        if matches!(
            self.task_backends.get(&task_id),
            Some(TaskBackend::Codex { .. })
        ) {
            return Err(DesktopError::InvalidCodexResponse(
                "Codex 任务不能回退旧执行引擎",
            ));
        }
        let state = self.core.snapshot();
        let task = state
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or(local_agent_core::CoreError::TaskNotFound(task_id))?;
        let permission_level = self
            .task_permissions
            .get(&task_id)
            .copied()
            .unwrap_or_default();
        ensure_permission_available(permission_level)?;
        let project = state
            .projects
            .iter()
            .find(|project| project.id == task.project_id)
            .ok_or(DesktopError::StateUnavailable)?;
        let workspace = open_workspace(&project.root, permission_level)?;
        let profile = if let Some(profile_id) = self.turn_profiles.get(source_turn_id) {
            self.storage.get_model_profile(profile_id)?
        } else {
            self.storage
                .list_model_profiles()?
                .into_iter()
                .find(|profile| profile.is_default)
        }
        .ok_or_else(|| DesktopError::ModelProfileNotFound("default".to_owned()))?;
        let profile_id = profile.profile_id.clone();
        let adapter = adapter_for_profile(&profile, &*self.secret_store)?;
        let max_output_tokens = profile
            .max_output_tokens
            .map(u32::try_from)
            .transpose()
            .map_err(|_| DesktopError::InvalidModelProfile)?;
        let context_window_tokens = profile
            .context_window_tokens
            .map(u32::try_from)
            .transpose()
            .map_err(|_| DesktopError::InvalidModelProfile)?;
        let turn_id = parse_turn_id(source_turn_id)?;
        let turn = state
            .turns
            .iter()
            .find(|turn| {
                turn.id == turn_id && turn.task_id == task_id && turn.status == TurnStatus::Running
            })
            .ok_or(local_agent_core::CoreError::TurnNotFound(turn_id))?;
        let now = unix_time_ms()?;
        let model_messages = self.model_messages(
            task_id,
            max_output_tokens,
            context_window_tokens,
            ModelContextOverrides {
                permission_level,
                extra_system: Some(reason),
                goal_override: None,
                pending_message: None,
                project_root: None,
            },
        )?;
        self.storage.append_events(vec![
            NewEvent {
                event_id: Uuid::new_v4().to_string(),
                task_id: task_id.to_string(),
                turn_id: Some(turn.id.to_string()),
                event_type: "continuation_context".to_owned(),
                payload: json!({
                    "content": reason,
                    "source_turn_id": source_turn_id
                }),
                created_at_ms: now,
            },
            NewEvent {
                event_id: Uuid::new_v4().to_string(),
                task_id: task_id.to_string(),
                turn_id: Some(turn.id.to_string()),
                event_type: "turn_model_profile".to_owned(),
                payload: json!({ "profile_id": profile_id.clone() }),
                created_at_ms: now,
            },
        ])?;
        self.turn_profiles.insert(turn.id.to_string(), profile_id);
        self.legacy
            .continuation_sources
            .insert(turn.id.to_string(), source_turn_id.to_owned());
        let engine = self
            .legacy
            .turn_engines
            .get(source_turn_id)
            .cloned()
            .unwrap_or_else(|| {
                TurnEngine::restore(turn.id, turn.phase, turn.budget, Default::default())
            });
        let tool_router = coding_tool_router(&workspace, permission_level)?;
        Ok(PreparedTurn {
            turn_id: turn.id,
            task_id,
            model_profile_id: profile.profile_id.clone(),
            model_name: profile.model.clone(),
            model_dialect: profile.dialect.clone(),
            adapter,
            request: ModelRequest {
                messages: model_messages,
                reasoning: ReasoningMode::Off,
                max_output_tokens,
                tools: coding_tool_definitions(&workspace, permission_level)?,
                tool_history: Vec::new(),
            },
            permission_level,
            engine,
            tool_router,
        })
    }

    pub(super) fn model_messages(
        &self,
        task_id: TaskId,
        max_output_tokens: Option<u32>,
        context_window_tokens: Option<u32>,
        overrides: ModelContextOverrides<'_>,
    ) -> Result<Vec<ModelMessage>, DesktopError> {
        validate_model_token_budget(max_output_tokens, context_window_tokens)?;
        let mut system_messages = vec![ContextMessage {
            id: "local-agent:system".to_owned(),
            role: ContextRole::System,
            content: permission_system_prompt(overrides.permission_level),
            created_at_ms: 0,
        }];
        if let Some(extra_system) = overrides.extra_system {
            system_messages.push(ContextMessage {
                id: format!("local-agent:continuation:{task_id}"),
                role: ContextRole::System,
                content: extra_system.to_owned(),
                created_at_ms: unix_time_ms()?,
            });
        }
        let mut history = self
            .messages
            .get(&task_id)
            .into_iter()
            .flatten()
            .map(|message| ContextMessage {
                id: message.id.clone(),
                role: match message.role {
                    ConversationRole::User => ContextRole::User,
                    ConversationRole::Assistant => ContextRole::Assistant,
                },
                content: message.content.clone(),
                created_at_ms: message.created_at_ms,
            })
            .collect::<Vec<_>>();
        if let Some(message) = overrides.pending_message {
            history.push(ContextMessage {
                id: message.id.clone(),
                role: match message.role {
                    ConversationRole::User => ContextRole::User,
                    ConversationRole::Assistant => ContextRole::Assistant,
                },
                content: message.content.clone(),
                created_at_ms: message.created_at_ms,
            });
        }
        let task_id_string = task_id.to_string();
        let mut task_actions = self
            .actions
            .values()
            .filter(|action| action.task_id == task_id_string)
            .collect::<Vec<_>>();
        task_actions.sort_by(|left, right| {
            (left.created_at_ms, left.id.as_str()).cmp(&(right.created_at_ms, right.id.as_str()))
        });
        history.extend(task_actions.iter().map(|action| ContextMessage {
            id: format!("local-agent:action:{}", action.id),
            role: ContextRole::Tool,
            content: truncate_tool_result(&format!(
                "Local tool action {} is {}. {}",
                BackendAction::from(*action).title,
                action_status_name(action.status),
                action.result.as_deref().unwrap_or("It has not been executed.")
            )),
            created_at_ms: action.created_at_ms,
        }));
        if let Some(exchanges) = self.tool_exchanges.get(&task_id) {
            history.extend(exchanges.iter().map(|stored| ContextMessage {
                id: format!("local-agent:tool-exchange:{}", stored.id),
                role: ContextRole::Tool,
                content: truncate_tool_result(&format_tool_exchange_for_context(&stored.exchange)),
                created_at_ms: stored.created_at_ms,
            }));
        }
        history.sort_by(|left, right| {
            (left.created_at_ms, left.id.as_str()).cmp(&(right.created_at_ms, right.id.as_str()))
        });

        let unresolved = task_actions
            .iter()
            .filter(|action| matches!(action.status, ActionStatus::Pending | ActionStatus::Running))
            .collect::<Vec<_>>();
        let pending_approvals = unresolved
            .iter()
            .map(|action| PendingApproval {
                approval_id: action.id.clone(),
                kind: match action.payload {
                    ActionPayload::WriteFile { .. } => ApprovalKind::FileChange,
                    ActionPayload::RunCommand { .. } => ApprovalKind::Command,
                    ActionPayload::CodexApproval { ref request } => match request.kind {
                        CodexApprovalActionKind::CommandExecution => ApprovalKind::Command,
                        CodexApprovalActionKind::FileChange => ApprovalKind::FileChange,
                    },
                },
                summary: BackendAction::from(**action).title,
            })
            .collect();
        let mut latest_hashes = std::collections::BTreeMap::new();
        for action in &task_actions {
            if let ActionPayload::WriteFile { preview } = &action.payload {
                let hash = match action.status {
                    ActionStatus::Pending | ActionStatus::Running | ActionStatus::Rejected => {
                        preview.original_sha256.as_ref()
                    }
                    ActionStatus::Applied => Some(&preview.new_sha256),
                    ActionStatus::Undone => preview.original_sha256.as_ref(),
                    ActionStatus::Failed => None,
                };
                if let Some(sha256) = hash {
                    latest_hashes.insert(
                        preview.path.clone(),
                        FileHashFact {
                            path: preview.path.clone(),
                            sha256: sha256.clone(),
                        },
                    );
                }
            }
        }
        let file_hashes = latest_hashes.into_values().collect();
        let recent_error = task_actions
            .iter()
            .rev()
            .find(|action| action.status == ActionStatus::Failed)
            .and_then(|action| {
                action.result.as_ref().map(|message| RecentError {
                    source: BackendAction::from(*action).title,
                    message: message.clone(),
                })
            });
        let user_goal = overrides
            .goal_override
            .map(str::to_owned)
            .or_else(|| self.task_goals.get(&task_id).cloned())
            .unwrap_or_else(|| "Continue the local coding task safely.".to_owned());
        let mut rules = Vec::new();
        let mut tool_evidence = Vec::new();
        for message in system_messages {
            if message.id == "local-agent:system" {
                rules.push(ContextPacket::new(
                    message.id,
                    ContextSource::SystemInstruction,
                    TrustLevel::TrustedSystem,
                    RetentionPolicy::Pinned,
                    message.content,
                    message.created_at_ms,
                ));
            } else {
                tool_evidence.push(ContextPacket::new(
                    message.id,
                    ContextSource::ToolEvidence,
                    TrustLevel::UntrustedToolOutput,
                    RetentionPolicy::Pinned,
                    message.content,
                    message.created_at_ms,
                ));
            }
        }
        let project_root = overrides
            .project_root
            .map(Path::to_path_buf)
            .map(Ok)
            .unwrap_or_else(|| self.task_project_root(task_id))?;
        if let Some(instruction) = load_project_instruction(&project_root)?
            && !instruction.content.trim().is_empty()
        {
            rules.push(ContextPacket::new(
                format!(
                    "local-agent:project-instruction:{}",
                    instruction.file_name.to_ascii_lowercase()
                ),
                ContextSource::ProjectInstruction,
                TrustLevel::UntrustedWorkspace,
                RetentionPolicy::Pinned,
                format!(
                    "Project instructions from `{}`:\n\n{}",
                    instruction.file_name, instruction.content
                ),
                0,
            ));
        }
        let mut history_packets = Vec::new();
        for message in history {
            let (source, trust, retention) = match message.role {
                ContextRole::User => (
                    ContextSource::UserMessage,
                    TrustLevel::UserProvided,
                    RetentionPolicy::Recent,
                ),
                ContextRole::Assistant => (
                    ContextSource::AssistantMessage,
                    TrustLevel::ModelGenerated,
                    RetentionPolicy::Recent,
                ),
                ContextRole::Tool => (
                    ContextSource::ToolEvidence,
                    TrustLevel::UntrustedToolOutput,
                    RetentionPolicy::Compressible,
                ),
                ContextRole::System => (
                    ContextSource::WorkspaceContent,
                    TrustLevel::UntrustedWorkspace,
                    RetentionPolicy::Compressible,
                ),
            };
            let packet = ContextPacket::new(
                message.id,
                source,
                trust,
                retention,
                message.content,
                message.created_at_ms,
            );
            if source == ContextSource::ToolEvidence {
                tool_evidence.push(packet);
            } else {
                history_packets.push(packet);
            }
        }
        let max_tokens = context_window_tokens.map_or(32_768, |value| value as usize);
        let maximum_reserve = max_tokens.saturating_sub(1_024).max(1);
        let output_reserve = max_output_tokens
            .map_or(4_000, |value| value as usize)
            .clamp(1_024.min(maximum_reserve), maximum_reserve);
        let budget_config = BudgetConfig::from_profile(
            ContextWindowProfile::new(max_tokens, output_reserve).with_chars_per_token(1),
        );
        let built = ContextBuilder.build_packets(&ContextBuildInput {
            user_goal: ContextPacket::new(
                format!("local-agent:goal:{task_id}"),
                ContextSource::UserGoal,
                TrustLevel::UserProvided,
                RetentionPolicy::Pinned,
                user_goal,
                0,
            ),
            rules,
            saved_notes: Vec::new(),
            history: history_packets,
            tool_evidence,
            pending_approvals,
            recent_error,
            file_hashes,
        })?;
        let budgeted = ContextBudgeter::new(budget_config)?.build_packets(&built)?;
        Ok(budgeted
            .messages
            .into_iter()
            .map(|message| ModelMessage {
                role: match message.role {
                    ContextRole::System => Role::System,
                    ContextRole::Assistant => Role::Assistant,
                    _ => Role::User,
                },
                content: message.content,
            })
            .collect())
    }

    pub(super) fn action_group_resolved(
        &self,
        action_id: &str,
    ) -> Result<Option<(TaskId, String)>, DesktopError> {
        let action = self
            .actions
            .get(action_id)
            .ok_or_else(|| DesktopError::ActionNotFound(action_id.to_owned()))?;
        let unresolved = self.actions.values().any(|candidate| {
            candidate.task_id == action.task_id
                && candidate.turn_id == action.turn_id
                && matches!(
                    candidate.status,
                    ActionStatus::Pending | ActionStatus::Running
                )
        });
        if unresolved {
            Ok(None)
        } else {
            Ok(Some((
                parse_task_id(&action.task_id)?,
                action.turn_id.clone(),
            )))
        }
    }

    pub(super) fn checkpoint_turn_engine(
        &mut self,
        task_id: TaskId,
        engine: &TurnEngine,
    ) -> Result<(), DesktopError> {
        let turn_id = engine.turn_id();
        let current_phase = self
            .core
            .snapshot()
            .turns
            .into_iter()
            .find(|turn| turn.id == turn_id)
            .ok_or(local_agent_core::CoreError::TurnNotFound(turn_id))?
            .phase;
        let transition = if current_phase == engine.phase() {
            None
        } else {
            Some(self.core.decide(AppCommand::TransitionTurn {
                turn_id,
                phase: engine.phase(),
            })?)
        };
        let now = unix_time_ms()?;
        let mut events = Vec::new();
        if let Some(event) = transition.as_ref() {
            events.push(NewEvent {
                event_id: Uuid::new_v4().to_string(),
                task_id: task_id.to_string(),
                turn_id: Some(turn_id.to_string()),
                event_type: "turn_phase_changed".to_owned(),
                payload: serde_json::to_value(event)?,
                created_at_ms: now,
            });
        }
        events.push(NewEvent {
            event_id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            turn_id: Some(turn_id.to_string()),
            event_type: "turn_kernel_checkpoint".to_owned(),
            payload: serde_json::to_value(engine)?,
            created_at_ms: now,
        });
        self.storage.append_events(events)?;
        if let Some(event) = transition.as_ref() {
            self.core.apply(event)?;
        }
        self.legacy
            .turn_engines
            .insert(turn_id.to_string(), engine.clone());
        Ok(())
    }

    pub(super) fn finish_turn(
        &mut self,
        turn_id: TurnId,
        outcome: TurnOutcome<'_>,
    ) -> Result<(), DesktopError> {
        let finish_event = self.core.decide(AppCommand::FinishTurn {
            turn_id,
            status: outcome.status,
        })?;
        let AppEvent::TurnFinished { turn } = &finish_event else {
            return Err(DesktopError::StateUnavailable);
        };
        let now = turn
            .finished_at_ms
            .unwrap_or_else(|| unix_time_ms().unwrap_or_default());
        let mut events = Vec::new();
        let assistant_event_id = Uuid::new_v4().to_string();
        if !outcome.assistant_text.is_empty() {
            events.push(NewEvent {
                event_id: assistant_event_id.clone(),
                task_id: turn.task_id.to_string(),
                turn_id: Some(turn_id.to_string()),
                event_type: "assistant_message".to_owned(),
                payload: json!({ "content": outcome.assistant_text }),
                created_at_ms: now,
            });
        }
        events.push(NewEvent {
            event_id: Uuid::new_v4().to_string(),
            task_id: turn.task_id.to_string(),
            turn_id: Some(turn_id.to_string()),
            event_type: "turn_metrics".to_owned(),
            payload: json!({
                "elapsed_ms": outcome.elapsed_ms,
                "first_token_ms": outcome.first_token_ms,
                "input_tokens": outcome.usage.input_tokens,
                "output_tokens": outcome.usage.output_tokens,
                "reasoning_tokens": outcome.usage.reasoning_tokens
            }),
            created_at_ms: now,
        });
        if let Some(error) = outcome.error {
            events.push(NewEvent {
                event_id: Uuid::new_v4().to_string(),
                task_id: turn.task_id.to_string(),
                turn_id: Some(turn_id.to_string()),
                event_type: "turn_error".to_owned(),
                payload: json!({ "message": error }),
                created_at_ms: now,
            });
        }
        events.push(NewEvent {
            event_id: Uuid::new_v4().to_string(),
            task_id: turn.task_id.to_string(),
            turn_id: Some(turn_id.to_string()),
            event_type: "turn_finished".to_owned(),
            payload: serde_json::to_value(&finish_event)?,
            created_at_ms: now,
        });
        self.storage.append_events(events)?;
        self.core.apply(&finish_event)?;
        if !outcome.assistant_text.is_empty() {
            self.messages
                .entry(turn.task_id)
                .or_default()
                .push(ConversationMessage {
                    id: assistant_event_id,
                    phase: None,
                    task_id: turn.task_id,
                    turn_id: Some(turn_id.to_string()),
                    role: ConversationRole::Assistant,
                    content: outcome.assistant_text.to_owned(),
                    created_at_ms: now,
                });
        }
        Ok(())
    }
}

pub(super) async fn execute_turn(
    app: AppHandle,
    runtime: Arc<Mutex<DesktopRuntime>>,
    active_turns: Arc<Mutex<HashMap<String, CancellationToken>>>,
    prepared: PreparedTurn,
    cancellation: CancellationToken,
) {
    let turn_id = prepared.turn_id.to_string();
    let task_id = prepared.task_id.to_string();
    crate::logging::info(
        "turn_started",
        json!({
            "task_id": task_id,
            "turn_id": turn_id,
            "model_profile_id": prepared.model_profile_id,
            "model": prepared.model_name,
            "dialect": prepared.model_dialect,
            "reasoning": "off",
            "message_count": prepared.request.messages.len(),
            "tool_exchange_count": prepared.request.tool_history.len(),
            "max_output_tokens": prepared.request.max_output_tokens
        }),
    );
    emit_turn(&app, &turn_id, &task_id, "started", None, None, None);
    let started = Instant::now();
    let mut first_token_ms = None;
    let mut text = String::new();
    let mut usage = Usage::default();
    let mut request = prepared.request;
    let mut engine = prepared.engine;
    let prior_elapsed_ms = engine.usage().elapsed_ms;
    let mut terminal_status = TurnStatus::Completed;
    let mut terminal_kind = "finished";
    let mut terminal_message = None;
    let mut finished = false;
    let mut truncated_tool_recoveries = 0_usize;
    let mut pending_write_drafts = HashMap::new();
    loop {
        let kernel_elapsed = prior_elapsed_ms.saturating_add(elapsed_ms(started));
        if let Err(error) =
            engine.begin_sampling(unix_time_ms().unwrap_or_default(), kernel_elapsed)
        {
            terminal_status = TurnStatus::Failed;
            terminal_kind = "failed";
            terminal_message = Some(error.to_string());
            finished = true;
            break;
        }
        let checkpoint = runtime
            .lock()
            .map_err(|_| DesktopError::StateUnavailable)
            .and_then(|mut runtime| runtime.checkpoint_turn_engine(prepared.task_id, &engine));
        if let Err(error) = checkpoint {
            terminal_status = TurnStatus::Failed;
            terminal_kind = "failed";
            terminal_message = Some(error.to_string());
            finished = true;
            break;
        }
        let round = engine.usage().model_rounds;
        let round_started = Instant::now();
        let mut round_text = String::new();
        let mut round_usage = Usage::default();
        crate::logging::info(
            "model_request_started",
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
                "round": round,
                "message_count": request.messages.len(),
                "tool_exchange_count": request.tool_history.len()
            }),
        );
        let mut stream = match prepared
            .adapter
            .stream(request.clone(), cancellation.clone())
            .await
        {
            Ok(stream) => {
                crate::logging::info(
                    "model_stream_connected",
                    json!({
                        "task_id": task_id,
                        "turn_id": turn_id,
                        "round": round,
                        "elapsed_ms": elapsed_ms(round_started)
                    }),
                );
                stream
            }
            Err(ModelError::Cancelled) => {
                crate::logging::warn(
                    "model_request_cancelled",
                    json!({ "task_id": task_id, "turn_id": turn_id, "round": round }),
                );
                terminal_status = TurnStatus::Cancelled;
                terminal_kind = "cancelled";
                finished = true;
                break;
            }
            Err(error) => {
                crate::logging::error(
                    "model_request_failed",
                    json!({
                        "task_id": task_id,
                        "turn_id": turn_id,
                        "round": round,
                        "error_kind": model_error_kind(&error),
                        "error": error.to_string(),
                        "elapsed_ms": elapsed_ms(round_started)
                    }),
                );
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some(public_model_error(&error));
                finished = true;
                break;
            }
        };
        let mut completion = None;
        let mut partial_calls = Vec::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(ModelEvent::TextDelta(delta)) => {
                    first_token_ms.get_or_insert_with(|| elapsed_ms(started));
                    round_text.push_str(&delta);
                    emit_turn(&app, &turn_id, &task_id, "delta", Some(delta), None, None);
                }
                Ok(ModelEvent::ToolCallDelta(deltas)) => {
                    apply_tool_deltas(&mut partial_calls, deltas);
                }
                Ok(ModelEvent::Usage(value)) => {
                    round_usage.input_tokens =
                        round_usage.input_tokens.saturating_add(value.input_tokens);
                    round_usage.output_tokens = round_usage
                        .output_tokens
                        .saturating_add(value.output_tokens);
                    round_usage.reasoning_tokens = round_usage
                        .reasoning_tokens
                        .saturating_add(value.reasoning_tokens);
                    usage.input_tokens = usage.input_tokens.saturating_add(value.input_tokens);
                    usage.output_tokens = usage.output_tokens.saturating_add(value.output_tokens);
                    usage.reasoning_tokens = usage
                        .reasoning_tokens
                        .saturating_add(value.reasoning_tokens);
                    emit_turn(&app, &turn_id, &task_id, "usage", None, None, Some(usage));
                }
                Ok(ModelEvent::Completed(reason)) => {
                    completion = Some(reason);
                    break;
                }
                Err(ModelError::Cancelled) => {
                    crate::logging::warn(
                        "model_stream_cancelled",
                        json!({ "task_id": task_id, "turn_id": turn_id, "round": round }),
                    );
                    terminal_status = TurnStatus::Cancelled;
                    terminal_kind = "cancelled";
                    finished = true;
                    break;
                }
                Err(error) => {
                    crate::logging::error(
                        "model_stream_failed",
                        json!({
                            "task_id": task_id,
                            "turn_id": turn_id,
                            "round": round,
                            "error_kind": model_error_kind(&error),
                            "error": error.to_string(),
                            "elapsed_ms": elapsed_ms(round_started)
                        }),
                    );
                    terminal_status = TurnStatus::Failed;
                    terminal_kind = "failed";
                    terminal_message = Some(public_model_error(&error));
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            emit_turn(&app, &turn_id, &task_id, "reset", None, None, None);
            break;
        }
        let Some(completion) = completion else {
            crate::logging::error(
                "model_stream_incomplete",
                json!({ "task_id": task_id, "turn_id": turn_id, "round": round }),
            );
            terminal_status = TurnStatus::Failed;
            terminal_kind = "failed";
            terminal_message = Some("模型流在完成前中断".to_owned());
            emit_turn(&app, &turn_id, &task_id, "reset", None, None, None);
            finished = true;
            break;
        };
        let kernel_elapsed = prior_elapsed_ms.saturating_add(elapsed_ms(started));
        if let Err(error) = engine
            .record_tokens(
                round_usage.input_tokens,
                round_usage.output_tokens,
                kernel_elapsed,
            )
            .and_then(|()| engine.record_output_chars(round_text.chars().count(), kernel_elapsed))
        {
            terminal_status = TurnStatus::Failed;
            terminal_kind = "failed";
            terminal_message = Some(error.to_string());
            finished = true;
            break;
        }
        crate::logging::info(
            "model_round_completed",
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
                "round": round,
                "finish_reason": format!("{completion:?}"),
                "elapsed_ms": elapsed_ms(round_started),
                "first_token_ms": first_token_ms,
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
                "reasoning_tokens": usage.reasoning_tokens,
                "partial_tool_call_count": partial_calls.len()
            }),
        );
        if completion == FinishReason::Length {
            emit_turn(&app, &turn_id, &task_id, "reset", None, None, None);
            if partial_calls.is_empty() {
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some("模型输出达到上限，回复未完成".to_owned());
                finished = true;
                break;
            }
            let recovery =
                match recover_truncated_write_calls(partial_calls, &mut pending_write_drafts) {
                    Ok(recovery) => recovery,
                    Err(error) => {
                        terminal_status = TurnStatus::Failed;
                        terminal_kind = "failed";
                        terminal_message = Some(public_model_error(&error));
                        finished = true;
                        break;
                    }
                };
            truncated_tool_recoveries += 1;
            if let Err(error) =
                engine.record_recovery(prior_elapsed_ms.saturating_add(elapsed_ms(started)))
            {
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some(error.to_string());
                finished = true;
                break;
            }
            let checkpoint = runtime
                .lock()
                .map_err(|_| DesktopError::StateUnavailable)
                .and_then(|mut runtime| runtime.checkpoint_turn_engine(prepared.task_id, &engine));
            if let Err(error) = checkpoint {
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some(error.to_string());
                finished = true;
                break;
            }
            crate::logging::warn(
                "truncated_tool_call_recovery",
                json!({
                    "task_id": task_id,
                    "turn_id": turn_id,
                    "round": round,
                    "recovery_attempt": truncated_tool_recoveries,
                    "tool_call_count": recovery.calls.len()
                }),
            );
            request.tool_history.push(recovery);
            continue;
        }
        if matches!(
            completion,
            FinishReason::ContentFilter | FinishReason::Other
        ) {
            emit_turn(&app, &turn_id, &task_id, "reset", None, None, None);
            terminal_status = TurnStatus::Failed;
            terminal_kind = "failed";
            terminal_message = Some(match completion {
                FinishReason::ContentFilter => "模型回复被内容策略截断".to_owned(),
                _ => "模型以未知原因中断回复".to_owned(),
            });
            finished = true;
            break;
        }
        if completion == FinishReason::Stop && !partial_calls.is_empty() {
            crate::logging::warn(
                "model_finish_reason_corrected",
                json!({
                    "task_id": task_id,
                    "turn_id": turn_id,
                    "round": round,
                    "reported_finish_reason": format!("{completion:?}"),
                    "partial_tool_call_count": partial_calls.len(),
                    "effective_finish_reason": "ToolCalls"
                }),
            );
        }
        if !should_execute_tool_calls(completion, partial_calls.len()) {
            text = round_text;
            finished = true;
            break;
        }
        emit_turn(&app, &turn_id, &task_id, "reset", None, None, None);
        let calls = match complete_tool_calls(partial_calls) {
            Ok(calls) if !calls.is_empty() => calls,
            Ok(_) => {
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some("模型请求了工具，但没有给出工具调用".to_owned());
                finished = true;
                break;
            }
            Err(error) => {
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some(public_model_error(&error));
                finished = true;
                break;
            }
        };
        if let Err(error) = engine.begin_tools(
            calls.len(),
            unix_time_ms().unwrap_or_default(),
            prior_elapsed_ms.saturating_add(elapsed_ms(started)),
        ) {
            terminal_status = TurnStatus::Failed;
            terminal_kind = "failed";
            terminal_message = Some(error.to_string());
            finished = true;
            break;
        }
        let checkpoint = runtime
            .lock()
            .map_err(|_| DesktopError::StateUnavailable)
            .and_then(|mut runtime| runtime.checkpoint_turn_engine(prepared.task_id, &engine));
        if let Err(error) = checkpoint {
            terminal_status = TurnStatus::Failed;
            terminal_kind = "failed";
            terminal_message = Some(error.to_string());
            finished = true;
            break;
        }
        let mut results = Vec::new();
        let mut proposed = Vec::new();
        for call in &calls {
            let tool_started = Instant::now();
            crate::logging::info(
                "tool_call_started",
                json!({
                    "task_id": task_id,
                    "turn_id": turn_id,
                    "round": round,
                    "tool_call_id": call.id,
                    "tool": call.name,
                    "arguments_chars": call.arguments.chars().count()
                }),
            );
            match execute_tool_call_with_drafts(
                DesktopToolContext {
                    router: &prepared.tool_router,
                    task_id: prepared.task_id,
                    turn_id: prepared.turn_id,
                    cancellation: cancellation.child_token(),
                    permission_level: prepared.permission_level,
                },
                call,
                &mut pending_write_drafts,
            )
            .await
            {
                ToolDisposition::Immediate(result) => {
                    crate::logging::info(
                        "tool_call_completed",
                        json!({
                            "task_id": task_id,
                            "turn_id": turn_id,
                            "round": round,
                            "tool_call_id": call.id,
                            "tool": call.name,
                            "disposition": "immediate",
                            "status": tool_result_status(&result.content),
                            "error": tool_result_error(&result.content),
                            "result_chars": result.content.chars().count(),
                            "elapsed_ms": elapsed_ms(tool_started)
                        }),
                    );
                    results.push(result);
                }
                ToolDisposition::Proposed(action) => {
                    crate::logging::info(
                        "tool_call_completed",
                        json!({
                            "task_id": task_id,
                            "turn_id": turn_id,
                            "round": round,
                            "tool_call_id": call.id,
                            "tool": call.name,
                            "action_id": action.id,
                            "disposition": "approval_required",
                            "elapsed_ms": elapsed_ms(tool_started)
                        }),
                    );
                    proposed.push(*action);
                }
            }
        }
        let mut exchange = ToolExchange { calls, results };
        let persist_result = runtime
            .lock()
            .map_err(|_| DesktopError::StateUnavailable)
            .and_then(|mut runtime| {
                runtime.persist_tool_exchange(
                    prepared.task_id,
                    prepared.turn_id,
                    &exchange,
                    proposed.clone(),
                )
            });
        let exchange_id = match persist_result {
            Ok(exchange_id) => exchange_id,
            Err(error) => {
                crate::logging::error(
                    "tool_exchange_persist_failed",
                    json!({
                        "task_id": task_id,
                        "turn_id": turn_id,
                        "round": round,
                        "error": error.to_string()
                    }),
                );
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some(error.to_string());
                finished = true;
                break;
            }
        };
        if !proposed.is_empty() {
            if prepared.permission_level == PermissionLevel::Approval {
                match engine.wait_for_approval() {
                    Ok(()) => {
                        let checkpoint = runtime
                            .lock()
                            .map_err(|_| DesktopError::StateUnavailable)
                            .and_then(|mut runtime| {
                                runtime.checkpoint_turn_engine(prepared.task_id, &engine)
                            });
                        match checkpoint {
                            Ok(()) => {
                                terminal_kind = "approval_required";
                                finished = true;
                                break;
                            }
                            Err(error) => {
                                terminal_status = TurnStatus::Failed;
                                terminal_kind = "failed";
                                terminal_message = Some(error.to_string());
                                finished = true;
                                break;
                            }
                        }
                    }
                    Err(error) => {
                        terminal_status = TurnStatus::Failed;
                        terminal_kind = "failed";
                        terminal_message = Some(error.to_string());
                        finished = true;
                        break;
                    }
                }
            }
            if let Err(error) = engine.begin_actions(
                unix_time_ms().unwrap_or_default(),
                prior_elapsed_ms.saturating_add(elapsed_ms(started)),
            ) {
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some(error.to_string());
                finished = true;
                break;
            }
            let checkpoint = runtime
                .lock()
                .map_err(|_| DesktopError::StateUnavailable)
                .and_then(|mut runtime| runtime.checkpoint_turn_engine(prepared.task_id, &engine));
            if let Err(error) = checkpoint {
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some(error.to_string());
                finished = true;
                break;
            }
            for action in &proposed {
                let result = execute_authorized_action(
                    &runtime,
                    action,
                    prepared.permission_level,
                    cancellation.child_token(),
                )
                .await;
                let _ = app.emit("action-changed", action.id.clone());
                exchange.results.push(result);
            }
            let completion = runtime
                .lock()
                .map_err(|_| DesktopError::StateUnavailable)
                .and_then(|mut runtime| {
                    runtime.complete_tool_exchange(
                        prepared.task_id,
                        prepared.turn_id,
                        &exchange_id,
                        &exchange,
                    )
                });
            if let Err(error) = completion {
                terminal_status = TurnStatus::Failed;
                terminal_kind = "failed";
                terminal_message = Some(error.to_string());
                finished = true;
                break;
            }
        }
        request.tool_history.push(exchange);
    }
    debug_assert!(finished, "turn loop must exit through a terminal decision");
    if terminal_kind == "approval_required" && terminal_message.is_none() {
        emit_turn(
            &app,
            &turn_id,
            &task_id,
            terminal_kind,
            None,
            None,
            Some(usage),
        );
        crate::logging::info(
            "turn_waiting_approval",
            json!({
                "task_id": task_id,
                "turn_id": turn_id,
                "model_rounds": engine.usage().model_rounds,
                "tool_calls": engine.usage().tool_calls,
                "elapsed_ms": engine.usage().elapsed_ms
            }),
        );
        if let Ok(mut turns) = active_turns.lock() {
            turns.remove(&turn_id);
        }
        return;
    }
    let persistence = runtime
        .lock()
        .map_err(|_| DesktopError::StateUnavailable)
        .and_then(|mut runtime| {
            runtime.finish_turn(
                prepared.turn_id,
                TurnOutcome {
                    status: terminal_status,
                    assistant_text: &text,
                    usage,
                    elapsed_ms: elapsed_ms(started),
                    first_token_ms,
                    error: terminal_message.as_deref(),
                },
            )
        });
    let final_message = persistence
        .err()
        .map(|error| error.to_string())
        .or(terminal_message);
    let final_kind = if final_message.is_some() {
        "failed"
    } else {
        terminal_kind
    };
    emit_turn(
        &app,
        &turn_id,
        &task_id,
        final_kind,
        None,
        final_message.clone(),
        Some(usage),
    );
    crate::logging::info(
        "turn_finished",
        json!({
            "task_id": task_id,
            "turn_id": turn_id,
            "status": final_kind,
            "elapsed_ms": elapsed_ms(started),
            "first_token_ms": first_token_ms,
            "input_tokens": usage.input_tokens,
            "output_tokens": usage.output_tokens,
            "reasoning_tokens": usage.reasoning_tokens,
            "assistant_chars": text.chars().count(),
            "error": final_message
        }),
    );
    if let Ok(mut turns) = active_turns.lock() {
        turns.remove(&turn_id);
    }
}
