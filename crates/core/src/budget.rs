use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Composite safety budget for one user turn.
///
/// A turn may pause for approval and resume without resetting these counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TurnBudget {
    pub max_model_rounds: u32,
    pub max_tool_calls: u32,
    pub max_elapsed_ms: u64,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_output_chars: u64,
    pub max_recovery_attempts: u32,
}

impl Default for TurnBudget {
    fn default() -> Self {
        Self {
            max_model_rounds: 16,
            max_tool_calls: 64,
            max_elapsed_ms: 15 * 60 * 1_000,
            max_input_tokens: 2_000_000,
            max_output_tokens: 200_000,
            max_output_chars: 1_000_000,
            max_recovery_attempts: 6,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TurnBudgetUsage {
    pub model_rounds: u32,
    pub tool_calls: u32,
    pub elapsed_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub output_chars: u64,
    pub recovery_attempts: u32,
}

impl TurnBudgetUsage {
    pub fn begin_model_round(&mut self) {
        self.model_rounds = self.model_rounds.saturating_add(1);
    }

    pub fn record_tool_calls(&mut self, count: usize) {
        self.tool_calls = self
            .tool_calls
            .saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    }

    pub fn record_tokens(&mut self, input: u64, output: u64) {
        self.input_tokens = self.input_tokens.saturating_add(input);
        self.output_tokens = self.output_tokens.saturating_add(output);
    }

    pub fn record_output_chars(&mut self, chars: usize) {
        self.output_chars = self
            .output_chars
            .saturating_add(u64::try_from(chars).unwrap_or(u64::MAX));
    }

    pub fn record_recovery(&mut self) {
        self.recovery_attempts = self.recovery_attempts.saturating_add(1);
    }

    pub fn set_elapsed_ms(&mut self, elapsed_ms: u64) {
        self.elapsed_ms = self.elapsed_ms.max(elapsed_ms);
    }

    pub fn check(&self, budget: TurnBudget) -> Result<(), BudgetExceeded> {
        let checks = [
            (
                self.model_rounds as u64,
                budget.max_model_rounds as u64,
                BudgetDimension::ModelRounds,
            ),
            (
                self.tool_calls as u64,
                budget.max_tool_calls as u64,
                BudgetDimension::ToolCalls,
            ),
            (
                self.elapsed_ms,
                budget.max_elapsed_ms,
                BudgetDimension::ElapsedMs,
            ),
            (
                self.input_tokens,
                budget.max_input_tokens,
                BudgetDimension::InputTokens,
            ),
            (
                self.output_tokens,
                budget.max_output_tokens,
                BudgetDimension::OutputTokens,
            ),
            (
                self.output_chars,
                budget.max_output_chars,
                BudgetDimension::OutputChars,
            ),
            (
                self.recovery_attempts as u64,
                budget.max_recovery_attempts as u64,
                BudgetDimension::RecoveryAttempts,
            ),
        ];
        for (actual, limit, dimension) in checks {
            if actual > limit {
                return Err(BudgetExceeded {
                    dimension,
                    limit,
                    actual,
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetDimension {
    ModelRounds,
    ToolCalls,
    ElapsedMs,
    InputTokens,
    OutputTokens,
    OutputChars,
    RecoveryAttempts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("turn budget exceeded for {dimension:?}: {actual} > {limit}")]
pub struct BudgetExceeded {
    pub dimension: BudgetDimension,
    pub limit: u64,
    pub actual: u64,
}

#[cfg(test)]
mod tests {
    use super::{BudgetDimension, TurnBudget, TurnBudgetUsage};

    #[test]
    fn composite_budget_reports_the_first_exhausted_dimension() {
        let mut usage = TurnBudgetUsage::default();
        usage.begin_model_round();
        usage.begin_model_round();
        let budget = TurnBudget {
            max_model_rounds: 1,
            ..TurnBudget::default()
        };
        let error = usage
            .check(budget)
            .expect_err("round limit must be enforced");
        assert_eq!(error.dimension, BudgetDimension::ModelRounds);
    }

    #[test]
    fn approval_pause_does_not_require_resetting_usage() {
        let mut usage = TurnBudgetUsage::default();
        usage.begin_model_round();
        usage.record_tool_calls(2);
        usage.set_elapsed_ms(100);
        usage.set_elapsed_ms(40);
        assert_eq!(usage.model_rounds, 1);
        assert_eq!(usage.tool_calls, 2);
        assert_eq!(usage.elapsed_ms, 100);
        usage
            .check(TurnBudget::default())
            .expect("default budget should allow progress");
    }
}
