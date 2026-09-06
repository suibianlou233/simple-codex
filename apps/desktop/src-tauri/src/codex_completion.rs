//! Live completion stays pending until passive native task-tree observations agree.
//! The event consumer never awaits this check; approvals and Stop remain usable.
use super::*;
use local_agent_model::CodexTaskTreeWatch;

#[derive(Clone)]
pub(super) struct PendingCompletion {
    pub(super) check_failed: bool,
    instance_id: String,
    binding: CodexTurnBinding,
    terminal: ProjectedTurnTerminal,
}

enum Dispatch {
    Cold,
    Ignore,
    Watch(Box<PendingCompletion>),
}

enum CompletionUpdate {
    Waiting { changed: bool },
    Sealed(ProjectedTurnTerminal),
    Obsolete,
}

impl DesktopRuntime {
    fn defer_completion(
        &mut self,
        terminal: &ProjectedTurnTerminal,
        instance: &str,
    ) -> Result<Dispatch, DesktopError> {
        let Some(owner) = self.codex_turn_owners.get(&terminal.turn_id) else {
            return if self.project_leases.contains_key(&terminal.turn_id) {
                Err(DesktopError::CodexKernelUnavailable)
            } else {
                Ok(Dispatch::Cold)
            };
        };
        if owner.instance_id != instance
            || self.pending_codex_finishes.contains_key(&terminal.turn_id)
        {
            return Ok(Dispatch::Ignore);
        }
        if !self.project_leases.contains_key(&terminal.turn_id) {
            return Ok(Dispatch::Cold);
        }
        let binding = self
            .codex_turn_links
            .values()
            .find(|binding| {
                binding.turn_id == terminal.turn_id && binding.task_id == terminal.task_id
            })
            .cloned()
            .ok_or(DesktopError::StateUnavailable)?;
        self.storage.append_event(NewEvent {
            event_id: Uuid::new_v4().to_string(), task_id: terminal.task_id.clone(), turn_id: Some(terminal.turn_id.clone()),
            event_type: "codex_root_terminal_pending".into(),
            payload: json!({"native_thread_id":binding.codex_thread_id,"native_turn_id":binding.codex_turn_id,"root_status":terminal.status.stream_kind()}),
            created_at_ms: unix_time_ms()?,
        })?;
        let pending = PendingCompletion {
            check_failed: false,
            instance_id: instance.into(),
            binding,
            terminal: terminal.clone(),
        };
        self.pending_codex_finishes
            .insert(terminal.turn_id.clone(), pending.clone());
        Ok(Dispatch::Watch(Box::new(pending)))
    }

    fn owns_completion(&self, pending: &PendingCompletion) -> bool {
        let turn = &pending.terminal.turn_id;
        self.project_leases.contains_key(turn)
            && self
                .codex_turn_owners
                .get(turn)
                .is_some_and(|owner| owner.instance_id == pending.instance_id)
            && self
                .pending_codex_finishes
                .get(turn)
                .is_some_and(|current| {
                    current.instance_id == pending.instance_id && current.binding == pending.binding
                })
    }

    fn completion_check_state(
        &mut self,
        pending: &PendingCompletion,
        failed: bool,
    ) -> Result<bool, DesktopError> {
        if !self.owns_completion(pending) {
            return Ok(false);
        }
        let turn = &pending.terminal.turn_id;
        if self
            .pending_codex_finishes
            .get(turn)
            .is_some_and(|current| current.check_failed == failed)
        {
            return Ok(false);
        }
        // This is a live safety notice, not a claim that persistence succeeded.
        // Keep it visible even if the event store itself is temporarily broken.
        if let Some(current) = self.pending_codex_finishes.get_mut(turn) {
            current.check_failed = failed;
        }
        if self
            .storage
            .append_event(NewEvent {
                event_id: Uuid::new_v4().to_string(),
                task_id: pending.terminal.task_id.clone(),
                turn_id: Some(turn.clone()),
                event_type: "codex_completion_check_state".into(),
                payload: json!({"check_failed":failed}),
                created_at_ms: unix_time_ms()?,
            })
            .is_err()
        {
            crate::logging::error(
                "codex_completion_notice_persist_failed",
                json!({"turn_id":turn}),
            );
        }
        Ok(true)
    }

