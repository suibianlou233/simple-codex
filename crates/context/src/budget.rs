use std::{collections::HashSet, error::Error, fmt};

use crate::{
    BuiltContext, ContextPacket, ContextSource, PacketProvenance, RetentionPolicy, TrustLevel,
    protocol::{
        ApprovalKind, BudgetConfig, BudgetedContext, BudgetedMessage, ContextInput, ContextMessage,
        ContextRole, CriticalFacts, MessageOrigin,
    },
};

const MESSAGE_OVERHEAD_CHARS: usize = 12;
const TRUNCATION_MARKER: &str = "\n[…已在本地截断…]\n";
const CRITICAL_ID: &str = "local-agent:critical-context";
const HISTORY_ID: &str = "local-agent:earlier-context";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BudgetError {
    InvalidConfig(&'static str),
    MissingUserGoal,
    InvalidCriticalFact { field: &'static str, value: String },
    DuplicateMessageId { message_id: String },
    ReservedMessageId { message_id: String },
    BudgetTooSmall { available: usize, required: usize },
}

impl fmt::Display for BudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(field) => {
                write!(formatter, "上下文预算字段无效：{field}")
            }
            Self::MissingUserGoal => formatter.write_str("用户目标不能为空"),
            Self::InvalidCriticalFact { field, value } => {
                write!(formatter, "关键上下文字段 {field} 无效：{value}")
            }
            Self::DuplicateMessageId { message_id } => {
                write!(formatter, "上下文消息 ID 重复：{message_id}")
            }
            Self::ReservedMessageId { message_id } => {
                write!(formatter, "上下文消息使用了保留 ID：{message_id}")
            }
            Self::BudgetTooSmall {
                available,
                required,
            } => write!(
                formatter,
                "上下文预算过小：可用 {available} 个字符，至少需要 {required} 个"
            ),
        }
    }
}

impl Error for BudgetError {}

/// A deterministic, offline context selector.
#[derive(Clone, Debug)]
pub struct ContextBudgeter {
    config: BudgetConfig,
}

#[derive(Clone, Debug)]
struct TrackedMessage {
    id: String,
    role: ContextRole,
    content: String,
    created_at_ms: i64,
    source: ContextSource,
    trust: TrustLevel,
    retention: RetentionPolicy,
}

impl TrackedMessage {
    fn from_packet(packet: &ContextPacket) -> Self {
        Self {
            id: packet.id.clone(),
            role: packet_role(packet),
            content: packet.content.clone(),
            created_at_ms: packet.created_at_ms,
            source: packet.source,
            trust: packet.trust,
            retention: packet.retention,
        }
    }

    fn from_legacy_pinned(message: &ContextMessage) -> Self {
        let (source, trust) = legacy_metadata(message.role, true);
        Self {
            id: message.id.clone(),
            role: message.role,
            content: message.content.clone(),
            created_at_ms: message.created_at_ms,
            source,
            trust,
            retention: RetentionPolicy::Pinned,
        }
    }

    fn from_legacy_history(message: &ContextMessage) -> Self {
        let role = if message.role == ContextRole::System {
            ContextRole::User
        } else {
            message.role
        };
        let (source, trust) = legacy_metadata(message.role, false);
        Self {
            id: message.id.clone(),
            role,
            content: message.content.clone(),
            created_at_ms: message.created_at_ms,
            source,
            trust,
            retention: RetentionPolicy::Recent,
        }
    }

    fn provenance(&self) -> PacketProvenance {
        PacketProvenance {
            id: self.id.clone(),
            source: self.source,
            trust: self.trust,
            retention: self.retention,
        }
    }
}

fn packet_role(packet: &ContextPacket) -> ContextRole {
    match (packet.source, packet.trust) {
        (ContextSource::SystemInstruction, TrustLevel::TrustedSystem) => ContextRole::System,
        (ContextSource::AssistantMessage, TrustLevel::ModelGenerated) => ContextRole::Assistant,
        (ContextSource::ToolEvidence, TrustLevel::UntrustedToolOutput)
        | (ContextSource::ActionResult, TrustLevel::UntrustedToolOutput) => ContextRole::Tool,
        _ => ContextRole::User,
    }
}

