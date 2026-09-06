//! Domain types and the local task state machine.
//!
//! This crate owns the protocol shared by the desktop shell and the future
//! agent runtime. It deliberately has no dependency on Tauri, SQLite, model
//! providers, or tool implementations.

mod action_gate;
mod budget;
mod domain;
mod engine;
mod error;
mod ids;
mod protocol;
mod service;
mod state;

pub use action_gate::{
    ActionGate, ActionGateDecision, ApprovalPolicy, CapabilityBoundary, CapabilityDenial,
};
pub use budget::{BudgetDimension, BudgetExceeded, TurnBudget, TurnBudgetUsage};
pub use domain::{
    Item, ItemKind, MAX_TASK_TITLE_CHARS, Project, Step, StepKind, Task, TaskStatus, Thread, Turn,
    TurnPhase, TurnStatus, Workspace,
};
pub use engine::{EngineError, TurnEngine};
pub use error::{CoreError, ProjectionError};
pub use ids::{ItemId, ProjectId, StepId, TaskId, ThreadId, TurnId, WorkspaceId};
pub use protocol::{AppCommand, AppEvent};
pub use service::InMemoryTaskService;
pub use state::{AppState, StateProjection, StateSnapshot};
