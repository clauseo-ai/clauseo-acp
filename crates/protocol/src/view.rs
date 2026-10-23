//! The view: what a subscriber shows, as entries keyed by [`EntryId`] and sorted by `order`.
//!
//! The reducer folds a stream's events into entries; the host sends them as view patches
//! (`patch.rs`). Each entry starts with `order`, the `seq` of the event that created it, and
//! holds the domain id its `EntryId` is built from, never a separate id field: a client keys an
//! entry as `<variant key>:<that id>`, which [`ViewEntry::id`] computes.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::open_enum::enum_with_names;
use crate::stream::{split_kind, unknown_prefix};
use crate::{
    CheckpointId, ContentBlock, Cost, Count, ElicitationId, Error, EventId, MatterId, MessageId,
    PermissionRequestId, Seq, TextOrBlob, ThreadId, Timestamp, ToolCallContent, ToolCallId, TurnId,
};

mod entry;

pub use entry::{
    AgentContentEntry, AgentTextEntry, ElicitationEntry, ElicitationState, HeaderEntry,
    LimitStopEntry, MarkerEntry, MarkerKind, MatterHeaderEntry, NoticeEntry, PermissionEntry,
    PermissionState, StepEntry, ToolCallEntry, UserMessageEntry, UserMessageStatus, VersionEntry,
};

enum_with_names! {
    /// Where a thread stands, for the sidebar dot and the header. `NeedsYou` means a permission or
    /// elicitation is pending. Not logged: a new status is additive under 0002's freeze rules,
    /// since the desktop and the cli ship with the host.
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
            behind: ThreadId,
        },
        /// The last turn stopped at the account's usage limit.
        PausedAtLimit {
            /// When the limit resets, if known; the UI compares it with the time.
            #[serde(skip_serializing_if = "Option::is_none", default)]
            resets_at: Option<Timestamp>,
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
    pub context_used: Option<Count>,
    /// The context window's size in tokens, at most [`MAX_SAFE`](crate::MAX_SAFE).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub context_size: Option<Count>,
    /// The session's cost so far.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cost: Option<Cost>,
}