    fn advance_completion(
        &mut self,
        pending: &PendingCompletion,
        quiet: Result<bool, ()>,
    ) -> Result<CompletionUpdate, DesktopError> {
        if !self.owns_completion(pending) {
            return Ok(CompletionUpdate::Obsolete);
        }
        let failed = match quiet {
            Ok(true) => match self.seal_completion(pending) {
                Ok(Some(terminal)) => return Ok(CompletionUpdate::Sealed(terminal)),
                Ok(None) => return Ok(CompletionUpdate::Obsolete),
                Err(_) => {
                    // Never turn a failed final snapshot/event commit into a
                    // silent wait or release. Avoid logging project contents.
                    if !self
                        .pending_codex_finishes
                        .get(&pending.terminal.turn_id)
                        .is_some_and(|current| current.check_failed)
                    {
                        crate::logging::error(
                            "codex_completion_finalize_failed",
                            json!({"turn_id":pending.terminal.turn_id}),
                        );
                    }
                    true
                }
            },
            Ok(false) => false,
            Err(()) => true,
        };
        Ok(CompletionUpdate::Waiting {
            changed: self.completion_check_state(pending, failed)?,
        })
    }

    fn seal_completion(
        &mut self,
        pending: &PendingCompletion,
    ) -> Result<Option<ProjectedTurnTerminal>, DesktopError> {
        if !self.owns_completion(pending) {
            return Ok(None);
        }
        let terminal = self
            .pending_codex_finishes
            .get(&pending.terminal.turn_id)
            .ok_or(DesktopError::StateUnavailable)?
            .terminal
            .clone();
        self.finish_codex_projected_turn(&terminal)?;
        self.codex_turn_projectors
            .retain(|_, projector| projector.binding().turn_id != pending.terminal.turn_id);
        Ok(Some(terminal))
    }

    pub(super) fn note_completion_cancel_requested(
        &mut self,
        target: &CodexInterruptTarget,
    ) -> Result<(), DesktopError> {
        let turn = self
            .pending_codex_finishes
            .iter()
            .find(|(_, pending)| {
                pending.instance_id == target.instance_id
                    && pending.binding.codex_thread_id == target.thread_id
                    && pending.binding.codex_turn_id == target.turn_id
            })
            .map(|(turn, _)| turn.clone());
        if let Some(turn) = turn {
            let pending = self
                .pending_codex_finishes
                .get(&turn)
                .cloned()
                .ok_or(DesktopError::StateUnavailable)?;
            if pending.terminal.status == ProjectedTurnStatus::Completed
                && self.owns_completion(&pending)
            {
                self.storage.append_event(NewEvent {
                    event_id: Uuid::new_v4().to_string(),
                    task_id: pending.terminal.task_id,
                    turn_id: Some(turn.clone()),
                    event_type: "codex_completion_cancel_requested".into(),
                    payload: json!({}),
                    created_at_ms: unix_time_ms()?,
                })?;
                if let Some(current) = self.pending_codex_finishes.get_mut(&turn) {
                    current.terminal.status = ProjectedTurnStatus::Cancelled;
                }
            }
        }
        Ok(())
    }
}