fn legacy_metadata(role: ContextRole, pinned: bool) -> (ContextSource, TrustLevel) {
    match (role, pinned) {
        (ContextRole::System, true) => {
            (ContextSource::SystemInstruction, TrustLevel::TrustedSystem)
        }
        (ContextRole::System, false) => (ContextSource::Summary, TrustLevel::UntrustedDerived),
        (ContextRole::User, _) => (ContextSource::UserMessage, TrustLevel::UserProvided),
        (ContextRole::Assistant, _) => {
            (ContextSource::AssistantMessage, TrustLevel::ModelGenerated)
        }
        (ContextRole::Tool, _) => (ContextSource::ToolEvidence, TrustLevel::UntrustedToolOutput),
    }
}

impl ContextBudgeter {
    pub fn new(config: BudgetConfig) -> Result<Self, BudgetError> {
        validate_config(&config)?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &BudgetConfig {
        &self.config
    }

    pub fn build(&self, input: &ContextInput) -> Result<BudgetedContext, BudgetError> {
        let pinned = input
            .system_messages
            .iter()
            .map(TrackedMessage::from_legacy_pinned)
            .collect::<Vec<_>>();
        let history = input
            .history
            .iter()
            .map(TrackedMessage::from_legacy_history)
            .collect::<Vec<_>>();
        self.build_tracked(&pinned, &history, &input.critical)
    }

    /// Budgets the packet-aware builder output without discarding provenance.
    pub fn build_packets(&self, input: &BuiltContext) -> Result<BudgetedContext, BudgetError> {
        let pinned = input
            .pinned_packets
            .iter()
            .map(TrackedMessage::from_packet)
            .collect::<Vec<_>>();
        let history = input
            .history_packets
            .iter()
            .map(TrackedMessage::from_packet)
            .collect::<Vec<_>>();
        self.build_tracked(&pinned, &history, &input.critical)
    }

    fn build_tracked(
        &self,
        pinned: &[TrackedMessage],
        history: &[TrackedMessage],
        critical: &CriticalFacts,
    ) -> Result<BudgetedContext, BudgetError> {
        validate_critical(critical)?;
        validate_message_ids(pinned, history)?;

        let available = self.available_chars();
        let critical_content = render_critical(critical);
        let required = pinned
            .iter()
            .map(message_cost)
            .sum::<usize>()
            .saturating_add(content_cost(&critical_content));

        if required > available {
            return Err(BudgetError::BudgetTooSmall {
                available,
                required,
            });
        }

        let mut messages = Vec::new();
        for message in pinned {
            messages.push(original_message(message, message.content.clone(), false));
        }
        messages.push(BudgetedMessage {
            id: CRITICAL_ID.to_owned(),
            // Goals, approval state, errors, and file revisions are runtime facts,
            // not trusted instructions. Keep them pinned without promoting them
            // to the system role.
            role: ContextRole::User,
            content: critical_content,
            created_at_ms: minimum_timestamp(pinned, history),
            origin: MessageOrigin::CriticalSummary,
            truncated: false,
            source: ContextSource::RuntimeState,
            trust: TrustLevel::TrustedLocalState,
            retention: RetentionPolicy::Pinned,
            compressed_from: Vec::new(),
        });

        let mut used = cost_of_budgeted(&messages);
        let mut remaining = available.saturating_sub(used);
        let priority_ids = recent_priority_ids(history, &self.config);
        let summary_reserve = summary_reserve(history, remaining, &self.config);
        let mut recent_budget = remaining.saturating_sub(summary_reserve);
        let mut selected = Vec::new();

        for message in history.iter().rev() {
            if !priority_ids.contains(&message.id) || recent_budget <= MESSAGE_OVERHEAD_CHARS {
                continue;
            }

            let content_budget = recent_budget
                .saturating_sub(MESSAGE_OVERHEAD_CHARS)
                .min(self.config.max_message_chars);
            if content_budget == 0 {
                continue;
            }
            let (content, truncated) = truncate_middle(&message.content, content_budget);
            let cost = content_cost(&content);
            if content.is_empty() || cost > recent_budget {
                continue;
            }
            recent_budget -= cost;
            selected.push(original_message(message, content, truncated));
        }
        selected.reverse();

        let selected_ids: HashSet<&str> = selected.iter().map(|item| item.id.as_str()).collect();
        let omitted: Vec<&TrackedMessage> = history
            .iter()
            .filter(|item| !selected_ids.contains(item.id.as_str()))
            .collect();

        used = used.saturating_add(cost_of_budgeted(&selected));
        remaining = available.saturating_sub(used);
        let mut compressed_from = Vec::new();
        if let Some(summary) = render_history_summary(&omitted, remaining, &self.config) {
            compressed_from = omitted
                .iter()
                .filter(|message| message.retention != RetentionPolicy::Ephemeral)
                .map(|message| message.provenance())
                .collect();
            messages.push(BudgetedMessage {
                id: HISTORY_ID.to_owned(),
                // A summary derived from conversation, workspace, or tool text
                // is data, not a system instruction.
                role: ContextRole::User,
                content: summary,
                created_at_ms: omitted.first().map_or(0, |message| message.created_at_ms),
                origin: MessageOrigin::HistorySummary,
                truncated: false,
                source: ContextSource::Summary,
                trust: TrustLevel::UntrustedDerived,
                retention: RetentionPolicy::Compressible,
                compressed_from: compressed_from.clone(),
            });
        }
        messages.extend(selected);

        let estimated_chars = cost_of_budgeted(&messages);
        let estimated_tokens = div_ceil(estimated_chars, self.config.chars_per_token);
        debug_assert!(estimated_chars <= available);
        debug_assert!(
            estimated_tokens + self.config.reserved_output_tokens <= self.config.max_tokens
        );

        Ok(BudgetedContext {
            messages,
            estimated_chars,
            estimated_tokens,
            omitted_messages: omitted.len(),
            retained_critical: critical.clone(),
            omitted_packets: omitted
                .into_iter()
                .filter(|message| {
                    !compressed_from
                        .iter()
                        .any(|provenance| provenance.id == message.id)
                })
                .map(TrackedMessage::provenance)
                .collect(),
        })
    }

    fn available_chars(&self) -> usize {
        let input_tokens = self
            .config
            .max_tokens
            .saturating_sub(self.config.reserved_output_tokens);
        self.config
            .max_chars
            .min(input_tokens.saturating_mul(self.config.chars_per_token))
    }
}

fn validate_config(config: &BudgetConfig) -> Result<(), BudgetError> {
    if config.max_chars == 0 {
        return Err(BudgetError::InvalidConfig("max_chars"));
    }
    if config.max_tokens == 0 || config.reserved_output_tokens >= config.max_tokens {
        return Err(BudgetError::InvalidConfig("max_tokens"));
    }
    if config.chars_per_token == 0 {
        return Err(BudgetError::InvalidConfig("chars_per_token"));
    }
    if config.max_message_chars == 0 {
        return Err(BudgetError::InvalidConfig("max_message_chars"));
    }
    if config.max_summary_entries > 0 && config.summary_entry_chars == 0 {
        return Err(BudgetError::InvalidConfig("summary_entry_chars"));
    }
    Ok(())
}

fn validate_critical(critical: &CriticalFacts) -> Result<(), BudgetError> {
    if critical.user_goal.trim().is_empty() {
        return Err(BudgetError::MissingUserGoal);
    }
    let mut approval_ids = HashSet::new();
    for approval in &critical.pending_approvals {
        if approval.approval_id.trim().is_empty() || approval.summary.trim().is_empty() {
            return Err(BudgetError::InvalidCriticalFact {
                field: "approval_id",
                value: approval.approval_id.clone(),
            });
        }
        if !approval_ids.insert(approval.approval_id.as_str()) {
            return Err(BudgetError::InvalidCriticalFact {
                field: "duplicate_approval_id",
                value: approval.approval_id.clone(),
            });
        }
    }
    if let Some(error) = &critical.recent_error
        && (error.source.trim().is_empty() || error.message.trim().is_empty())
    {
        return Err(BudgetError::InvalidCriticalFact {
            field: "recent_error",
            value: error.source.clone(),
        });
    }
    let mut file_paths = HashSet::new();
    for file in &critical.file_hashes {
        let valid_hash = file.sha256.len() == 64
            && file
                .sha256
                .bytes()
                .all(|character| character.is_ascii_hexdigit());
        if file.path.trim().is_empty() || !valid_hash {
            return Err(BudgetError::InvalidCriticalFact {
                field: "file_hash",
                value: file.path.clone(),
            });
        }
        if !file_paths.insert(file.path.as_str()) {
            return Err(BudgetError::InvalidCriticalFact {
                field: "duplicate_file_hash",
                value: file.path.clone(),
            });
        }
    }
    Ok(())
}

fn validate_message_ids(
    pinned: &[TrackedMessage],
    history: &[TrackedMessage],
) -> Result<(), BudgetError> {
    let mut ids = HashSet::new();
    for message in pinned.iter().chain(history) {
        if matches!(message.id.as_str(), CRITICAL_ID | HISTORY_ID) {
            return Err(BudgetError::ReservedMessageId {
                message_id: message.id.clone(),
            });
        }
        if !ids.insert(message.id.as_str()) {
            return Err(BudgetError::DuplicateMessageId {
                message_id: message.id.clone(),
            });
        }
    }
    Ok(())
}

fn render_critical(critical: &CriticalFacts) -> String {
    let mut lines = vec![
        "[Local Agent critical context — retain exactly]".to_owned(),
        format!("USER_GOAL: {:?}", critical.user_goal),
    ];

    if critical.pending_approvals.is_empty() {
        lines.push("PENDING_APPROVALS: none".to_owned());
    } else {
        lines.push("PENDING_APPROVALS:".to_owned());
        let mut approvals = critical.pending_approvals.iter().collect::<Vec<_>>();
        approvals.sort_by(|left, right| left.approval_id.cmp(&right.approval_id));
        for approval in approvals {
            let kind = match approval.kind {
                ApprovalKind::FileChange => "file_change",
                ApprovalKind::Command => "command",
            };
            lines.push(format!(
                "- id={:?} kind={} summary={:?}",
                approval.approval_id, kind, approval.summary
            ));
        }
    }

    match &critical.recent_error {
        Some(error) => lines.push(format!(
            "RECENT_ERROR: source={:?} message={:?}",
            error.source, error.message
        )),
        None => lines.push("RECENT_ERROR: none".to_owned()),
    }

    if critical.file_hashes.is_empty() {
        lines.push("FILE_HASHES: none".to_owned());
    } else {
        lines.push("FILE_HASHES:".to_owned());
        let mut hashes = critical.file_hashes.iter().collect::<Vec<_>>();
        hashes.sort_by(|left, right| left.path.cmp(&right.path));
        for file in hashes {
            lines.push(format!(
                "- path={:?} sha256={}",
                file.path,
                file.sha256.to_ascii_lowercase()
            ));
        }
    }
    lines.join("\n")
}

fn recent_priority_ids(history: &[TrackedMessage], config: &BudgetConfig) -> HashSet<String> {
    let mut dialogue = 0;
    let mut tools = 0;
    let mut ids = HashSet::new();
    for message in history.iter().rev() {
        match message.role {
            ContextRole::Tool if tools < config.recent_tool_results => {
                ids.insert(message.id.clone());
                tools += 1;
            }
            ContextRole::User | ContextRole::Assistant
                if dialogue < config.recent_dialogue_messages =>
            {
                ids.insert(message.id.clone());
                dialogue += 1;
            }
            ContextRole::System
            | ContextRole::Tool
            | ContextRole::User
            | ContextRole::Assistant => {}
        }
    }
    ids
}

fn summary_reserve(history: &[TrackedMessage], remaining: usize, config: &BudgetConfig) -> usize {
    let compressible = history
        .iter()
        .filter(|message| message.retention != RetentionPolicy::Ephemeral)
        .count();
    if compressible == 0 || config.max_summary_chars == 0 {
        return 0;
    }
    let minimum = content_cost(&format!(
        "[Earlier context: {compressible} messages compressed]"
    ));
    minimum
        .min(
            config
                .max_summary_chars
                .saturating_add(MESSAGE_OVERHEAD_CHARS),
        )
        .min(remaining)
}

fn render_history_summary(
    omitted: &[&TrackedMessage],
    available: usize,
    config: &BudgetConfig,
) -> Option<String> {
    let compressible = omitted
        .iter()
        .copied()
        .filter(|message| message.retention != RetentionPolicy::Ephemeral)
        .collect::<Vec<_>>();
    if compressible.is_empty()
        || available <= MESSAGE_OVERHEAD_CHARS
        || config.max_summary_chars == 0
    {
        return None;
    }
    let content_limit = available
        .saturating_sub(MESSAGE_OVERHEAD_CHARS)
        .min(config.max_summary_chars);
    let header = format!(
        "[Earlier context: {} messages compressed]",
        compressible.len()
    );
    if char_count(&header) > content_limit {
        return None;
    }

    let mut summary = header;
    let start = compressible
        .len()
        .saturating_sub(config.max_summary_entries);
    for message in &compressible[start..] {
        let preview = truncate_end(&one_line(&message.content), config.summary_entry_chars);
        if preview.is_empty() {
            continue;
        }
        let entry = format!(
            "\n- id={:?} source={} trust={} retention={} role={}: {preview}",
            message.id,
            source_name(message.source),
            trust_name(message.trust),
            retention_name(message.retention),
            role_name(message.role)
        );
        if char_count(&summary).saturating_add(char_count(&entry)) > content_limit {
            break;
        }
        summary.push_str(&entry);
    }
    Some(summary)
}

fn truncate_middle(content: &str, limit: usize) -> (String, bool) {
    if char_count(content) <= limit {
        return (content.to_owned(), false);
    }
    let marker_len = char_count(TRUNCATION_MARKER);
    if limit <= marker_len {
        return (truncate_end(content, limit), true);
    }
    let payload = limit - marker_len;
    let head_len = payload.div_ceil(2);
    let tail_len = payload / 2;
    let head: String = content.chars().take(head_len).collect();
    let tail: String = content
        .chars()
        .rev()
        .take(tail_len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    (format!("{head}{TRUNCATION_MARKER}{tail}"), true)
}

fn truncate_end(content: &str, limit: usize) -> String {
    content.chars().take(limit).collect()
}

fn one_line(content: &str) -> String {
    content
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn original_message(message: &TrackedMessage, content: String, truncated: bool) -> BudgetedMessage {
    BudgetedMessage {
        id: message.id.clone(),
        role: message.role,
        content,
        created_at_ms: message.created_at_ms,
        origin: MessageOrigin::Original,
        truncated,
        source: message.source,
        trust: message.trust,
        retention: message.retention,
        compressed_from: Vec::new(),
    }
}

fn minimum_timestamp(system: &[TrackedMessage], history: &[TrackedMessage]) -> i64 {
    system
        .iter()
        .chain(history)
        .map(|message| message.created_at_ms)
        .min()
        .unwrap_or(0)
}

fn source_name(source: ContextSource) -> &'static str {
    match source {
        ContextSource::SystemInstruction => "system_instruction",
        ContextSource::UserGoal => "user_goal",
        ContextSource::UserMessage => "user_message",
        ContextSource::AssistantMessage => "assistant_message",
        ContextSource::ProjectInstruction => "project_instruction",
        ContextSource::SavedNote => "saved_note",
        ContextSource::WorkspaceContent => "workspace_content",
        ContextSource::ToolEvidence => "tool_evidence",
        ContextSource::ActionResult => "action_result",
        ContextSource::RuntimeState => "runtime_state",
        ContextSource::Summary => "summary",
    }
}

fn trust_name(trust: TrustLevel) -> &'static str {
    match trust {
        TrustLevel::TrustedSystem => "trusted_system",
        TrustLevel::TrustedLocalState => "trusted_local_state",
        TrustLevel::UserProvided => "user_provided",
        TrustLevel::ModelGenerated => "model_generated",
        TrustLevel::UntrustedWorkspace => "untrusted_workspace",
        TrustLevel::UntrustedToolOutput => "untrusted_tool_output",
        TrustLevel::UntrustedDerived => "untrusted_derived",
    }
}

fn retention_name(retention: RetentionPolicy) -> &'static str {
    match retention {
        RetentionPolicy::Pinned => "pinned",
        RetentionPolicy::Recent => "recent",
        RetentionPolicy::Compressible => "compressible",
        RetentionPolicy::Ephemeral => "ephemeral",
    }
}

