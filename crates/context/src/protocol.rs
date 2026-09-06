use serde::{Deserialize, Serialize};

use crate::{ContextSource, PacketProvenance, RetentionPolicy, TrustLevel};

/// The role used by the provider-neutral model protocol.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A conversation item before local context budgeting.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContextMessage {
    pub id: String,
    pub role: ContextRole,
    pub content: String,
    pub created_at_ms: i64,
}

/// A pending operation that still requires a user decision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    FileChange,
    Command,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingApproval {
    pub approval_id: String,
    pub kind: ApprovalKind,
    pub summary: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecentError {
    pub source: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileHashFact {
    pub path: String,
    pub sha256: String,
}

/// Facts that must survive every successful compaction.
///
/// They are intentionally supplied as typed data rather than inferred from prose.
/// This keeps compaction fast and prevents a local heuristic from silently losing
/// an approval or the revision used for an optimistic write.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CriticalFacts {
    pub user_goal: String,
    #[serde(default)]
    pub pending_approvals: Vec<PendingApproval>,
    pub recent_error: Option<RecentError>,
    #[serde(default)]
    pub file_hashes: Vec<FileHashFact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContextInput {
    #[serde(default)]
    pub system_messages: Vec<ContextMessage>,
    #[serde(default)]
    pub history: Vec<ContextMessage>,
    pub critical: CriticalFacts,
}

/// Hard context limits and deterministic retention knobs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BudgetConfig {
    /// Absolute input ceiling, measured as Unicode scalar values plus a small
    /// per-message framing estimate.
    pub max_chars: usize,
    /// Provider input limit. Set this to the model's advertised context size.
    pub max_tokens: usize,
    /// Space never consumed by input, leaving room for the model response.
    pub reserved_output_tokens: usize,
    /// Conservative local token estimate. Four is a common default; callers can
    /// lower it for CJK-heavy conversations.
    pub chars_per_token: usize,
    pub recent_dialogue_messages: usize,
    pub recent_tool_results: usize,
    pub max_message_chars: usize,
    pub max_summary_chars: usize,
    pub max_summary_entries: usize,
    pub summary_entry_chars: usize,
}

/// The two model-profile limits that must govern every context budget.
///
/// Keeping them together makes it harder for a runtime to accidentally use a
/// global context limit with a different profile's output reserve.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContextWindowProfile {
    pub context_window_tokens: usize,
    pub output_reserve_tokens: usize,
    pub chars_per_token: usize,
}

impl ContextWindowProfile {
    pub const fn new(context_window_tokens: usize, output_reserve_tokens: usize) -> Self {
        Self {
            context_window_tokens,
            output_reserve_tokens,
            chars_per_token: 3,
        }
    }

    pub const fn with_chars_per_token(mut self, chars_per_token: usize) -> Self {
        self.chars_per_token = chars_per_token;
        self
    }
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_chars: 48_000,
            max_tokens: 16_000,
            reserved_output_tokens: 4_000,
            chars_per_token: 3,
            recent_dialogue_messages: 12,
            recent_tool_results: 6,
            max_message_chars: 12_000,
            max_summary_chars: 4_000,
            max_summary_entries: 16,
            summary_entry_chars: 180,
        }
    }
}

impl BudgetConfig {
    /// Creates deterministic budget knobs from the selected model profile.
    pub fn from_profile(profile: ContextWindowProfile) -> Self {
        Self {
            max_chars: profile
                .context_window_tokens
                .saturating_mul(profile.chars_per_token),
            max_tokens: profile.context_window_tokens,
            reserved_output_tokens: profile.output_reserve_tokens,
            chars_per_token: profile.chars_per_token,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    Original,
    CriticalSummary,
    HistorySummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BudgetedMessage {
    pub id: String,
    pub role: ContextRole,
    pub content: String,
    pub created_at_ms: i64,
    pub origin: MessageOrigin,
    pub truncated: bool,
    /// Metadata describes this emitted segment. For a compression summary,
    /// `compressed_from` preserves the metadata of every represented packet.
    #[serde(default)]
    pub source: ContextSource,
    #[serde(default)]
    pub trust: TrustLevel,
    #[serde(default)]
    pub retention: RetentionPolicy,
    #[serde(default)]
    pub compressed_from: Vec<PacketProvenance>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BudgetedContext {
    pub messages: Vec<BudgetedMessage>,
    pub estimated_chars: usize,
    pub estimated_tokens: usize,
    pub omitted_messages: usize,
    pub retained_critical: CriticalFacts,
    /// Metadata for packets omitted without a summary (normally ephemeral
    /// content) so compaction remains inspectable.
    #[serde(default)]
    pub omitted_packets: Vec<PacketProvenance>,
}
