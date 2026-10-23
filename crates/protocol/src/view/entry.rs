//! The entry structs of [`ViewEntry`](super::ViewEntry). Each starts with `order` and holds the
//! domain id its `EntryId` is built from.

use serde::{Deserialize, Serialize};

use super::{ContextUsage, ThreadStatus};
use crate::{
    AgentId, AvailableCommand, CheckpointId, CheckpointKind, CompactString, CompactionStatus,
    ConfigOption, ContentBlock, Deliverable, Delivery, ElicitationId, ElicitationOutcome,
    ElicitationRequest, ErrorCode, EventId, ExpiryReason, FileChange, ImportSummary,
    LimitStopReason, MatterId, MatterKind, MatterPath, MessageId, NoticeLevel, PermissionChoice,
    PermissionOption, PermissionRequestId, PlanEntry, Principal, RestoreScope, Seq, SkippedFile,
    SteerOutcome, StopReason, TextKind, TextOrBlob, ThreadId, Timestamp, ToolCallContent,
    ToolCallId, ToolCallStatus, ToolKind, ToolLocation, TurnId, TurnUsage,
};

/// A thread's header. Entry id `header:<thread>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HeaderEntry {
    /// The `seq` of `ThreadCreated`.
    pub order: Seq,
    /// The thread.
    pub thread: ThreadId,
    /// Its matter.
    pub matter: MatterId,
    /// Its title, once it has one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub title: Option<CompactString>,
    /// Its agent.
    pub agent: AgentId,
    /// Where it stands.
    pub status: ThreadStatus,
    /// Whether the thread's queue is held: its queued turns wait for `ResumeQueue`. Filled by
    /// the host; also true after a restart, when every turn that never started comes back held.
    /// Missing reads as `false`, so a header written before the field still decodes.
    #[serde(default)]
    pub queue_held: bool,
    /// The agent's current mode, if it reported one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub mode_id: Option<CompactString>,
    /// The agent's config options (mode, model, thought level, others).
    pub config_options: Vec<ConfigOption>,
    /// The slash commands the agent offers.
    pub commands: Vec<AvailableCommand>,
    /// The agent's current plan.
    pub plan: Vec<PlanEntry>,
    /// Context and cost, once reported.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage: Option<ContextUsage>,
}

/// Where a prompt stands in its thread's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum UserMessageStatus {
    /// Sent: its turn started or is about to.
    Sent,
    /// Waiting in the thread's queue.
    Queued,
    /// Removed from the queue before its turn started (its turn ended cancelled).
    Removed,
}

/// A prompt the lawyer sent. Entry id `user_message:<message>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UserMessageEntry {
    /// The `seq` of `UserMessage`.
    pub order: Seq,
    /// The message.
    pub message: MessageId,
    /// The turn that answers it (after a steer, the outcome's turn).
    pub turn: TurnId,
    /// What the lawyer sent (the edited content, if edited while queued). The `Text` blocks
    /// arrive cut to at most [`INLINE_LIMIT_BYTES`](crate::INLINE_LIMIT_BYTES) in all, summed
    /// over the blocks, each counted as `escaped_len` counts it. Blocks are taken in order, so a
    /// block past the limit arrives empty. `AppendText` patches with `AppendTarget::UserBlock`
    /// and its index add the rest of each cut block.
    pub content: Vec<ContentBlock>,
    /// How it was asked to be delivered. Queue is written as a missing field, and a missing
    /// field reads as queue.
    #[serde(default, skip_serializing_if = "Delivery::is_queue")]
    pub delivery: Delivery,
    /// Where it stands in the queue.
    pub status: UserMessageStatus,
    /// Whether it was edited while queued.
    pub edited: bool,
    /// Where a steered message went, once resolved.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub steer: Option<SteerOutcome>,
}

/// An answer or a thought, as one text. Entry id `agent_text:<message>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentTextEntry {
    /// The `seq` of the message's first text.
    pub order: Seq,
    /// The message.
    pub message: MessageId,
    /// Its turn.
    pub turn: TurnId,
    /// Answer or thought, from its first text.
    pub kind: TextKind,
    /// The text so far; `AppendText` patches add to it.
    pub text: String,
}

/// One non-text block of an answer or a thought. Entry id `agent_content:<event>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentContentEntry {
    /// The `seq` of its `ContentAppended`.
    pub order: Seq,
    /// The id of its `ContentAppended` event.
    pub event: EventId,
    /// Its turn.
    pub turn: TurnId,
    /// The message it belongs to.
    pub message: MessageId,
    /// Answer or thought.
    pub kind: TextKind,
    /// The block (never `Text`).
    pub block: ContentBlock,
}

/// A tool call: `ToolCallStarted`'s fields after `acp_id`, with every update applied. Entry id
/// `tool_call:<tool_call>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ToolCallEntry {
    /// The `seq` of `ToolCallStarted`.
    pub order: Seq,
    /// The tool call.
    pub tool_call: ToolCallId,
    /// Its turn.
    pub turn: TurnId,
    /// A title to show.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub title: CompactString,
    /// What it does.
    pub kind: ToolKind,
    /// The programmatic tool name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub tool_name: Option<CompactString>,
    /// Where it stands.
    pub status: ToolCallStatus,
    /// The places it touches.
    pub locations: Vec<ToolLocation>,
    /// Its input, as compact JSON text.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_input: Option<TextOrBlob>,
    /// Its output, as compact JSON text.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_output: Option<TextOrBlob>,
    /// What it shows.
    pub content: Vec<ToolCallContent>,
}

/// Where a permission request or an elicitation stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PermissionState {
    /// Waiting for an answer.
    Pending,
    /// Answered or cancelled.
    Resolved {
        /// The answer.
        choice: PermissionChoice,
        /// Who answered.
        by: Principal,
    },
    /// Expired unanswered.
    Expired {
        /// Why.
        reason: ExpiryReason,
    },
}

