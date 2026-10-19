//! The view: what a subscriber shows, as entries keyed by [`EntryId`] and sorted by `order`.
//!
//! The reducer (R6) folds a stream's events into entries; the host sends them as view patches
//! (`patch.rs`). Each entry starts with `order`, the `seq` of the event that created it, and
//! holds the domain id its `EntryId` is built from, never a separate id field: a client keys an
//! entry as `<variant key>:<that id>`, which [`ViewEntry::id`] computes.

use serde::{Deserialize, Serialize};

use crate::open_enum::enum_with_all;
use crate::{ContentBlock, Cost, EntryId, Seq, TextOrBlob, ThreadId, Timestamp, ToolCallContent};

mod entry;

pub use entry::{
    AgentContentEntry, AssistantMessageEntry, ElicitationEntry, ElicitationState, HeaderEntry,
    LimitStopEntry, MarkerEntry, MarkerKind, MatterHeaderEntry, NoticeEntry, PermissionEntry,
    PermissionState, StepEntry, ToolCallEntry, UserMessageEntry, UserMessageStatus, VersionEntry,
};

enum_with_all! {
    /// Where a thread stands, for the sidebar dot and the header. `NeedsYou` means a permission or
    /// elicitation is pending. Closed: a new status bumps `schema_version`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "specta", derive(specta::Type))]
    pub enum ThreadStatus {
        /// Nothing runs.
        Idle,
        /// A prompt was sent; the agent is starting or the turn has not started yet.
        Starting,
        /// A turn runs (queued prompts of the same thread wait behind it).
        Running,
        /// A permission or elicitation waits for the lawyer.
        NeedsYou,
        /// The turn waits for another thread of the matter to finish writing.
        Queued {
            /// The thread it waits for.
            behind: ThreadId = ThreadId::NIL,
        },
        /// The last turn stopped at the account's usage limit.
        PausedAtLimit {
            /// When the limit resets, if known; the UI compares it with the time.
            #[serde(skip_serializing_if = "Option::is_none", default)]
            resets_at: Option<Timestamp> = None,
        },
        /// The agent process exited (or detached for a reason this version does not know).
        AgentExited,
        /// The agent was stopped for being idle; the next prompt starts it again.
        IdleStopped,
        /// The thread is archived.
        Archived,
    }
}

/// The context window and cost the agent last reported, for the header.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ContextUsage {
    /// Tokens in the context, at most [`MAX_SAFE`](crate::MAX_SAFE).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    #[serde(with = "crate::number::safe_opt")]
    pub context_used: Option<u64>,
    /// The context window's size in tokens, at most [`MAX_SAFE`](crate::MAX_SAFE).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    #[serde(with = "crate::number::safe_opt")]
    pub context_size: Option<u64>,
    /// The session's cost so far.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cost: Option<Cost>,
}

/// One entry of a stream's view. JSON: `{"tool_call": {...}}`. A thread's stream has one
/// `Header` and transcript entries; a matter's stream has one `MatterHeader` and one `Version`
/// per checkpoint (the Versions timeline).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ViewEntry {
    /// The thread's header: title, agent, status, mode, options, commands, plan, usage.
    Header(HeaderEntry),
    /// A prompt the lawyer sent.
    UserMessage(UserMessageEntry),
    /// An answer or a thought, as one text.
    AssistantMessage(AssistantMessageEntry),
    /// One non-text block of an answer or a thought.
    AgentContent(AgentContentEntry),
    /// A tool call.
    ToolCall(Box<ToolCallEntry>),
    /// A permission request.
    Permission(PermissionEntry),
    /// An elicitation.
    Elicitation(ElicitationEntry),
    /// One per ended turn that started: the step card.
    Step(StepEntry),
    /// A turn stopped at the usage limit: the at-limit card.
    LimitStop(LimitStopEntry),
    /// A notice from the agent or the host.
    Notice(NoticeEntry),
    /// An undo, a restore or a compaction.
    Marker(MarkerEntry),
    /// The matter's header.
    MatterHeader(MatterHeaderEntry),
    /// One version of the matter's files.
    Version(VersionEntry),
}

