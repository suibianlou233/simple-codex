//! Accept the durable local turn promptly; start the native kernel asynchronously.
use super::*;
use tauri::Manager;

pub(super) fn queue(
    app: AppHandle,
    state: &DesktopState,
    prepared: PreparedCodexTurn,
) -> Result<(), DesktopError> {
    let turn = prepared.turn_id.to_string();
    state
        .lock()?
        .preparing_codex_turns
        .insert(turn.clone(), CancellationToken::new());
    tauri::async_runtime::spawn(async move {
        let state = app.state::<DesktopState>();
        let task_id = prepared.task_id;
        let turn_id = prepared.turn_id;
        let result = start_codex_prepared_turn(app.clone(), &state, prepared).await;
        if let Err(error) = result {
            // Beyond dispatch, existing recovery owns the outcome. Never label
            // uncertain native execution as a pre-submission failure.
            let (kind, message) = match state.lock() {
                Ok(mut runtime) => {
                    if runtime.pending_codex_submissions.contains_key(&turn) {
                        return;
                    }
                    let cancelled = runtime
                        .preparing_codex_turns
                        .remove(&turn)
                        .is_some_and(|token| token.is_cancelled());
                    // No native turn was dispatched; release local ownership.
                    runtime.project_leases.remove(&turn);
                    let persisted = if cancelled {
                        // No native turn was sent.
                        runtime.finish_codex_projected_turn(&ProjectedTurnTerminal {
                            task_id: task_id.to_string(),
                            turn_id: turn.clone(),
                            status: ProjectedTurnStatus::Cancelled,
                            error_message: None,
                        })
                    } else {
                        runtime.mark_codex_submission_failed(task_id, turn_id, &error.to_string())
                    };
                    if persisted.is_err() {
                        crate::logging::error(
                            "codex_preparation_finalize_failed",
                            json!({"turn_id":turn}),
                        );
                        return;
                    }
                    (
                        if cancelled { "cancelled" } else { "failed" },
                        (!cancelled).then(|| error.to_string()),
                    )
                }
                Err(_) => return,
            };
            crate::logging::warn(
                "codex_preparation_ended",
                json!({"turn_id":turn,"status":kind}),
            );
            emit_turn(&app, &turn, &task_id.to_string(), kind, None, message, None);
        }
    });
    Ok(())
}