pub(super) fn apply_live_effects(
    app: &AppHandle,
    runtime: &Arc<Mutex<DesktopRuntime>>,
    client: &CodexKernelClient,
    effects: Vec<CodexDesktopEffect>,
) -> Result<(), DesktopError> {
    for effect in effects {
        let CodexDesktopEffect::Finish(terminal) = &effect else {
            apply_codex_effects(app, runtime, vec![effect])?;
            continue;
        };
        let dispatch = runtime
            .lock()
            .map_err(|_| DesktopError::StateUnavailable)?
            .defer_completion(terminal, client.instance_id())?;
        match dispatch {
            Dispatch::Cold => apply_codex_effects(app, runtime, vec![effect])?,
            Dispatch::Ignore => {}
            Dispatch::Watch(pending) => {
                let pending = *pending;
                emit_completion_pending(app, &pending);
                let app = app.clone();
                let notice = pending.clone();
                tauri::async_runtime::spawn(watch_completion(
                    Arc::clone(runtime),
                    client.clone(),
                    pending,
                    move |terminal| {
                        if let Some(terminal) = terminal {
                            emit_turn_item(
                                &app,
                                &notice.terminal.turn_id,
                                &notice.terminal.task_id,
                                None,
                                None,
                                terminal.status.stream_kind(),
                                None,
                                terminal.error_message,
                                None,
                            );
                        } else {
                            emit_completion_pending(&app, &notice);
                        }
                    },
                ));
            }
        }
    }
    Ok(())
}

fn emit_completion_pending(app: &AppHandle, pending: &PendingCompletion) {
    emit_turn_item(
        app,
        &pending.terminal.turn_id,
        &pending.terminal.task_id,
        None,
        None,
        "completion_pending",
        None,
        None,
        None,
    );
}

async fn watch_completion(
    runtime: Arc<Mutex<DesktopRuntime>>,
    client: CodexKernelClient,
    pending: PendingCompletion,
    notify: impl Fn(Option<ProjectedTurnTerminal>),
) {
    let Ok(mut watch) = CodexTaskTreeWatch::new(&pending.binding.codex_thread_id) else {
        return;
    };
    let seeds = runtime
        .lock()
        .map_err(|_| DesktopError::StateUnavailable)
        .and_then(|runtime| {
            runtime.spawned_children(&pending.terminal.task_id, &pending.terminal.turn_id)
        });
    match seeds {
        Ok(ids) => watch.include_spawned(ids),
        Err(_) => {
            if let Ok(mut runtime) = runtime.lock() {
                let _ = runtime.completion_check_state(&pending, true);
            }
            notify(None);
            return;
        }
    }
    loop {
        if !runtime
            .lock()
            .is_ok_and(|runtime| runtime.owns_completion(&pending))
        {
            return;
        }
        // Observation timeout is uncertainty, never permission to release files.
        let observation = tokio::time::timeout(Duration::from_secs(10), async {
            if !watch.is_quiet(&client).await? {
                return Ok::<_, CodexKernelError>((false, None));
            }
            let report = watch.child_outcomes(&client).await;
            let quiet = watch.is_quiet(&client).await?;
            Ok((quiet, quiet.then_some(report)))
        })
        .await;
        let quiet = match observation {
            Ok(Ok((true, Some(report)))) => {
                runtime.lock().map_err(|_| ()).and_then(|mut runtime| {
                    if !runtime.owns_completion(&pending) {
                        return Err(());
                    }
                    runtime
                        .record_child_report(
                            &pending.terminal.task_id,
                            &pending.terminal.turn_id,
                            report,
                        )
                        .map(|_| true)
                        .map_err(|_| ())
                })
            }
            Ok(Ok((false, _))) => Ok(false),
            _ => Err(()),
        };
        let update = runtime
            .lock()
            .map_err(|_| DesktopError::StateUnavailable)
            .and_then(|mut runtime| runtime.advance_completion(&pending, quiet));
        match update {
            Ok(CompletionUpdate::Waiting { changed: true }) => notify(None),
            Ok(CompletionUpdate::Waiting { changed: false }) => {}
            Ok(CompletionUpdate::Obsolete) => return,
            Ok(CompletionUpdate::Sealed(terminal)) => {
                notify(Some(terminal));
                return;
            }
            Err(_) => crate::logging::error(
                "codex_completion_update_failed",
                json!({"turn_id":pending.terminal.turn_id}),
            ),
        }
        // Closed transport does not prove external descendants exited. Retain
        // the pending turn/lease and uncertainty notice until explicit recovery.
        if client.control_is_closed() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
#[path = "codex_completion_tests.rs"]
mod tests;