impl ViewEntry {
    /// The entry's id, built from the domain id it holds.
    pub fn id(&self) -> EntryId {
        match self {
            Self::Header(e) => EntryId::Header(e.thread),
            Self::UserMessage(e) => EntryId::UserMessage(e.message),
            Self::AssistantMessage(e) => EntryId::AssistantMessage(e.message),
            Self::AgentContent(e) => EntryId::AgentContent(e.event),
            Self::ToolCall(e) => EntryId::ToolCall(e.tool_call),
            Self::Permission(e) => EntryId::Permission(e.request),
            Self::Elicitation(e) => EntryId::Elicitation(e.elicitation),
            Self::Step(e) => EntryId::Step(e.turn),
            Self::LimitStop(e) => EntryId::LimitStop(e.turn),
            Self::Notice(e) => EntryId::Notice(e.event),
            Self::Marker(e) => EntryId::Marker(e.event),
            Self::MatterHeader(e) => EntryId::MatterHeader(e.matter),
            Self::Version(e) => EntryId::Version(e.checkpoint),
        }
    }

    /// The entry's place: the `seq` of the event that created it. Entries sort by it.
    pub fn order(&self) -> Seq {
        match self {
            Self::Header(e) => e.order,
            Self::UserMessage(e) => e.order,
            Self::AssistantMessage(e) => e.order,
            Self::AgentContent(e) => e.order,
            Self::ToolCall(e) => e.order,
            Self::Permission(e) => e.order,
            Self::Elicitation(e) => e.order,
            Self::Step(e) => e.order,
            Self::LimitStop(e) => e.order,
            Self::Notice(e) => e.order,
            Self::Marker(e) => e.order,
            Self::MatterHeader(e) => e.order,
            Self::Version(e) => e.order,
        }
    }

    /// The entry's inline cost, the number the host keeps within
    /// [`INLINE_LIMIT_BYTES`](crate::INLINE_LIMIT_BYTES): UTF-8 bytes after JSON escaping,
    /// without the quotes, summed over the entry's `Inline` values, `Vec` items included.
    ///
    /// Only `TextOrBlob` fields count, each through [`TextOrBlob::inline_len`] (a `Blob` costs
    /// 0), taken in this order, each item's fields in declaration order: for a `ToolCall`,
    /// `raw_input`, `raw_output`, then `content` items; for a `UserMessage`, `content` items; for
    /// an `AgentContent`, its block; for a `Marker`, `summary`. Every other entry costs 0. Message
    /// text (`ContentBlock::Text`, an assistant message's `text`) is a `String`, not a
    /// `TextOrBlob`, and is not counted here: the reducer splits it into text patches.
    pub fn inline_len(&self) -> usize {
        match self {
            Self::ToolCall(e) => {
                optional_len(e.raw_input.as_ref())
                    + optional_len(e.raw_output.as_ref())
                    + e.content.iter().map(tool_content_len).sum::<usize>()
            }
            Self::UserMessage(e) => e.content.iter().map(block_len).sum(),
            Self::AgentContent(e) => block_len(&e.block),
            Self::Marker(e) => optional_len(e.summary.as_ref()),
            Self::Header(_)
            | Self::AssistantMessage(_)
            | Self::Permission(_)
            | Self::Elicitation(_)
            | Self::Step(_)
            | Self::LimitStop(_)
            | Self::Notice(_)
            | Self::MatterHeader(_)
            | Self::Version(_) => 0,
        }
    }
}

fn optional_len(value: Option<&TextOrBlob>) -> usize {
    value.map_or(0, TextOrBlob::inline_len)
}

/// A content block's `TextOrBlob` fields, in declaration order.
fn block_len(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Image { data, .. } | ContentBlock::Audio { data, .. } => data.inline_len(),
        ContentBlock::Resource { text, blob, .. } => {
            optional_len(text.as_ref()) + optional_len(blob.as_ref())
        }
        ContentBlock::Unsupported { raw, .. } => optional_len(raw.as_ref()),
        ContentBlock::Text { .. } | ContentBlock::ResourceLink { .. } => 0,
    }
}

/// A tool call content item's `TextOrBlob` fields, in declaration order.
fn tool_content_len(content: &ToolCallContent) -> usize {
    match content {
        ToolCallContent::Text(text) => text.inline_len(),
        ToolCallContent::Content(block) => block_len(block),
        ToolCallContent::Diff { old, new, .. } => optional_len(old.as_ref()) + new.inline_len(),
    }
}
