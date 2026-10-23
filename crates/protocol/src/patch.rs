//! View patches, the host events a subscriber receives, and the parameters of the subscription
//! calls.
//!
//! Cursors live in host memory: text patches reach the screen before their events are durable,
//! so a cursor cannot come from `seq`. The host mints a [`ViewEpoch`] each time it loads a stream,
//! shared by every subscriber of that stream; a [`Subscribe`] whose epoch differs gets a `Reset`.
//! The webview applies only the next cursor, and `AppendText` only when the text it extends is
//! `from_len` UTF-16 units long.
//!
//! The call shapes follow the host's `HostHandle` and `transport_ws`'s frames:
//! `subscribe(stream, after: Option<(epoch, cursor)>)`, `subscribe_events(stream, after_seq:
//! Option<Seq>)` and `ack(stream, epoch, cursor)`.

use serde::{Deserialize, Serialize};

use crate::{
    AccountLimits, Cursor, EntryId, Envelope, HeaderEntry, MatterHeaderEntry, MessageId, Seq,
    StreamId, ViewEntry, ViewEpoch,
};

/// One change to a stream's view, at its cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ViewPatch {
    /// The patch's place in the stream's patch sequence; each patch is the previous one plus 1.
    pub cursor: Cursor,
    /// The change.
    pub body: ViewPatchBody,
}

/// What a view patch changes. JSON: `{"upsert": {"entry": {...}}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ViewPatchBody {
    /// Add an entry, or replace the entry with the same id.
    Upsert {
        /// The whole entry.
        entry: Box<ViewEntry>,
    },
    /// Remove an entry (no event does this yet).
    Remove {
        /// The entry.
        entry_id: EntryId,
    },
    /// Append text to a message, only if the text it extends is `from_len` long. No patch
    /// carries more than [`INLINE_LIMIT_BYTES`](crate::INLINE_LIMIT_BYTES) of text, so a long
    /// message goes out as an `Upsert` with its long text cut, then `AppendText` patches.
    AppendText {
        /// The message: an `AgentText` entry's or a `UserMessage` entry's, as `target` says.
        message_id: MessageId,
        /// Which text of the message grows.
        target: AppendTarget,
        /// The extended text's length before this append, in UTF-16 code units (JavaScript's
        /// `string.length`; see [`utf16_len`](crate::utf16_len)).
        from_len: u32,
        /// The text to append; its `escaped_len` is at most
        /// [`INLINE_LIMIT_BYTES`](crate::INLINE_LIMIT_BYTES).
        text: String,
    },
    /// Drop the view the subscriber holds and show this window of it instead; the patch's cursor
    /// is where the window stands.
    Reset {
        /// The header and the most recent entries.
        view_window: Box<ViewWindow>,
    },
}

/// The text an `AppendText` extends. JSON: `"agent_text"` or `{"user_block": 1}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AppendTarget {
    /// The `text` of the `AgentText` entry `message_id` names.
    AgentText,
    /// The `Text` block at this index of the `content` of the `UserMessage` entry
    /// `message_id` names.
    UserBlock(u32),
}

/// A window of a stream's view, as a `Reset` carries it: the header and about 200 recent
/// entries. It is neither stored nor the whole view; older entries page in with `PageEntries`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ViewWindow {
    /// The stream's header, kept even when it is older than `entries`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub header: Option<StreamHeader>,
    /// The most recent other entries, sorted by `order`, oldest first.
    pub entries: Vec<ViewEntry>,
    /// Whether entries older than the first of `entries` exist.
    pub has_older: bool,
}

/// A stream's header entry, as a `ViewWindow` carries it: only the two header kinds decode
/// here. JSON as the entry's: `{"header": {...}}` or `{"matter_header": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum StreamHeader {
    /// A thread's header.
    #[serde(rename = "header")]
    Thread(HeaderEntry),
    /// A matter's header.
    #[serde(rename = "matter_header")]
    Matter(MatterHeaderEntry),
}

/// Patches of one stream, sent together. The stream and the epoch come once per batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PatchBatch {
    /// The stream.
    pub stream: StreamId,
    /// The epoch the cursors belong to.
    pub epoch: ViewEpoch,
    /// The patches, cursors consecutive.
    pub patches: Vec<ViewPatch>,
}

/// What a subscriber receives. Never logged (the log holds only `DomainEvent`s).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum HostEvent {
    /// View patches, for a view subscription.
    Patches(PatchBatch),
    /// Envelopes in `seq` order with no gap or duplicate, for an event subscription.
    Events(Vec<Envelope>),
    /// Once per event subscription, between the replayed envelopes and the live ones.
    CaughtUp {
        /// The stream.
        stream: StreamId,
        /// The stream's head when the replay ended. Every stream the host knows holds its
        /// creation event, so it is at least 1.
        last_seq: Seq,
    },
    /// An account's limits changed (not part of any stream).
    AccountLimitsChanged(AccountLimits),
}

/// Where a view subscriber stands: an epoch and the last cursor it applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ViewPosition {
    /// The epoch of the cursor.
    pub epoch: ViewEpoch,
    /// The last cursor applied.
    pub cursor: Cursor,
}

/// The parameters of `subscribe`: the tail after `after`, or a `Reset` when `after` is `None`,
/// names another epoch, or fell out of the host's patch window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Subscribe {
    /// The stream.
    pub stream: StreamId,
    /// Where the subscriber stands; `None` for a first subscription.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub after: Option<ViewPosition>,
}

/// The parameters of `subscribe_events`: envelopes after `after_seq` (from the first when
/// `None`), then `CaughtUp`, then live envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SubscribeEvents {
    /// The stream.
    pub stream: StreamId,
    /// The last `seq` the subscriber has.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub after_seq: Option<Seq>,
}

/// The parameters of `ack`: the cursor a view subscriber has applied. The host records it;
/// backpressure is the desktop bridge's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Ack {
    /// The stream.
    pub stream: StreamId,
    /// The epoch of the cursor.
    pub epoch: ViewEpoch,
    /// The last cursor applied.
    pub cursor: Cursor,
}
