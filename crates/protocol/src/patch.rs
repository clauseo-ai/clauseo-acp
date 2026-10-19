//! View patches, the host events a subscriber receives, and the parameters of the subscription
//! calls.
//!
//! Cursors live in host memory: text patches reach the screen before their events are durable,
//! so a cursor cannot come from `seq`. The host mints a [`ViewEpoch`] each time it loads a stream,
//! shared by every subscriber of that stream; a [`Subscribe`] whose epoch differs gets a `Reset`.
//! The webview applies only the next cursor, and `AppendText` only when its message's text length
//! in UTF-16 units equals `from_len`.
//!
//! The call shapes follow R11's `HostHandle` and S10's frames: `subscribe(stream, after:
//! Option<(epoch, cursor)>)`, `subscribe_events(stream, after_seq: Option<Seq>)` and
//! `ack(stream, epoch, cursor)`.

use serde::{Deserialize, Serialize};

use crate::{
    AccountLimits, Cursor, EntryId, Envelope, MessageId, Seq, StreamId, ViewEntry, ViewEpoch,
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
    /// Remove an entry (no M1 event does this).
    Remove {
        /// The entry.
        entry_id: EntryId,
    },
    /// Append text to an assistant message, only if its text is `from_len` long.
    AppendText {
        /// The message.
        message_id: MessageId,
        /// The text's length before this append, in UTF-16 code units (JavaScript's
        /// `string.length`; see [`utf16_len`](crate::utf16_len)).
        from_len: u32,
        /// The text to append; its `escaped_len` is at most
        /// [`INLINE_LIMIT_BYTES`](crate::INLINE_LIMIT_BYTES).
        text: String,
    },
    /// Replace the whole view; the patch's cursor is where the snapshot stands.
    Reset {
        /// The snapshot.
        view_snapshot: Box<ViewSnapshot>,
    },
}

/// A stream's view as a `Reset` carries it: the header and about 200 recent entries; older
/// entries page in with `PageEntries`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ViewSnapshot {
    /// The stream's `Header` or `MatterHeader`, kept even when it is older than `entries`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub header: Option<ViewEntry>,
    /// The most recent other entries, sorted by `order`, oldest first.
    pub entries: Vec<ViewEntry>,
    /// Whether entries older than the first of `entries` exist.
    pub has_older: bool,
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
/// backpressure is the desktop bridge's (M4).
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