/// A permission request. Entry id `permission:<request>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PermissionEntry {
    /// The `seq` of `PermissionRequested`.
    pub order: Seq,
    /// The request.
    pub request: PermissionRequestId,
    /// The tool call it is about.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call: Option<ToolCallId>,
    /// The offered options.
    pub options: Vec<PermissionOption>,
    /// Whether an answer can still reach the agent.
    pub deliverable: Deliverable,
    /// Where it stands.
    pub state: PermissionState,
}

/// Where an elicitation stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ElicitationState {
    /// Waiting for an answer.
    Pending,
    /// Answered (the values are never kept).
    Resolved {
        /// How it ended.
        outcome: ElicitationOutcome,
        /// Who answered.
        by: Principal,
    },
    /// Expired unanswered.
    Expired {
        /// Why.
        reason: ExpiryReason,
    },
}

/// An elicitation. Entry id `elicitation:<elicitation>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ElicitationEntry {
    /// The `seq` of `ElicitationRequested`.
    pub order: Seq,
    /// The elicitation.
    pub elicitation: ElicitationId,
    /// What it asks for.
    pub request: ElicitationRequest,
    /// Where it stands.
    pub state: ElicitationState,
}

/// The step card of an ended turn that started; the reducer writes none for a turn that ends
/// without starting (a queued prompt removed or cancelled). Entry id `step:<turn>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct StepEntry {
    /// The `seq` of `TurnEnded`.
    pub order: Seq,
    /// The turn.
    pub turn: TurnId,
    /// Why it stopped.
    pub stop: StopReason,
    /// The code of the `TurnFailed` before it, when it failed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub failure: Option<ErrorCode>,
    /// The files it changed (from its `FilesChanged`; empty when none changed).
    pub files: Vec<FileChange>,
    /// The files it changed but did not version.
    pub skipped: Vec<SkippedFile>,
    /// The after-turn checkpoint; `None` when it failed, and the step cannot be undone.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub checkpoint_after: Option<CheckpointId>,
    /// Whether the step was undone.
    pub undone: bool,
    /// The turn's token usage, if the agent reported it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage: Option<TurnUsage>,
}

impl StepEntry {
    /// Whether the step itself can be undone: `checkpoint_after` exists and the step is not
    /// undone. The UI derives it with the same rule and does not receive it. It does not change
    /// while turns run: the UI also offers undo only while the header's `status` is not
    /// `Starting`, `Running`, `NeedsYou` or `Queued`, and other threads of the matter are the
    /// host's check at command time.
    pub fn undo_possible(&self) -> bool {
        self.checkpoint_after.is_some() && !self.undone
    }
}

/// A turn stopped at the usage limit: the at-limit card. Entry id `limit_stop:<turn>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LimitStopEntry {
    /// The `seq` of `TurnStoppedAtLimit`.
    pub order: Seq,
    /// The turn.
    pub turn: TurnId,
    /// The limit window's id, if known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub window: Option<String>,
    /// When it resets, if known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resets_at: Option<Timestamp>,
    /// Which limit.
    pub reason: LimitStopReason,
}

/// A notice. Entry id `notice:<event>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NoticeEntry {
    /// The `seq` of `Notice`.
    pub order: Seq,
    /// The id of its `Notice` event.
    pub event: EventId,
    /// How loud.
    pub level: NoticeLevel,
    /// The text.
    pub text: String,
}

/// What a marker records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum MarkerKind {
    /// A step was undone (thread stream).
    StepUndone {
        /// The step's turn.
        turn: TurnId,
        /// The checkpoint the undo wrote.
        checkpoint: CheckpointId,
        /// Files put back.
        restored: Vec<MatterPath>,
        /// Files kept as they were.
        kept: Vec<MatterPath>,
    },
    /// A version was restored (matter stream).
    VersionRestored {
        /// The checkpoint restored from.
        from: CheckpointId,
        /// The checkpoint the restore wrote.
        checkpoint: CheckpointId,
        /// What was put back.
        scope: RestoreScope,
    },
    /// The agent compacted its context (thread stream).
    Compaction {
        /// Started or completed.
        status: CompactionStatus,
        /// The summary the agent kept, when it sent one.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        summary: Option<TextOrBlob>,
    },
}

/// An undo, a restore or a compaction. Entry id `marker:<event>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MarkerEntry {
    /// The `seq` of the event it records.
    pub order: Seq,
    /// That event's id.
    pub event: EventId,
    /// What it records.
    pub kind: MarkerKind,
}

/// A matter's header. Entry id `matter_header:<matter>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MatterHeaderEntry {
    /// The `seq` of `MatterCreated`.
    pub order: Seq,
    /// The matter.
    pub matter: MatterId,
    /// Its name.
    pub name: String,
    /// How it was made.
    pub kind: MatterKind,
    /// What the import found, for a folder matter.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub import: Option<ImportSummary>,
}

/// One version of the matter's files, in the Versions timeline. Entry id `version:<checkpoint>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VersionEntry {
    /// The `seq` of `CheckpointRecorded`.
    pub order: Seq,
    /// The checkpoint.
    pub checkpoint: CheckpointId,
    /// Why it was taken.
    pub kind: CheckpointKind,
    /// The thread whose turn or undo took it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thread: Option<ThreadId>,
    /// The turn it was taken around.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub turn: Option<TurnId>,
    /// When it was recorded (display only; the envelope's `ts`).
    pub ts: Timestamp,
    /// For an outside-edits checkpoint, the files changed outside Clauseo.
    pub files: Vec<FileChange>,
}