fn role_name(role: ContextRole) -> &'static str {
    match role {
        ContextRole::System => "system",
        ContextRole::User => "user",
        ContextRole::Assistant => "assistant",
        ContextRole::Tool => "tool",
    }
}

fn message_cost(message: &TrackedMessage) -> usize {
    content_cost(&message.content)
}

fn content_cost(content: &str) -> usize {
    char_count(content).saturating_add(MESSAGE_OVERHEAD_CHARS)
}

fn cost_of_budgeted(messages: &[BudgetedMessage]) -> usize {
    messages
        .iter()
        .map(|message| content_cost(&message.content))
        .sum()
}

fn char_count(content: &str) -> usize {
    content.chars().count()
}

fn div_ceil(value: usize, divisor: usize) -> usize {
    value / divisor + usize::from(!value.is_multiple_of(divisor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ContextWindowProfile, FileHashFact, PendingApproval, RecentError};

    fn message(id: &str, role: ContextRole, content: &str, created_at_ms: i64) -> ContextMessage {
        ContextMessage {
            id: id.to_owned(),
            role,
            content: content.to_owned(),
            created_at_ms,
        }
    }

    fn critical() -> CriticalFacts {
        CriticalFacts {
            user_goal: "修复保存时的数据竞争".to_owned(),
            pending_approvals: vec![PendingApproval {
                approval_id: "approval-7".to_owned(),
                kind: ApprovalKind::FileChange,
                summary: "修改 src/state.rs".to_owned(),
            }],
            recent_error: Some(RecentError {
                source: "cargo test".to_owned(),
                message: "state::tests::save_conflict failed".to_owned(),
            }),
            file_hashes: vec![FileHashFact {
                path: "src/state.rs".to_owned(),
                sha256: "A".repeat(64),
            }],
        }
    }

    fn tight_config() -> BudgetConfig {
        BudgetConfig {
            max_chars: 900,
            max_tokens: 900,
            reserved_output_tokens: 100,
            chars_per_token: 1,
            recent_dialogue_messages: 2,
            recent_tool_results: 1,
            max_message_chars: 180,
            max_summary_chars: 150,
            max_summary_entries: 4,
            summary_entry_chars: 40,
        }
    }

    #[test]
    fn critical_facts_survive_tight_budget() -> Result<(), Box<dyn Error>> {
        let history = (0..20)
            .map(|index| {
                message(
                    &format!("m-{index}"),
                    if index % 3 == 0 {
                        ContextRole::Tool
                    } else {
                        ContextRole::Assistant
                    },
                    &"旧工具和对话内容".repeat(30),
                    index,
                )
            })
            .collect();
        let input = ContextInput {
            system_messages: vec![message(
                "system",
                ContextRole::System,
                "只修改项目内文件",
                0,
            )],
            history,
            critical: critical(),
        };

        let result = ContextBudgeter::new(tight_config())?.build(&input)?;
        let rendered = result
            .messages
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("修复保存时的数据竞争"));
        assert!(rendered.contains("approval-7"));
        assert!(rendered.contains("state::tests::save_conflict failed"));
        assert!(rendered.contains(&"a".repeat(64)));
        assert_eq!(result.retained_critical, input.critical);
        assert!(result.estimated_chars <= 800);
        Ok(())
    }

    #[test]
    fn returns_error_instead_of_dropping_critical_facts() -> Result<(), Box<dyn Error>> {
        let mut config = tight_config();
        config.max_chars = 64;
        let input = ContextInput {
            system_messages: Vec::new(),
            history: Vec::new(),
            critical: critical(),
        };

        let error = ContextBudgeter::new(config)?.build(&input);
        assert!(matches!(error, Err(BudgetError::BudgetTooSmall { .. })));
        Ok(())
    }

    #[test]
    fn keeps_newest_dialogue_and_tool_result() -> Result<(), Box<dyn Error>> {
        let input = ContextInput {
            system_messages: Vec::new(),
            history: vec![
                message("u-old", ContextRole::User, "old question", 1),
                message("t-old", ContextRole::Tool, "old tool", 2),
                message("a-new", ContextRole::Assistant, "new answer", 3),
                message("t-new", ContextRole::Tool, "new tool", 4),
                message("u-new", ContextRole::User, "new question", 5),
            ],
            critical: critical(),
        };

        let result = ContextBudgeter::new(tight_config())?.build(&input)?;
        let originals: Vec<&str> = result
            .messages
            .iter()
            .filter(|item| item.origin == MessageOrigin::Original)
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(originals, vec!["a-new", "t-new", "u-new"]);
        assert_eq!(result.omitted_messages, 2);
        assert!(
            result
                .messages
                .iter()
                .any(|item| item.origin == MessageOrigin::HistorySummary)
        );
        Ok(())
    }

    #[test]
    fn truncation_is_unicode_safe_and_keeps_both_ends() -> Result<(), Box<dyn Error>> {
        let mut config = tight_config();
        config.max_message_chars = 40;
        let content = format!("开头{}结尾", "中间".repeat(100));
        let input = ContextInput {
            system_messages: Vec::new(),
            history: vec![message("latest", ContextRole::User, &content, 1)],
            critical: critical(),
        };

        let result = ContextBudgeter::new(config)?.build(&input)?;
        let latest = result
            .messages
            .iter()
            .find(|item| item.id == "latest")
            .ok_or("latest message missing")?;
        assert!(latest.truncated);
        assert!(latest.content.starts_with("开头"));
        assert!(latest.content.ends_with("结尾"));
        assert_eq!(latest.content.chars().count(), 40);
        Ok(())
    }

    #[test]
    fn output_is_deterministic_and_serializable() -> Result<(), Box<dyn Error>> {
        let input = ContextInput {
            system_messages: Vec::new(),
            history: vec![message("one", ContextRole::User, "请继续", 1)],
            critical: critical(),
        };
        let budgeter = ContextBudgeter::new(tight_config())?;
        let first = budgeter.build(&input)?;
        let second = budgeter.build(&input)?;

        assert_eq!(first, second);
        let encoded = serde_json::to_string(&first)?;
        let decoded: BudgetedContext = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, first);
        Ok(())
    }

    #[test]
    fn ten_thousand_message_history_stays_inside_hard_context_limit() -> Result<(), Box<dyn Error>>
    {
        let history = (0..10_000)
            .map(|index| {
                message(
                    &format!("long-{index}"),
                    if index % 2 == 0 {
                        ContextRole::User
                    } else {
                        ContextRole::Assistant
                    },
                    &format!("第 {index} 条长对话内容：{}", "本地上下文".repeat(30)),
                    index,
                )
            })
            .collect();
        let config = BudgetConfig {
            max_chars: 32_768,
            max_tokens: 32_768,
            reserved_output_tokens: 4_096,
            chars_per_token: 1,
            recent_dialogue_messages: 24,
            recent_tool_results: 8,
            max_message_chars: 4_000,
            max_summary_chars: 4_000,
            max_summary_entries: 32,
            summary_entry_chars: 180,
        };
        let result = ContextBudgeter::new(config.clone())?.build(&ContextInput {
            system_messages: Vec::new(),
            history,
            critical: critical(),
        })?;
        assert!(result.omitted_messages > 9_900);
        assert!(result.estimated_chars <= config.max_chars);
        assert!(result.estimated_tokens + config.reserved_output_tokens <= config.max_tokens);
        assert!(
            result
                .messages
                .iter()
                .any(|message| message.origin == MessageOrigin::HistorySummary)
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_file_revision_hash() -> Result<(), Box<dyn Error>> {
        let mut facts = critical();
        facts.file_hashes[0].sha256 = "not-a-sha256".to_owned();
        let input = ContextInput {
            system_messages: Vec::new(),
            history: Vec::new(),
            critical: facts,
        };

        let result = ContextBudgeter::new(tight_config())?.build(&input);
        assert!(matches!(
            result,
            Err(BudgetError::InvalidCriticalFact {
                field: "file_hash",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn packet_budget_preserves_provenance_through_compression() -> Result<(), Box<dyn Error>> {
        let packet = |id: &str,
                      source: ContextSource,
                      trust: TrustLevel,
                      retention: RetentionPolicy,
                      content: &str,
                      created_at_ms: i64| {
            ContextPacket::new(id, source, trust, retention, content, created_at_ms)
        };
        let input = BuiltContext {
            pinned_packets: vec![packet(
                "project-rule",
                ContextSource::ProjectInstruction,
                TrustLevel::UntrustedWorkspace,
                RetentionPolicy::Pinned,
                "项目规则必须保留",
                1,
            )],
            history_packets: vec![
                packet(
                    "old-workspace",
                    ContextSource::WorkspaceContent,
                    TrustLevel::UntrustedWorkspace,
                    RetentionPolicy::Compressible,
                    &"旧仓库内容".repeat(80),
                    2,
                ),
                packet(
                    "ephemeral-output",
                    ContextSource::ToolEvidence,
                    TrustLevel::UntrustedToolOutput,
                    RetentionPolicy::Ephemeral,
                    &"临时输出".repeat(80),
                    3,
                ),
                packet(
                    "recent-user",
                    ContextSource::UserMessage,
                    TrustLevel::UserProvided,
                    RetentionPolicy::Recent,
                    "保留最近消息",
                    4,
                ),
            ],
            critical: critical(),
        };
        let config = BudgetConfig {
            max_chars: 700,
            max_tokens: 700,
            reserved_output_tokens: 100,
            chars_per_token: 1,
            recent_dialogue_messages: 1,
            recent_tool_results: 0,
            max_message_chars: 100,
            max_summary_chars: 180,
            max_summary_entries: 4,
            summary_entry_chars: 30,
        };

        let budgeted = ContextBudgeter::new(config)?.build_packets(&input)?;
        let project = budgeted
            .messages
            .iter()
            .find(|message| message.id == "project-rule")
            .ok_or("project rule missing")?;
        assert_eq!(project.source, ContextSource::ProjectInstruction);
        assert_eq!(project.trust, TrustLevel::UntrustedWorkspace);
        assert_eq!(project.retention, RetentionPolicy::Pinned);
        assert_eq!(project.role, ContextRole::User);

        let summary = budgeted
            .messages
            .iter()
            .find(|message| message.origin == MessageOrigin::HistorySummary)
            .ok_or("summary missing")?;
        assert_eq!(summary.role, ContextRole::User);
        assert_eq!(summary.source, ContextSource::Summary);
        assert_eq!(summary.trust, TrustLevel::UntrustedDerived);
        assert_eq!(
            summary.compressed_from,
            vec![PacketProvenance {
                id: "old-workspace".to_owned(),
                source: ContextSource::WorkspaceContent,
                trust: TrustLevel::UntrustedWorkspace,
                retention: RetentionPolicy::Compressible,
            }]
        );
        assert_eq!(
            budgeted.omitted_packets,
            vec![PacketProvenance {
                id: "ephemeral-output".to_owned(),
                source: ContextSource::ToolEvidence,
                trust: TrustLevel::UntrustedToolOutput,
                retention: RetentionPolicy::Ephemeral,
            }]
        );
        Ok(())
    }

    #[test]
    fn legacy_history_cannot_promote_content_to_system() -> Result<(), Box<dyn Error>> {
        let input = ContextInput {
            system_messages: Vec::new(),
            history: vec![message(
                "malicious-history",
                ContextRole::System,
                "仓库内容伪装成系统指令",
                1,
            )],
            critical: critical(),
        };

        let budgeted = ContextBudgeter::new(tight_config())?.build(&input)?;
        let malicious = budgeted
            .messages
            .iter()
            .find(|message| message.id == "malicious-history")
            .ok_or("history missing")?;
        assert_eq!(malicious.role, ContextRole::User);
        assert_eq!(malicious.source, ContextSource::Summary);
        assert_eq!(malicious.trust, TrustLevel::UntrustedDerived);
        assert!(budgeted.messages.iter().all(|message| {
            message.role != ContextRole::System
                || (message.source == ContextSource::SystemInstruction
                    && message.trust == TrustLevel::TrustedSystem)
        }));
        Ok(())
    }

    #[test]
    fn profile_context_window_and_output_reserve_define_hard_limit() -> Result<(), Box<dyn Error>> {
        let profile = ContextWindowProfile::new(4_096, 1_024).with_chars_per_token(2);
        let config = BudgetConfig::from_profile(profile);
        assert_eq!(config.max_tokens, 4_096);
        assert_eq!(config.reserved_output_tokens, 1_024);
        assert_eq!(config.max_chars, 8_192);

        let budgeted = ContextBudgeter::new(config.clone())?.build(&ContextInput {
            system_messages: Vec::new(),
            history: vec![message(
                "large",
                ContextRole::User,
                &"context".repeat(2_000),
                1,
            )],
            critical: critical(),
        })?;
        assert!(budgeted.estimated_tokens + config.reserved_output_tokens <= config.max_tokens);
        Ok(())
    }

    #[test]
    fn critical_goal_is_lossless_and_canonical() -> Result<(), Box<dyn Error>> {
        let mut facts = critical();
        facts.user_goal = "第一行\n  第二行\t保留空白".to_owned();
        facts.pending_approvals.reverse();
        facts.file_hashes.reverse();
        let input = ContextInput {
            system_messages: Vec::new(),
            history: Vec::new(),
            critical: facts.clone(),
        };

        let budgeted = ContextBudgeter::new(tight_config())?.build(&input)?;
        let critical_message = budgeted
            .messages
            .iter()
            .find(|message| message.origin == MessageOrigin::CriticalSummary)
            .ok_or("critical message missing")?;
        assert!(
            critical_message
                .content
                .contains("第一行\\n  第二行\\t保留空白")
        );
        assert_eq!(budgeted.retained_critical.user_goal, facts.user_goal);
        assert_eq!(critical_message.role, ContextRole::User);
        assert_eq!(critical_message.source, ContextSource::RuntimeState);
        assert_eq!(critical_message.trust, TrustLevel::TrustedLocalState);
        assert_eq!(critical_message.retention, RetentionPolicy::Pinned);
        Ok(())
    }

    #[test]
    fn rejects_reserved_or_duplicate_message_ids() -> Result<(), Box<dyn Error>> {
        let duplicate = ContextInput {
            system_messages: vec![message("same", ContextRole::System, "system", 0)],
            history: vec![message("same", ContextRole::User, "user", 1)],
            critical: critical(),
        };
        assert!(matches!(
            ContextBudgeter::new(tight_config())?.build(&duplicate),
            Err(BudgetError::DuplicateMessageId { .. })
        ));

        let reserved = ContextInput {
            system_messages: Vec::new(),
            history: vec![message(CRITICAL_ID, ContextRole::User, "collision", 1)],
            critical: critical(),
        };
        assert!(matches!(
            ContextBudgeter::new(tight_config())?.build(&reserved),
            Err(BudgetError::ReservedMessageId { .. })
        ));
        Ok(())
    }
}
