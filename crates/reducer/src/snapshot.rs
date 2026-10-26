//! Snapshots: a stream's state as bytes, written from a borrow, and read back.
//!
//! The bytes are `MessagePack` with named maps (`rmp_serde::to_vec_named`). A thread's state
//! serialises only the durable part of each message (cut at its durable length) with its durable
//! UTF-16 count, and leaves out messages only deltas opened, so no copy is made to drop
//! un-durable text and a snapshot always equals the fold of the log. Decoding rebuilds the
//! indexes from the entries.
//!
//! `reducer_schema` is the first key, and `Snapshot::from_bytes` reads it before the state: bytes
//! another schema wrote give [`Error::SchemaMismatch`] whatever shape their state has, never
//! [`Error::SnapshotDecode`].

use std::collections::BTreeMap;
use std::fmt;

use clauseo_protocol::{
    AgentTextEntry, ErrorCode, HeaderEntry, MatterHeaderEntry, MatterId, MessageId, Seq, TextKind,
    ThreadId, ThreadStatus, TurnId, ViewEntry,
};
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::state::{Index, MatterState, ThreadState, TurnFiles};
use crate::{Error, StreamState};

/// The version of the state's serialised shape. A snapshot of another version is rebuilt from
/// events. Bump it with any change to a state field or to how it serialises: the insta test
/// `snapshot_shape_is_pinned` fails on such a change.
pub const REDUCER_SCHEMA_VERSION: u32 = 1;

/// A snapshot as read: the schema it was written with and the state.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// The `REDUCER_SCHEMA_VERSION` that wrote it.
    pub reducer_schema: u32,
    /// The state.
    pub state: StreamState,
}

/// A snapshot to write: the schema and a borrow of the state, never a copy. `reducer_schema` is
/// written first, so a reader can stop before a state of another shape.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SnapshotRef<'a> {
    /// This build's `REDUCER_SCHEMA_VERSION`.
    pub reducer_schema: u32,
    /// The state.
    pub state: &'a StreamState,
}

impl Snapshot {
    /// A snapshot of `state` at this build's schema, borrowing it.
    pub const fn of(state: &StreamState) -> SnapshotRef<'_> {
        SnapshotRef {
            reducer_schema: REDUCER_SCHEMA_VERSION,
            state,
        }
    }

    /// Decodes snapshot bytes. Reads `reducer_schema` first and stops with
    /// [`Error::SchemaMismatch`] when another schema wrote them, before decoding their state;
    /// [`Error::SnapshotDecode`] when they do not decode.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let read: Read = rmp_serde::from_slice(bytes).map_err(Error::SnapshotDecode)?;
        match read.state {
            Some(state) => Ok(Self {
                reducer_schema: read.reducer_schema,
                state,
            }),
            None => Err(Error::SchemaMismatch {
                found: read.reducer_schema,
                expected: REDUCER_SCHEMA_VERSION,
            }),
        }
    }

    /// The state, if this build's schema wrote it; [`Error::SchemaMismatch`] otherwise.
    pub fn into_state(self) -> Result<StreamState, Error> {
        if self.reducer_schema != REDUCER_SCHEMA_VERSION {
            return Err(Error::SchemaMismatch {
                found: self.reducer_schema,
                expected: REDUCER_SCHEMA_VERSION,
            });
        }
        Ok(self.state)
    }
}

/// What snapshot bytes hold: the schema that wrote them, and the state when it is this build's.
struct Read {
    reducer_schema: u32,
    state: Option<StreamState>,
}

/// A snapshot's keys, in the order `SnapshotRef` writes them.
#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum Key {
    ReducerSchema,
    State,
}

impl<'de> Deserialize<'de> for Read {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_struct("Snapshot", &["reducer_schema", "state"], ReadVisitor)
    }
}

struct ReadVisitor;

impl<'de> Visitor<'de> for ReadVisitor {
    type Value = Read;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a snapshot map: reducer_schema, then state")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Read, A::Error> {
        if !matches!(map.next_key::<Key>()?, Some(Key::ReducerSchema)) {
            return Err(de::Error::custom("a snapshot starts with reducer_schema"));
        }
        let found: u32 = map.next_value()?;
        if found != REDUCER_SCHEMA_VERSION {
            // Another schema: its state may have any shape, so it is skipped, never decoded.
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            return Ok(Read {
                reducer_schema: found,
                state: None,
            });
        }
        if !matches!(map.next_key::<Key>()?, Some(Key::State)) {
            return Err(de::Error::missing_field("state"));
        }
        let state = map.next_value()?;
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Read {
            reducer_schema: found,
            state: Some(state),
        })
    }
}

impl SnapshotRef<'_> {
    /// The snapshot's bytes; [`Error::SnapshotEncode`] when a value does not encode.
    pub fn to_bytes(self) -> Result<Vec<u8>, Error> {
        rmp_serde::to_vec_named(&self).map_err(Error::SnapshotEncode)
    }
}

impl Serialize for ThreadState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("ThreadState", 10)?;
        s.serialize_field("thread", &self.thread)?;
        s.serialize_field("seq", &self.seq)?;
        s.serialize_field("header", &self.header)?;
        s.serialize_field("entries", &DurableEntries(self))?;
        s.serialize_field("text_utf16", &DurableCounts(self))?;
        s.serialize_field("turn", &self.turn)?;
        s.serialize_field("running", &self.running)?;
        s.serialize_field("after_turn", &self.after_turn)?;
        s.serialize_field("files", &self.files)?;
        s.serialize_field("failures", &self.failures)?;
        s.end()
    }
}

