//! Project execution ownership only. No filesystem snapshot or backup store.
use super::*;

impl DesktopRuntime {
    pub(super) fn reserve_project_execution(&mut self, prepared: &PreparedCodexTurn) -> Result<(), DesktopError> {
        let turn = prepared.turn_id.to_string();
        if self.preparing_codex_turns.get(&turn).is_some_and(CancellationToken::is_cancelled) {
            return Err(ModelError::Cancelled.into());
        }
        self.acquire_project_execution(&prepared.task_id.to_string(), &turn, &prepared.project_root)
    }

    pub(super) fn acquire_project_execution(&mut self, task: &str, turn: &str, root: &Path) -> Result<(), DesktopError> {
        if self.project_execution_busy(task, Some(turn))? {
            return Err(DesktopError::ProjectBusy);
        }
        if !self.project_leases.contains_key(turn) {
            let lease = project_lease::ProjectLease::acquire(
                root, self.database_path.parent().ok_or(DesktopError::InvalidStoredPath)?
            )?;
            self.project_leases.insert(turn.to_owned(), lease);
        }
        Ok(())
    }

    pub(super) fn project_execution_busy(
        &self,
        task: &str,
        except_turn: Option<&str>,
    ) -> Result<bool, DesktopError> {
        let parsed = parse_task_id(task)?;
        let snapshot = self.core.snapshot();
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.id == parsed)
            .ok_or(local_agent_core::CoreError::TaskNotFound(parsed))?;
        let siblings: HashSet<_> = snapshot
            .tasks
            .iter()
            .filter(|candidate| candidate.project_id == task.project_id)
            .map(|task| task.id)
            .collect();
        Ok(snapshot.turns.iter().any(|turn| {
            siblings.contains(&turn.task_id)
                && turn.status == TurnStatus::Running
                && except_turn != Some(turn.id.to_string().as_str())
        }))
    }

}

#[cfg(test)]
#[path = "project_execution_tests.rs"]
mod tests;
