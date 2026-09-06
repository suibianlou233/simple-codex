//! Local SQLite persistence for tasks and their append-only event streams.
//!
//! This crate deliberately owns no agent or UI behavior. Callers provide stable
//! identifiers and timestamps; storage atomically assigns each task event's
//! strictly increasing sequence number.

mod action;
mod codex_binding;
mod journal;
mod migrations;
mod native_history;
mod repository;

pub use action::ActionClaim;
pub use action::ActionClaimStatus;
pub use action::ActionExecutionResult;
pub use action::ApprovalDeclaration;
pub use action::BeginActionOutcome;
pub use action::NewActionIntent;
pub use action::PrepareActionOutcome;
pub use codex_binding::CodexThreadBinding;
pub use codex_binding::NewCodexThreadBinding;
pub use journal::ActionRecoveryStatus;
pub use journal::JournalEvent;
pub use journal::NewJournalEvent;
pub use journal::RecoveredAction;
pub use journal::RecoveredTurn;
pub use journal::RecoveryView;
pub use journal::ThreadReplay;
pub use journal::ThreadSnapshot;
pub use journal::journal_event_types;
pub use repository::EventRecord;
pub use repository::ModelProfileRecord;
pub use repository::NewEvent;
pub use repository::NewModelProfile;
pub use repository::NewProject;
pub use repository::NewTask;
pub use repository::ProjectMemoryNotes;
pub use repository::ProjectRecord;
pub use repository::Storage;
pub use repository::StorageError;
pub use repository::TaskRecord;
pub use repository::TaskSnapshot;
