use serde::{Deserialize, Serialize};

/// Where a piece of context originated.
///
/// Source is kept separate from model role so repository text and tool output
/// cannot become trusted instructions merely because a caller chose a role.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    SystemInstruction,
    UserGoal,
    #[default]
    UserMessage,
    AssistantMessage,
    ProjectInstruction,
    /// A note the user explicitly chose to retain for this local project.
    SavedNote,
    WorkspaceContent,
    ToolEvidence,
    ActionResult,
    /// Typed facts produced by the local runtime, such as approval state.
    RuntimeState,
    /// An offline, deterministic compression of older packets.
    Summary,
}

/// The security boundary attached to a context packet.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    TrustedSystem,
    /// Facts validated and supplied by the local runtime. This does not grant
    /// instruction authority and therefore never maps to the system role.
    TrustedLocalState,
    #[default]
    UserProvided,
    ModelGenerated,
    UntrustedWorkspace,
    UntrustedToolOutput,
    /// Locally generated text derived from one or more non-system packets.
    UntrustedDerived,
}

/// How aggressively a packet may be removed when the context is compacted.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionPolicy {
    /// Must be retained in full. A too-small budget is an error.
    Pinned,
    /// Prefer the newest messages according to the budgeter's dialogue/tool limits.
    #[default]
    Recent,
    /// May be represented by the deterministic earlier-context summary.
    Compressible,
    /// May be omitted when it no longer fits the active context.
    Ephemeral,
}

/// A typed context fragment before it is translated to provider-neutral model
/// messages.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContextPacket {
    pub id: String,
    pub source: ContextSource,
    pub trust: TrustLevel,
    pub retention: RetentionPolicy,
    pub content: String,
    pub created_at_ms: i64,
}

/// Source metadata retained when a packet is compressed or omitted.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PacketProvenance {
    pub id: String,
    pub source: ContextSource,
    pub trust: TrustLevel,
    pub retention: RetentionPolicy,
}

impl From<&ContextPacket> for PacketProvenance {
    fn from(packet: &ContextPacket) -> Self {
        Self {
            id: packet.id.clone(),
            source: packet.source,
            trust: packet.trust,
            retention: packet.retention,
        }
    }
}

impl ContextPacket {
    pub fn new(
        id: impl Into<String>,
        source: ContextSource,
        trust: TrustLevel,
        retention: RetentionPolicy,
        content: impl Into<String>,
        created_at_ms: i64,
    ) -> Self {
        Self {
            id: id.into(),
            source,
            trust,
            retention,
            content: content.into(),
            created_at_ms,
        }
    }
}
