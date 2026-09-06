//! Durable dispatch boundary: lack of a receipt never proves non-execution.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct PendingSubmission {
    pub(super) task_id: String,
    pub(super) turn_id: String,
    pub(super) thread_id: String,
    client_id: String,
    project_root: PathBuf,
    instance_id: String,
    #[serde(default)]
    pub(super) cancel_requested: bool,
}

pub(super) fn load_pending(
    storage: &Storage,
) -> Result<HashMap<String, PendingSubmission>, DesktopError> {
    let mut pending = HashMap::new();
    for task in storage.list_tasks()? {
        for event in storage.load_events(&task.task_id)? {
            let Some(turn) = event.turn_id.as_deref() else {
                continue;
            };
            match event.event_type.as_str() {
                "codex_submission_dispatching" => {
                    let value: PendingSubmission = serde_json::from_value(event.payload)?;
                    if value.turn_id != turn
                        || value.task_id != task.task_id
                        || value.thread_id.is_empty()
                        || value.client_id.is_empty()
                        || value.instance_id.is_empty()
                        || !value.project_root.is_absolute()
                    {
                        return Err(DesktopError::StateUnavailable);
                    }
                    if pending.insert(turn.to_owned(), value).is_some() {
                        return Err(DesktopError::StateUnavailable);
                    }
                }
                "codex_submission_stop_requested" => {
                    if let Some(value) = pending.get_mut(turn) {
                        value.cancel_requested = true;
                    }
                }
                "codex_submission_registered" | "turn_finished" => {
                    pending.remove(turn);
                }
                _ => {}
            }
        }
    }
    Ok(pending)
}

impl DesktopRuntime {
    pub(super) fn begin_codex_submission(
        &mut self,
        prepared: &PreparedCodexTurn,
        thread: &str,
        instance: &str,
    ) -> Result<PendingSubmission, DesktopError> {
        let turn = prepared.turn_id.to_string();
        if self
            .preparing_codex_turns
            .get(&turn)
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(ModelError::Cancelled.into());
        }
        if self.pending_codex_submissions.contains_key(&turn)
            || thread.is_empty()
            || !self.project_leases.contains_key(&turn)
            || !self
                .codex_turn_owners
                .get(&turn)
                .is_some_and(|owner| owner.instance_id == instance)
        {
            return Err(DesktopError::StateUnavailable);
        }
        let pending = PendingSubmission {
            task_id: prepared.task_id.to_string(),
            turn_id: turn.clone(),
            thread_id: thread.into(),
            client_id: prepared.user_message_id.clone(),
            project_root: fs::canonicalize(&prepared.project_root)?,
            instance_id: instance.into(),
            cancel_requested: false,
        };
        self.storage.append_event(NewEvent {
            event_id: Uuid::new_v4().to_string(),
            task_id: pending.task_id.clone(),
            turn_id: Some(turn.clone()),
            event_type: "codex_submission_dispatching".into(),
            payload: serde_json::to_value(&pending)?,
            created_at_ms: unix_time_ms()?,
        })?;
        self.pending_codex_submissions.insert(turn, pending.clone());
        self.preparing_codex_turns
            .remove(&prepared.turn_id.to_string());
        Ok(pending)
    }

    pub(super) fn owns_submission(&self, pending: &PendingSubmission) -> bool {
        self.pending_codex_submissions
            .get(&pending.turn_id)
            .is_some_and(|current| {
                current.client_id == pending.client_id && current.instance_id == pending.instance_id
            })
            && self.project_leases.contains_key(&pending.turn_id)
            && self
                .codex_turn_owners
                .get(&pending.turn_id)
                .is_some_and(|owner| owner.instance_id == pending.instance_id)
    }

    pub(super) fn complete_submission_handoff(
        &mut self,
        turn: &str,
        stop_confirmed: bool,
    ) -> Result<(), DesktopError> {
        let Some(pending) = self.pending_codex_submissions.get(turn) else {
            return Ok(());
        };
        if pending.cancel_requested && !stop_confirmed {
            return Err(DesktopError::SubmissionPending);
        }
        self.storage.append_event(NewEvent {
            event_id: Uuid::new_v4().to_string(),
            task_id: pending.task_id.clone(),
            turn_id: Some(turn.into()),
            event_type: "codex_submission_registered".into(),
            payload: json!({}),
            created_at_ms: unix_time_ms()?,
        })?;
        self.pending_codex_submissions.remove(turn);
        Ok(())
    }

    pub(super) fn request_submission_stop(&mut self, turn: &str) -> Result<bool, DesktopError> {
        let Some(pending) = self.pending_codex_submissions.get(turn).cloned() else {
            return Ok(false);
        };
        if !self.owns_submission(&pending) {
            return Err(DesktopError::SubmissionRecoveryUnavailable);
        }
        if !pending.cancel_requested {
            self.storage.append_event(NewEvent {
                event_id: Uuid::new_v4().to_string(),
                task_id: pending.task_id,
                turn_id: Some(turn.into()),
                event_type: "codex_submission_stop_requested".into(),
                payload: json!({}),
                created_at_ms: unix_time_ms()?,
            })?;
            if let Some(current) = self.pending_codex_submissions.get_mut(turn) {
                current.cancel_requested = true;
            }
        }
        Ok(true)
    }

    pub(super) fn restore_submission_leases(&mut self) {
        for pending in self.pending_codex_submissions.values() {
            let Some(data) = self.database_path.parent() else {
                continue;
            };
            if let Ok(lease) = project_lease::ProjectLease::acquire(&pending.project_root, data) {
                self.project_leases.insert(pending.turn_id.clone(), lease);
            }
            // A competing process or inaccessible root is not evidence that an
            // old native writer exited. Keep the unresolved local turn either way.
        }
    }

    pub(super) fn link_submission(
        &mut self,
        pending: &PendingSubmission,
        turn: &str,
        recovered: bool,
    ) -> Result<Vec<CodexDesktopEffect>, DesktopError> {
        if !self.owns_submission(pending) || turn.is_empty() {
            return Err(DesktopError::StateUnavailable);
        }
        let binding = CodexTurnBinding {
            task_id: pending.task_id.clone(),
            turn_id: pending.turn_id.clone(),
            codex_thread_id: pending.thread_id.clone(),
            codex_turn_id: turn.into(),
        };
        let prior = self
            .storage
            .load_events(&pending.task_id)?
            .into_iter()
            .find(|event| {
                event.turn_id.as_deref() == Some(&pending.turn_id)
                    && event.event_type == "codex_turn_linked"
            });
        if let Some(prior) = prior {
            if prior.payload["codex_thread_id"] != pending.thread_id
                || prior.payload["codex_turn_id"] != turn
            {
                return Err(DesktopError::StateUnavailable);
            }
        } else {
            self.storage.append_event(NewEvent {event_id:Uuid::new_v4().to_string(),task_id:pending.task_id.clone(),
                turn_id:Some(pending.turn_id.clone()),event_type:"codex_turn_linked".into(),
                payload:json!({"codex_thread_id":pending.thread_id,"codex_turn_id":turn,"submission_recovered":recovered}),created_at_ms:unix_time_ms()?})?;
        }
        self.register_codex_turn(binding)
    }
}