/// Defines `ViewEntry` and `EntryId` from one table, with `ViewEntry::id` and
/// `ViewEntry::order`. A row `Variant(Entry, id_field: IdType) = "key"` makes the variant
/// `ViewEntry::Variant(Entry)`, whose serde key is `key`, and `EntryId::Variant(IdType)`, whose
/// text form is `key:<uuid>`, built from the entry's `id_field`. So the key a client reads and the
/// `EntryId` a `Remove` names cannot disagree.
macro_rules! view_entries {
    (
        $(#[doc = $enum_doc:expr])*
        pub enum ViewEntry {$(
            $(#[doc = $doc:expr])*
            $variant:ident($entry:ty, $id_field:ident: $id:ident) = $key:literal,
        )+}
    ) => {
        $(#[doc = $enum_doc])*
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        pub enum ViewEntry {
            $(
                $(#[doc = $doc])*
                #[serde(rename = $key)]
                $variant($entry),
            )+
        }

        /// The key of a view entry: the entry's kind and the domain id it is built from.
        ///
        /// Text form: `<snake_case kind>:<uuid>`, for example `tool_call:<uuid>`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
        pub enum EntryId {
            $(
                $(#[doc = $doc])*
                $variant($id),
            )+
        }

        impl EntryId {
            /// Every kind's text, in declaration order.
            pub const KINDS: &'static [&'static str] = &[$($key),+];

            /// The text before the `:`, such as `"tool_call"`.
            pub const fn kind(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => $key,)+
                }
            }

            /// The domain id's UUID.
            pub const fn uuid(&self) -> uuid::Uuid {
                match self {
                    $(Self::$variant(id) => id.as_uuid(),)+
                }
            }
        }

        impl FromStr for EntryId {
            type Err = Error;

            fn from_str(text: &str) -> Result<Self, Error> {
                let (prefix, id) = split_kind(text, "EntryId")?;
                match prefix {
                    $($key => id.parse().map(Self::$variant),)+
                    other => Err(unknown_prefix("EntryId", other)),
                }
            }
        }

        impl ViewEntry {
            /// The entry's id, built from the domain id it holds.
            pub fn id(&self) -> EntryId {
                match self {
                    $(Self::$variant(e) => EntryId::$variant(e.$id_field),)+
                }
            }

            /// The entry's place: the `seq` of the event that created it. Entries sort by it.
            pub fn order(&self) -> Seq {
                match self {
                    $(Self::$variant(e) => e.order,)+
                }
            }
        }
    };
}

view_entries! {
    /// One entry of a stream's view. JSON: `{"tool_call": {...}}`. A thread's stream has one
    /// `Header` and transcript entries; a matter's stream has one `MatterHeader` and one `Version`
    /// per checkpoint (the Versions timeline).
    pub enum ViewEntry {
        /// The thread's header: title, agent, status, mode, options, commands, plan, usage.
        Header(HeaderEntry, thread: ThreadId) = "header",
        /// A prompt the lawyer sent.
        UserMessage(UserMessageEntry, message: MessageId) = "user_message",
        /// An answer or a thought, as one text.
        AgentText(AgentTextEntry, message: MessageId) = "agent_text",
        /// One non-text block of an answer or a thought.
        AgentContent(AgentContentEntry, event: EventId) = "agent_content",
        /// A tool call.
        ToolCall(Box<ToolCallEntry>, tool_call: ToolCallId) = "tool_call",
        /// A permission request.
        Permission(PermissionEntry, request: PermissionRequestId) = "permission",
        /// An elicitation.
        Elicitation(ElicitationEntry, elicitation: ElicitationId) = "elicitation",
        /// The step card: one per ended turn that started. A turn that ends without starting
        /// (removed from the queue) gets none.
        Step(StepEntry, turn: TurnId) = "step",
        /// A turn stopped at the usage limit: the at-limit card.
        LimitStop(LimitStopEntry, turn: TurnId) = "limit_stop",
        /// A notice from the agent or the host.
        Notice(NoticeEntry, event: EventId) = "notice",
        /// An undo, a restore or a compaction.
        Marker(MarkerEntry, event: EventId) = "marker",
        /// The matter's header.
        MatterHeader(MatterHeaderEntry, matter: MatterId) = "matter_header",
        /// One version of the matter's files.
        Version(VersionEntry, checkpoint: CheckpointId) = "version",
    }
}

impl ViewEntry {
    /// The entry's inline cost, the number the host keeps within
    /// [`INLINE_LIMIT_BYTES`](crate::INLINE_LIMIT_BYTES): UTF-8 bytes after JSON escaping,
    /// without the quotes, summed over the entry's `Inline` values, `Vec` items included.
    ///
    /// Only `TextOrBlob` fields count, each through [`TextOrBlob::inline_len`] (a `Blob` costs
    /// 0), taken in this order, each item's fields in declaration order: for a `ToolCall`,
    /// `raw_input`, `raw_output`, then `content` items; for a `UserMessage`, `content` items; for
    /// an `AgentContent`, its block; for a compaction `Marker`, its `summary`. Every other entry
    /// costs 0. Message text (`ContentBlock::Text`, an agent text's `text`) is a `String`, not a
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
            Self::Marker(e) => match &e.kind {
                MarkerKind::Compaction { summary, .. } => optional_len(summary.as_ref()),
                MarkerKind::StepUndone { .. } | MarkerKind::VersionRestored { .. } => 0,
            },
            Self::Header(_)
            | Self::AgentText(_)
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
        ToolCallContent::Text { text } => text.inline_len(),
        ToolCallContent::Content { content } => block_len(content),
        ToolCallContent::Diff { old, new, .. } => optional_len(old.as_ref()) + new.inline_len(),
        ToolCallContent::Unknown { .. } => 0,
    }
}
