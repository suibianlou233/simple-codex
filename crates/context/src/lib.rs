#![forbid(unsafe_code)]

mod budget;
mod builder;
mod packet;
mod protocol;

pub use budget::{BudgetError, ContextBudgeter};
pub use builder::{BuiltContext, ContextBuildError, ContextBuildInput, ContextBuilder};
pub use packet::{ContextPacket, ContextSource, PacketProvenance, RetentionPolicy, TrustLevel};
pub use protocol::{
    ApprovalKind, BudgetConfig, BudgetedContext, BudgetedMessage, ContextInput, ContextMessage,
    ContextRole, ContextWindowProfile, CriticalFacts, FileHashFact, MessageOrigin, PendingApproval,
    RecentError,
};