/// The entries as a list, oldest first, each message cut to its durable part, messages only
/// deltas opened left out.
struct DurableEntries<'a>(&'a ThreadState);

impl Serialize for DurableEntries<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let state = self.0;
        let provisional = usize::try_from(state.index.provisional).unwrap_or(usize::MAX);
        let len = state.entries.len().saturating_sub(provisional);
        let mut list = serializer.serialize_seq(Some(len))?;
        for entry in state.entries.values() {
            let ViewEntry::AgentText(text) = entry else {
                list.serialize_element(entry)?;
                continue;
            };
            match state.index.texts.get(&text.message) {
                Some(meta) if meta.provisional => {}
                Some(meta) if meta.durable_len < text.text.len() => {
                    let durable = text.text.get(..meta.durable_len).unwrap_or_default();
                    list.serialize_element(&DurableText { text, durable })?;
                }
                Some(_) | None => list.serialize_element(entry)?,
            }
        }
        list.end()
    }
}

/// An agent text entry with its text cut, serialised as `ViewEntry::AgentText` serialises.
struct DurableText<'a> {
    text: &'a AgentTextEntry,
    durable: &'a str,
}

/// `AgentTextEntry`'s fields, borrowed, in its declaration order.
#[derive(Serialize)]
struct AgentTextFields<'a> {
    order: Seq,
    message: MessageId,
    turn: TurnId,
    kind: TextKind,
    text: &'a str,
}

/// `ViewEntry::AgentText`'s variant index and key, as its derived `Serialize` writes them.
const AGENT_TEXT_INDEX: u32 = 2;
const AGENT_TEXT_KEY: &str = "agent_text";

impl Serialize for DurableText<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = AgentTextFields {
            order: self.text.order,
            message: self.text.message,
            turn: self.text.turn,
            kind: self.text.kind,
            text: self.durable,
        };
        serializer.serialize_newtype_variant("ViewEntry", AGENT_TEXT_INDEX, AGENT_TEXT_KEY, &fields)
    }
}

/// Each durable message's UTF-16 count, in entry order.
struct DurableCounts<'a>(&'a ThreadState);

impl Serialize for DurableCounts<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let state = self.0;
        let provisional = usize::try_from(state.index.provisional).unwrap_or(usize::MAX);
        let len = state.index.texts.len().saturating_sub(provisional);
        let mut map = serializer.serialize_map(Some(len))?;
        for entry in state.entries.values() {
            if let ViewEntry::AgentText(text) = entry
                && let Some(meta) = state.index.texts.get(&text.message)
                && !meta.provisional
            {
                map.serialize_entry(&text.message, &meta.durable_u16)?;
            }
        }
        map.end()
    }
}

/// A thread's state as read, before its indexes are rebuilt.
#[derive(Deserialize)]
struct ThreadWire {
    thread: ThreadId,
    seq: Option<Seq>,
    header: Option<HeaderEntry>,
    entries: Vec<ViewEntry>,
    text_utf16: BTreeMap<MessageId, u32>,
    turn: Option<TurnId>,
    running: Option<TurnId>,
    after_turn: Option<ThreadStatus>,
    files: BTreeMap<TurnId, TurnFiles>,
    failures: BTreeMap<TurnId, ErrorCode>,
}

impl<'de> Deserialize<'de> for ThreadState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ThreadWire::deserialize(deserializer)?;
        let entries = by_order(wire.entries);
        let index = Index::rebuild(&entries, &wire.text_utf16);
        Ok(Self {
            thread: wire.thread,
            seq: wire.seq,
            header: wire.header,
            entries,
            turn: wire.turn,
            running: wire.running,
            after_turn: wire.after_turn,
            files: wire.files,
            failures: wire.failures,
            index,
        })
    }
}

/// Entries keyed by their order.
fn by_order(entries: Vec<ViewEntry>) -> BTreeMap<Seq, ViewEntry> {
    entries
        .into_iter()
        .map(|entry| (entry.order(), entry))
        .collect()
}

impl Serialize for MatterState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("MatterState", 4)?;
        s.serialize_field("matter", &self.matter)?;
        s.serialize_field("seq", &self.seq)?;
        s.serialize_field("header", &self.header)?;
        s.serialize_field("entries", &EntryList(&self.entries))?;
        s.end()
    }
}

/// Entries as a list, oldest first.
struct EntryList<'a>(&'a BTreeMap<Seq, ViewEntry>);

impl Serialize for EntryList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.values())
    }
}

/// A matter's state as read.
#[derive(Deserialize)]
struct MatterWire {
    matter: MatterId,
    seq: Option<Seq>,
    header: Option<MatterHeaderEntry>,
    entries: Vec<ViewEntry>,
}

impl<'de> Deserialize<'de> for MatterState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = MatterWire::deserialize(deserializer)?;
        let mut state = Self::new(wire.matter);
        state.seq = wire.seq;
        state.header = wire.header;
        state.entries = by_order(wire.entries);
        state.rebuild();
        Ok(state)
    }
}
