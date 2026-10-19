//! Payload structs of the thread-level domain events. Each is the payload of the
//! `DomainEvent` variant of the same name.

use serde::{Deserialize, Serialize};

use crate::{
    AgentCapabilities, AgentId, AgentSessionId, AvailableCommand, CheckpointId, CompactString,
    CompactionStatus, ConfigOption, ContentBlock, Cost, Deliverable, Delivery, DetachReason,
    ElicitationId, ElicitationOutcome, ElicitationRequest, ErrorCode, ExpiryReason, FileChange,
    MatterId, MatterPath, MessageId, NoticeLevel, PermissionChoice, PermissionOption,
    PermissionRequestId, PlanEntry, Principal, RenameSource, SkippedFile, SteerOutcome, StopReason,
    TextKind, TextOrBlob, ThreadId, ToolCallId, ToolCallPatch, TurnId, TurnUsage,
};

/// A thread was created in a matter: the first event of its stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadCreated {
    /// The matter it belongs to.
    pub matter: MatterId,
    /// The agent, fixed for the thread's life.
    pub agent: AgentId,
    /// A title, when one was given.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub title: Option<CompactString>,
}

/// The thread got a new title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadRenamed {
    /// The new title.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub title: CompactString,
    /// Who renamed it.
    pub by: RenameSource,
}

/// The thread was archived. JSON: `{"thread_archived": {}}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadArchived {}

/// An agent session started or was loaded for the thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentSessionAttached {
    /// ACP's session id, a foreign attribute.
    pub session: AgentSessionId,
    /// The agent's version, when it reported one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_version: Option<String>,
    /// What the agent can do.
    pub capabilities: AgentCapabilities,
}

/// The thread's agent session ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentSessionDetached {
    /// Why.
    pub reason: DetachReason,
}

/// The lawyer's message, logged before its turn is queued or started. `session/load` replays
/// of `user_message_chunk` map to it too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UserMessage {
    /// The turn that answers it.
    pub turn: TurnId,
    /// The message.
    pub message: MessageId,
    /// ACP's `messageId`, when the agent gave one; a foreign attribute.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub acp_message_id: Option<CompactString>,
    /// What it says.
    pub content: Vec<ContentBlock>,
    /// How the lawyer asked it to be delivered; `None` is `Queue`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub delivery: Option<Delivery>,
}

/// A turn waits: behind its own thread's running turn, or behind another thread's writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnQueued {
    /// The waiting turn.
    pub turn: TurnId,
    /// The thread it waits for: this thread when `behind_turn` is set, or when start-up
    /// reconciliation holds a first turn.
    pub behind: ThreadId,
    /// The same thread's turn it waits for.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub behind_turn: Option<TurnId>,
}

/// A queued prompt's content was replaced; the turn keeps its id and its place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct QueuedPromptEdited {
    /// The queued turn.
    pub turn: TurnId,
    /// Its message.
    pub message: MessageId,
    /// The new content.
    pub content: Vec<ContentBlock>,
}

/// Where a steered message went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SteerResolved {
    /// The steered message.
    pub message: MessageId,
    /// Where it went.
    pub outcome: SteerOutcome,
}

/// A turn started, after its before-turn checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnStarted {
    /// The turn.
    pub turn: TurnId,
    /// The checkpoint taken before it; a turn never runs without one.
    pub checkpoint_before: CheckpointId,
}

/// Text appended to an agent's answer or thought.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TextAppended {
    /// The turn.
    pub turn: TurnId,
    /// The agent's message.
    pub message: MessageId,
    /// ACP's `messageId`, when the agent gave one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub acp_message_id: Option<CompactString>,
    /// Answer or thought.
    pub kind: TextKind,
    /// The text.
    pub text: String,
}

/// A non-text block in an agent's answer or thought (never `ContentBlock::Text`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ContentAppended {
    /// The turn.
    pub turn: TurnId,
    /// The agent's message.
    pub message: MessageId,
    /// ACP's `messageId`, when the agent gave one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub acp_message_id: Option<CompactString>,
    /// Answer or thought.
    pub kind: TextKind,
    /// The block.
    pub block: ContentBlock,
}

/// A tool call changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ToolCallUpdated {
    /// The tool call.
    pub tool_call: ToolCallId,
    /// What changed.
    pub patch: ToolCallPatch,
}

