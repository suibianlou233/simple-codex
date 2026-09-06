use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{BudgetExceeded, Step, StepKind, TurnBudget, TurnBudgetUsage, TurnId, TurnPhase};

/// Provider- and UI-independent control plane for one turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TurnEngine {
    turn_id: TurnId,
    phase: TurnPhase,
    budget: TurnBudget,
    usage: TurnBudgetUsage,
    active_step: Option<Step>,
}

impl TurnEngine {
    #[must_use]
    pub fn new(turn_id: TurnId, budget: TurnBudget) -> Self {
        Self {
            turn_id,
            phase: TurnPhase::Preparing,
            budget,
            usage: TurnBudgetUsage::default(),
            active_step: None,
        }
    }

    #[must_use]
    pub fn restore(
        turn_id: TurnId,
        phase: TurnPhase,
        budget: TurnBudget,
        usage: TurnBudgetUsage,
    ) -> Self {
        Self {
            turn_id,
            phase,
            budget,
            usage,
            active_step: None,
        }
    }

    #[must_use]
    pub const fn turn_id(&self) -> TurnId {
        self.turn_id
    }

    #[must_use]
    pub const fn phase(&self) -> TurnPhase {
        self.phase
    }

    #[must_use]
    pub const fn usage(&self) -> TurnBudgetUsage {
        self.usage
    }

    #[must_use]
    pub const fn budget(&self) -> TurnBudget {
        self.budget
    }

    #[must_use]
    pub fn active_step(&self) -> Option<&Step> {
        self.active_step.as_ref()
    }

    pub fn begin_sampling(&mut self, now_ms: i64, elapsed_ms: u64) -> Result<Step, EngineError> {
        self.transition(TurnPhase::Sampling)?;
        self.usage.begin_model_round();
        self.usage.set_elapsed_ms(elapsed_ms);
        self.check_budget()?;
        Ok(self.start_step(StepKind::ModelSampling, now_ms))
    }

    pub fn begin_tools(
        &mut self,
        count: usize,
        now_ms: i64,
        elapsed_ms: u64,
    ) -> Result<Step, EngineError> {
        self.transition(TurnPhase::ExecutingTools)?;
        self.usage.record_tool_calls(count);
        self.usage.set_elapsed_ms(elapsed_ms);
        self.check_budget()?;
        Ok(self.start_step(StepKind::ToolBatch, now_ms))
    }

    pub fn begin_actions(&mut self, now_ms: i64, elapsed_ms: u64) -> Result<Step, EngineError> {
        self.transition(TurnPhase::ExecutingActions)?;
        self.usage.set_elapsed_ms(elapsed_ms);
        self.check_budget()?;
        Ok(self.start_step(StepKind::ActionBatch, now_ms))
    }

    pub fn wait_for_approval(&mut self) -> Result<(), EngineError> {
        self.transition(TurnPhase::WaitingApproval)
    }

    pub fn record_tokens(
        &mut self,
        input: u64,
        output: u64,
        elapsed_ms: u64,
    ) -> Result<(), EngineError> {
        self.usage.record_tokens(input, output);
        self.usage.set_elapsed_ms(elapsed_ms);
        self.check_budget()
    }

    pub fn record_output_chars(
        &mut self,
        chars: usize,
        elapsed_ms: u64,
    ) -> Result<(), EngineError> {
        self.usage.record_output_chars(chars);
        self.usage.set_elapsed_ms(elapsed_ms);
        self.check_budget()
    }

    pub fn record_recovery(&mut self, elapsed_ms: u64) -> Result<(), EngineError> {
        self.usage.record_recovery();
        self.usage.set_elapsed_ms(elapsed_ms);
        self.check_budget()
    }

    pub fn complete(&mut self) -> Result<(), EngineError> {
        self.transition(TurnPhase::Completed)
    }
    pub fn fail(&mut self) -> Result<(), EngineError> {
        self.transition(TurnPhase::Failed)
    }
    pub fn cancel(&mut self) -> Result<(), EngineError> {
        self.transition(TurnPhase::Cancelled)
    }

    pub fn finish_active_step(&mut self, finished_at_ms: i64) {
        if let Some(step) = &mut self.active_step {
            step.finished_at_ms = Some(finished_at_ms);
        }
    }

    fn start_step(&mut self, kind: StepKind, started_at_ms: i64) -> Step {
        self.finish_active_step(started_at_ms);
        let step = Step::new(self.turn_id, kind, started_at_ms);
        self.active_step = Some(step.clone());
        step
    }

    fn check_budget(&self) -> Result<(), EngineError> {
        self.usage.check(self.budget).map_err(EngineError::Budget)
    }

    fn transition(&mut self, next: TurnPhase) -> Result<(), EngineError> {
        if !self.phase.can_transition_to(next) {
            return Err(EngineError::InvalidTransition {
                from: self.phase,
                to: next,
            });
        }
        self.phase = next;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EngineError {
    #[error("invalid turn transition: {from:?} -> {to:?}")]
    InvalidTransition { from: TurnPhase, to: TurnPhase },
    #[error(transparent)]
    Budget(#[from] BudgetExceeded),
}

#[cfg(test)]
mod tests {
    use super::TurnEngine;
    use crate::{TurnBudget, TurnId, TurnPhase};

    #[test]
    fn approval_resume_stays_in_the_same_turn() {
        let id = TurnId::new();
        let mut engine = TurnEngine::new(id, TurnBudget::default());
        engine.begin_sampling(1, 0).expect("sampling");
        engine.begin_tools(1, 2, 1).expect("tools");
        engine.wait_for_approval().expect("wait");
        engine.begin_sampling(3, 2).expect("resume");
        engine.complete().expect("complete");
        assert_eq!(engine.turn_id(), id);
        assert_eq!(engine.phase(), TurnPhase::Completed);
        assert_eq!(engine.usage().model_rounds, 2);
    }

    #[test]
    fn terminal_turn_cannot_resume() {
        let mut engine = TurnEngine::new(TurnId::new(), TurnBudget::default());
        engine.begin_sampling(1, 0).expect("sampling");
        engine.complete().expect("complete");
        assert!(engine.begin_sampling(2, 1).is_err());
    }
}
