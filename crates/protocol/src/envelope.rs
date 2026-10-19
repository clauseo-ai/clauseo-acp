//! `Envelope`: one entry of a stream's log, the event with its position and provenance.

use serde::{Deserialize, Serialize};

use crate::{CommandId, DomainEvent, EventId, SCHEMA_VERSION, Seq, StreamId, Timestamp};

/// One logged event: which stream it belongs to, its place there, and what caused it.
///
/// Order by `seq`, never by `ts`. The event's JSON is a one-key object:
/// `"event": {"text_appended": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Envelope {
    /// The schema the event was written with; [`SCHEMA_VERSION`] (1 at the freeze).
    pub schema_version: u32,
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

    /// True when this version can read the envelope: its `schema_version` is at most
    /// [`SCHEMA_VERSION`]. Older versions are upcast on read. A reader that finds a newer one
    /// (written by a newer Clauseo) stops and returns `store.newer_version` rather than
    /// guessing at fields whose meaning may have changed.
    pub const fn is_supported(&self) -> bool {
        self.schema_version <= SCHEMA_VERSION
    }
}