/// The agent asked for permission; a durable record any client may answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PermissionRequested {
    /// The request.
    pub request: PermissionRequestId,
    /// The tool call it is about, when there is one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call: Option<ToolCallId>,
    /// The options offered.
    pub options: Vec<PermissionOption>,
    /// Whether an answer can reach the agent.
    pub deliverable: Deliverable,
}

/// A permission request was answered; the first valid answer wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PermissionResolved {
    /// The request.
    pub request: PermissionRequestId,
    /// The answer.
    pub choice: PermissionChoice,
    /// Who answered.
    pub by: Principal,
}

/// A permission request expired unanswered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PermissionExpired {
    /// The request.
    pub request: PermissionRequestId,
    /// Why.
    pub reason: ExpiryReason,
}

/// The agent asked the lawyer for input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ElicitationRequested {
    /// The elicitation.
    pub elicitation: ElicitationId,
    /// What it asks for.
    pub request: ElicitationRequest,
}

/// An elicitation was answered. The values are not logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ElicitationResolved {
    /// The elicitation.
    pub elicitation: ElicitationId,
    /// How it ended.
    pub outcome: ElicitationOutcome,
    /// Who answered.
    pub by: Principal,
}

/// An elicitation expired unanswered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ElicitationExpired {
    /// The elicitation.
    pub elicitation: ElicitationId,
    /// Why.
    pub reason: ExpiryReason,
}

/// The agent's plan, replaced whole (ACP v1 sends the full plan each time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PlanReplaced {
    /// The entries.
    pub entries: Vec<PlanEntry>,
}

/// The session's mode changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ModeChanged {
    /// The new mode's id.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub mode_id: CompactString,
}

/// The session's config options, replaced whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConfigOptionsReplaced {
    /// The options.
    pub options: Vec<ConfigOption>,
}

/// The agent's slash commands, replaced whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CommandsAvailable {
    /// The commands.
    pub commands: Vec<AvailableCommand>,
}

/// The agent reported its context use and cost. Token counts are at most
/// [`MAX_SAFE`](crate::MAX_SAFE).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsageUpdated {
    /// Tokens in the context now.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    #[serde(with = "crate::number::safe_opt")]
    pub context_used: Option<u64>,
    /// The context's size in tokens.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    #[serde(with = "crate::number::safe_opt")]
    pub context_size: Option<u64>,
    /// The session's cost so far, shown only to pay-as-you-go accounts.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cost: Option<Cost>,
}

/// A notice for the lawyer, from the agent or the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Notice {
    /// How prominent it is.
    pub level: NoticeLevel,
    /// What it says.
    pub text: String,
}

/// Context compaction started or finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CompactionChanged {
    /// Where it stands.
    pub status: CompactionStatus,
    /// The summary the agent kept, when it sent one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<TextOrBlob>,
}

/// The files a turn changed. When present it comes before the turn's `TurnEnded`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FilesChanged {
    /// The turn.
    pub turn: TurnId,
    /// The files it changed.
    pub files: Vec<FileChange>,
    /// The files it changed that were not versioned.
    pub skipped: Vec<SkippedFile>,
}

/// Why a turn failed. Detail only: the turn's `TurnEnded` still follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnFailed {
    /// The turn.
    pub turn: TurnId,
    /// The failure's code; the UI picks copy by it.
    pub code: ErrorCode,
    /// More detail for logs, at most 4 KiB, redacted, never shown.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
}

/// A turn ended: exactly one per turn, including errors, cancels and a queued turn cancelled
/// before it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnEnded {
    /// The turn.
    pub turn: TurnId,
    /// Why it stopped.
    pub stop: StopReason,
    /// Its token counts, when the agent reported them.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage: Option<TurnUsage>,
    /// The checkpoint taken after it; `None` when that failed, which is shown, never silent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub checkpoint_after: Option<CheckpointId>,
}

/// A step (a turn's changes) was undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct StepUndone {
    /// The turn whose changes were undone.
    pub turn: TurnId,
    /// The checkpoint the undo wrote.
    pub checkpoint: CheckpointId,
    /// The files put back.
    pub restored: Vec<MatterPath>,
    /// The files the lawyer chose to keep.
    pub kept: Vec<MatterPath>,
}
