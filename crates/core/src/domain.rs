use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{ItemId, ProjectId, StepId, TaskId, TurnBudget, TurnId};

/// Maximum task title length after surrounding whitespace is removed.
///
/// The count uses Unicode scalar values (`str::chars`), not UTF-8 bytes, so a
/// Chinese character is counted as one. This is intentionally simpler and
/// more stable than locale-dependent grapheme segmentation at the protocol
/// boundary.
pub const MAX_TASK_TITLE_CHARS: usize = 80;

/// A local project selected by the user.
///
/// `root` is validated when the project is opened. It is an identity and
/// convenience value, not a process sandbox boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Project {
    pub id: ProjectId,
    pub root: PathBuf,
    pub name: String,
    pub opened_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Ready,
    Running,
    Completed,
    Failed,
}

/// A user-visible unit of work within one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Task {
    pub id: TaskId,
    pub project_id: ProjectId,
    pub title: String,
    pub status: TaskStatus,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    #[default]
    Preparing,
    Sampling,
    ExecutingTools,
    WaitingApproval,
    ExecutingActions,
    Completed,
    Failed,
    Cancelled,
}

impl TurnPhase {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use TurnPhase::{
            Cancelled, Completed, ExecutingActions, ExecutingTools, Failed, Preparing, Sampling,
            WaitingApproval,
        };
        matches!(
            (self, next),
            (Preparing, Sampling | Failed | Cancelled)
                | (Sampling, ExecutingTools | Completed | Failed | Cancelled)
                | (
                    ExecutingTools,
                    Sampling | WaitingApproval | ExecutingActions | Failed | Cancelled
                )
                | (ExecutingActions, Sampling | Completed | Failed | Cancelled)
                | (
                    WaitingApproval,
                    Sampling | ExecutingActions | Failed | Cancelled
                )
        )
    }
}

impl From<TurnStatus> for TurnPhase {
    fn from(status: TurnStatus) -> Self {
        match status {
            TurnStatus::Running => Self::Preparing,
            TurnStatus::Completed => Self::Completed,
            TurnStatus::Failed => Self::Failed,
            TurnStatus::Cancelled => Self::Cancelled,
        }
    }
}

/// A single user-to-agent interaction. Turn execution is added after phase 0;
/// defining the type now keeps persisted and desktop identifiers stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Turn {
    pub id: TurnId,
    pub task_id: TaskId,
    pub status: TurnStatus,
    #[serde(default)]
    pub phase: TurnPhase,
    #[serde(default)]
    pub budget: TurnBudget,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    ContextAssembly,
    ModelSampling,
    ToolBatch,
    ActionBatch,
    Persistence,
}

/// Internal execution slice. Steps are never presented as user turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Step {
    pub id: StepId,
    pub turn_id: TurnId,
    pub kind: StepKind,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
}

impl Step {
    #[must_use]
    pub fn new(turn_id: TurnId, kind: StepKind, started_at_ms: i64) -> Self {
        Self {
            id: StepId::new(),
            turn_id,
            kind,
            started_at_ms,
            finished_at_ms: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    UserMessage,
    AssistantMessage,
    Evidence,
    ActionIntent,
    ActionResult,
    Error,
}

/// Durable content correlated to a turn and optionally to one internal step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Item {
    pub id: ItemId,
    pub turn_id: TurnId,
    pub step_id: Option<StepId>,
    pub kind: ItemKind,
    pub content: String,
    pub created_at_ms: i64,
}

/// Kernel vocabulary aliases retained alongside the desktop protocol names.
pub type Workspace = Project;
pub type Thread = Task;