pub(super) async fn recover_once<R: local_agent_model::CodexRpc + Clone>(
    runtime: &Arc<Mutex<DesktopRuntime>>,
    rpc: &R,
    pending: &PendingSubmission,
) -> Result<Option<Vec<CodexDesktopEffect>>, DesktopError> {
    let Some(turn) =
        local_agent_model::find_codex_submission(rpc, &pending.thread_id, &pending.client_id)
            .await?
    else {
        return Ok(None);
    };
    let mut history = CodexSessionBridge::new(rpc.clone())
        .read_and_hydrate(&pending.thread_id)
        .await?;
    history.turns.retain(|entry| entry.id == turn);
    history.items.retain(|entry| entry.turn_id == turn);
    if history.turns.len() != 1 {
        return Err(DesktopError::StateUnavailable);
    }
    let mut runtime = runtime.lock().map_err(|_| DesktopError::StateUnavailable)?;
    if !runtime.owns_submission(pending) {
        return Ok(None);
    }
    let mut effects = runtime.link_submission(pending, &turn, true)?;
    effects.extend(runtime.reconcile_codex_snapshot(&history)?);
    Ok(Some(effects))
}

pub(super) async fn watch_submission(
    app: AppHandle,
    runtime: Arc<Mutex<DesktopRuntime>>,
    client: CodexKernelClient,
    pending: PendingSubmission,
) {
    loop {
        let gate = match runtime.lock() {
            Ok(mut runtime) if runtime.owns_submission(&pending) => {
                runtime.codex_event_gate(client.instance_id())
            }
            _ => return,
        };
        let handoff = gate.lock().await;
        let attempt = tokio::time::timeout(
            Duration::from_secs(10),
            recover_once(&runtime, &client, &pending),
        )
        .await;
        if let Ok(Ok(Some(effects))) = attempt {
            let apply = codex_completion::apply_live_effects(&app, &runtime, &client, effects);
            drop(handoff);
            if apply.is_ok() {
                let stop = runtime
                    .lock()
                    .ok()
                    .and_then(|runtime| {
                        runtime
                            .pending_codex_submissions
                            .get(&pending.turn_id)
                            .map(|value| value.cancel_requested)
                    })
                    .unwrap_or(false);
                let stopped = if stop {
                    let target = runtime
                        .lock()
                        .map_err(|_| DesktopError::StateUnavailable)
                        .and_then(|runtime| runtime.codex_interrupt_target(&pending.turn_id));
                    match target {
                        Ok(Some(target)) => local_agent_model::stop_codex_task_tree(
                            &client,
                            &target.thread_id,
                            &target.turn_id,
                        )
                        .await
                        .is_ok(),
                        _ => false,
                    }
                } else {
                    true
                };
                if stopped
                    && runtime.lock().is_ok_and(|mut runtime| {
                        runtime
                            .complete_submission_handoff(&pending.turn_id, stop && stopped)
                            .is_ok()
                    })
                {
                    emit_pending(&app, &pending);
                    return;
                }
            }
        } else {
            drop(handoff);
        }
        emit_pending(&app, &pending);
        // Never claim a disconnected transport proves external tools stopped.
        if client.control_is_closed() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub(super) fn emit_pending(app: &AppHandle, pending: &PendingSubmission) {
    emit_turn(
        app,
        &pending.turn_id,
        &pending.task_id,
        "submission_pending",
        None,
        None,
        None,
    );
}
