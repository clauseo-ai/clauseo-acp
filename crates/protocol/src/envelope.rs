//! `Envelope`: one entry of a stream's log, the event with its position and provenance.
//!
//! The version picks how the event is read: at [`SCHEMA_VERSION`] as this version's
//! [`DomainEvent`], older through [`upcast`], newer not at all. Serde writes `schema_version`
//! first, in JSON and in `MessagePack` named maps, so the event is decoded in place; an `event`
//! key before it (sorted keys) is held as read until the version is read. The decoder is in
//! `envelope/de.rs`, the reader of a held event in `envelope/held.rs`.

use serde::Serialize;

use crate::{
    CommandId, DomainEvent, Error, EventId, RawPayload, SCHEMA_VERSION, Seq, StreamId, Timestamp,
};

mod de;
mod held;
mod raw_de;
#[cfg(test)]
mod tests;

/// One logged event: which stream it belongs to, its place there, and what caused it.
///
/// Order by `seq`, never by `ts`. The event's JSON is a one-key object:
/// `"event": {"text_appended": {...}}`.
///
/// It decodes from a map only, its keys in any order. Serde writes `schema_version` before
/// `event`, and the event is then decoded in place; an `event` key before it, as sorted keys
/// put it (a `serde_json::Value`, a jsonb column, `jq -S`), is held as read, any key and any
/// depth the format reads, and decoded once the version is known by the same rules as in place:
/// a newer version reads as [`EnvelopeRead::Newer`] whatever the event holds, a field the event
/// does not know is skipped at any depth, and [`MAX_RAW_DEPTH`](crate::MAX_RAW_DEPTH) counts
/// from the root of each [`RawPayload`] in the event, an older event's included. So it decodes
/// to the same envelope within the format's nesting limit, which only the held copy reaches in
/// one case: `serde_json` skips a newer event or an unknown field without counting its depth in
/// place, but holds at most its 128 levels, the envelope's own map among them. A `MessagePack`
/// reader's `MAX_MSGPACK_DEPTH` counts both orders alike. A `schema_version` newer than
/// [`SCHEMA_VERSION`] fails to decode as an `Envelope`; decode an [`EnvelopeRead`] to tell
/// that apart from a bad payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[expect(
    clippy::partial_pub_fields,
    reason = "only schema_version is private, so Envelope::new is the only way to build one and \
              every row is written at SCHEMA_VERSION; the other fields stay plain data"
)]
pub struct Envelope {
    /// The schema the event is in: always [`SCHEMA_VERSION`] (1 at the freeze). Private, so
    /// [`Envelope::new`] is the only way to build one and every row is written at this version;
    /// a decoded envelope holds it too (an older entry's event is upcast on read, and its row
    /// keeps the version it was written with). Read it with [`Envelope::schema_version`].
    ///
    /// ```compile_fail,E0451
    /// fn bump(envelope: clauseo_protocol::Envelope) -> clauseo_protocol::Envelope {
    ///     clauseo_protocol::Envelope { schema_version: 7, ..envelope }
    /// }
    /// ```
    schema_version: u32,
    /// The event's id, a v7 UUID the host mints.
    pub event_id: EventId,
    /// The stream it belongs to: a thread or a matter.
    pub stream: StreamId,
    /// Its place in the stream: gapless, from 1, assigned by the host at commit.
    pub seq: Seq,
    /// When it was committed, for display only.
    pub ts: Timestamp,
    /// The command that caused it, if any.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub command_id: Option<CommandId>,
    /// The event.
    pub event: DomainEvent,
}

impl Envelope {
    /// An envelope for `event` at `seq` in `stream`, written with this crate's
    /// [`SCHEMA_VERSION`]. The host passes in the id and the time; this crate mints neither.
    pub fn new(
        event_id: EventId,
        stream: StreamId,
        seq: Seq,
        ts: Timestamp,
        command_id: Option<CommandId>,
        event: DomainEvent,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            event_id,
            stream,
            seq,
            ts,
            command_id,
            event,
        }
    }

    /// The schema the event is in: always [`SCHEMA_VERSION`], for a built envelope and a
    /// decoded one alike.
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

/// A stored envelope as this version reads it: an [`Envelope`], or a note that a newer Clauseo
/// wrote it.
///
/// The type a store decodes a row as. Plain `Deserialize` returns a format's own error type,
/// which cannot say "newer" in a way a reader can match, so the newer case is a value instead:
/// a reader maps [`EnvelopeRead::Newer`] to `store.newer_version` and a decode error to
/// `store.corrupt`, in JSON and in `MessagePack` alike, without reading error text.
#[derive(Debug, Clone, PartialEq)]
pub enum EnvelopeRead {
    /// Written at [`SCHEMA_VERSION`], or older and upcast to it.
    Current(Envelope),
    /// Written by a newer Clauseo. Nothing after `schema_version` is decoded with this version's
    /// types (their meaning may have changed); the decoder only checks the rest is well formed.
    Newer {
        /// The version found, greater than [`SCHEMA_VERSION`].
        schema_version: u32,
    },
}

/// Reads an event written at `version`, older than [`SCHEMA_VERSION`], as this version's
/// [`DomainEvent`].
///
/// The one place a `schema_version` bump adds to: version 2 adds the step that turns a version 1
/// payload into a version 2 event. Nothing older than version 1 exists, so at version 1 it
/// refuses every version with [`Error::NoUpcaster`].
pub(crate) fn upcast(version: u32, event: RawPayload) -> Result<DomainEvent, Error> {
    drop(event);
    Err(Error::NoUpcaster { version })
}
