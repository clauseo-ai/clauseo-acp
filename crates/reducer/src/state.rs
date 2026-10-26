//! The state a stream's events fold into: a thread's or a matter's.
//!
//! Entries live in a `BTreeMap` keyed by `order`, so equal states iterate and serialise alike.
//! The indexes from domain ids to `order` are std `HashMap`s for lookups only: never iterated,
//! never saved, and rebuilt from the entries when a state is decoded (`Index::rebuild`), so the
//! hash seed never reaches any output.

use std::collections::{BTreeMap, HashMap};

use clauseo_protocol::{
    CheckpointId, ElicitationId, ElicitationState, ErrorCode, FileChange, HeaderEntry,
    MatterHeaderEntry, MatterId, MessageId, PermissionRequestId, PermissionState, Seq, SkippedFile,
    StreamId, ThreadId, ThreadStatus, ToolCallId, TurnId, ViewEntry, utf16_len,
};
use serde::{Deserialize, Serialize};

/// A stream's folded state. JSON: `{"thread": {...}}` or `{"matter": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "a state is built once per stream and folded in place; boxing the thread's state \
              would add a pointer chase to every event"
)]
pub enum StreamState {
    /// A thread's stream.
    Thread(ThreadState),
    /// A matter's stream.
    Matter(MatterState),
}

impl StreamState {
    /// The empty state of `stream`, before its first event.
    pub fn new(stream: StreamId) -> Self {
        match stream {
            StreamId::Thread(thread) => Self::Thread(ThreadState::new(thread)),
            StreamId::Matter(matter) => Self::Matter(MatterState::new(matter)),
        }
    }

    /// The stream.
    pub const fn stream(&self) -> StreamId {
        match self {
            Self::Thread(state) => StreamId::Thread(state.thread),
            Self::Matter(state) => StreamId::Matter(state.matter),
        }
    }

    /// The `seq` of the last event folded in; `None` before the first.
    pub const fn seq(&self) -> Option<Seq> {
        match self {
            Self::Thread(state) => state.seq,
            Self::Matter(state) => state.seq,
        }
    }

    /// The thread's state, for `apply_text_delta`; `None` for a matter.
    pub const fn as_thread_mut(&mut self) -> Option<&mut ThreadState> {
        match self {
            Self::Thread(state) => Some(state),
            Self::Matter(_) => None,
        }
    }

    /// The thread's state; `None` for a matter.
    pub const fn as_thread(&self) -> Option<&ThreadState> {
        match self {
            Self::Thread(state) => Some(state),
            Self::Matter(_) => None,
        }
    }

    /// The matter's state; `None` for a thread.
    pub const fn as_matter(&self) -> Option<&MatterState> {
        match self {
            Self::Matter(state) => Some(state),
            Self::Thread(_) => None,
        }
    }

    /// Every entry other than the header, by `order`.
    pub const fn entries(&self) -> &BTreeMap<Seq, ViewEntry> {
        match self {
            Self::Thread(state) => &state.entries,
            Self::Matter(state) => &state.entries,
        }
    }
}

/// The files a turn changed, from its `FilesChanged`, until its `TurnEnded` makes the step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TurnFiles {
    pub(crate) files: Vec<FileChange>,
    pub(crate) skipped: Vec<SkippedFile>,
}

/// A thread's folded state: its header, its transcript entries, and what the fold needs to
/// know about the turn in progress.
///
/// Serialising it writes only the durable part of each message and leaves out messages that
/// only deltas opened (`snapshot.rs`), so a snapshot always equals the fold of the log.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadState {
    pub(crate) thread: ThreadId,
    pub(crate) seq: Option<Seq>,
    pub(crate) header: Option<HeaderEntry>,
    /// Every entry but the header, by `order`.
    pub(crate) entries: BTreeMap<Seq, ViewEntry>,
    /// The turn the last `UserMessage` or `TurnStarted` named.
    pub(crate) turn: Option<TurnId>,
    /// The turn that started and has not ended.
    pub(crate) running: Option<TurnId>,
    /// The status a detach during the running turn gives once the turn ends.
    pub(crate) after_turn: Option<ThreadStatus>,
    /// `FilesChanged` of turns that have not ended.
    pub(crate) files: BTreeMap<TurnId, TurnFiles>,
    /// `TurnFailed` codes of turns that have not ended.
    pub(crate) failures: BTreeMap<TurnId, ErrorCode>,
    pub(crate) index: Index,
}

impl ThreadState {
    /// The empty state of `thread`.
    pub fn new(thread: ThreadId) -> Self {
        Self {
            thread,
            seq: None,
            header: None,
            entries: BTreeMap::new(),
            turn: None,
            running: None,
            after_turn: None,
            files: BTreeMap::new(),
            failures: BTreeMap::new(),
            index: Index::default(),
        }
    }

    /// The thread.
    pub const fn thread(&self) -> ThreadId {
        self.thread
    }

    /// The `seq` of the last event folded in.
    pub const fn seq(&self) -> Option<Seq> {
        self.seq
    }

    /// The header, once `ThreadCreated` is folded in.
    pub const fn header(&self) -> Option<&HeaderEntry> {
        self.header.as_ref()
    }

    /// Where the thread stands; `Idle` before `ThreadCreated`.
    pub fn status(&self) -> ThreadStatus {
        self.header
            .as_ref()
            .map_or(ThreadStatus::Idle, |header| header.status)
    }

    /// Every entry but the header, by `order`, un-durable text included.
    pub const fn entries(&self) -> &BTreeMap<Seq, ViewEntry> {
        &self.entries
    }
}

/// A matter's folded state: its header and its Versions timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct MatterState {
    pub(crate) matter: MatterId,
    pub(crate) seq: Option<Seq>,
    pub(crate) header: Option<MatterHeaderEntry>,
    /// `Version` and `Marker` entries, by `order`.
    pub(crate) entries: BTreeMap<Seq, ViewEntry>,
    /// Checkpoint to its `Version` entry's order; never iterated or saved.
    pub(crate) versions: HashMap<CheckpointId, Seq>,
}

impl MatterState {
    /// The empty state of `matter`.
    pub fn new(matter: MatterId) -> Self {
        Self {
            matter,
            seq: None,
            header: None,
            entries: BTreeMap::new(),
            versions: HashMap::new(),
        }
    }

    /// The matter.
    pub const fn matter(&self) -> MatterId {
        self.matter
    }

    /// The `seq` of the last event folded in.
    pub const fn seq(&self) -> Option<Seq> {
        self.seq
    }

    /// The header, once `MatterCreated` is folded in.
    pub const fn header(&self) -> Option<&MatterHeaderEntry> {
        self.header.as_ref()
    }

    /// The `Version` and `Marker` entries, by `order`.
    pub const fn entries(&self) -> &BTreeMap<Seq, ViewEntry> {
        &self.entries
    }

    /// Rebuilds the checkpoint index from the entries.
    pub(crate) fn rebuild(&mut self) {
        self.versions.clear();
        for (order, entry) in &self.entries {
            if let ViewEntry::Version(version) = entry {
                self.versions.insert(version.checkpoint, *order);
            }
        }
    }
}

/// A message's text counts, kept beside its text so `from_len` never needs a rescan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextMeta {
    /// Its entry's order.
    pub(crate) order: Seq,
    /// UTF-16 length of the shown text.
    pub(crate) shown_u16: u32,
    /// Byte length of the durable part (text that came as `TextAppended`).
    pub(crate) durable_len: usize,
    /// UTF-16 length of the durable part.
    pub(crate) durable_u16: u32,
    /// Opened by a delta and not yet confirmed by its first `TextAppended`.
    pub(crate) provisional: bool,
}

/// The lookups from domain ids to `order`, and the counts derived from the entries. Never
/// iterated, never saved: rebuilt by [`Index::rebuild`].
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Index {
    pub(crate) texts: HashMap<MessageId, TextMeta>,
    pub(crate) users: HashMap<MessageId, Seq>,
    pub(crate) tool_calls: HashMap<ToolCallId, Seq>,
    pub(crate) permissions: HashMap<PermissionRequestId, Seq>,
    pub(crate) elicitations: HashMap<ElicitationId, Seq>,
    pub(crate) steps: HashMap<TurnId, Seq>,
    pub(crate) limit_stops: HashMap<TurnId, Seq>,
    /// Pending permissions and elicitations.
    pub(crate) waiting: u32,
    /// Messages a delta opened that no `TextAppended` has confirmed yet.
    pub(crate) provisional: u32,
}

impl Index {
    /// The index of `entries`, whose agent texts are wholly durable, with each message's UTF-16
    /// count from `text_utf16` (counted afresh when missing).
    pub(crate) fn rebuild(
        entries: &BTreeMap<Seq, ViewEntry>,
        text_utf16: &BTreeMap<MessageId, u32>,
    ) -> Self {
        let mut index = Self::default();
        for (order, entry) in entries {
            let order = *order;
            match entry {
                ViewEntry::AgentText(text) => {
                    let u16 = text_utf16
                        .get(&text.message)
                        .copied()
                        .unwrap_or_else(|| count_u16(&text.text));
                    let meta = TextMeta {
                        order,
                        shown_u16: u16,
                        durable_len: text.text.len(),
                        durable_u16: u16,
                        provisional: false,
                    };
                    index.texts.insert(text.message, meta);
                }
                ViewEntry::UserMessage(user) => {
                    index.users.insert(user.message, order);
                }
                ViewEntry::ToolCall(call) => {
                    index.tool_calls.insert(call.tool_call, order);
                }
                ViewEntry::Permission(permission) => {
                    index.permissions.insert(permission.request, order);
                    if permission.state == PermissionState::Pending {
                        index.waiting += 1;
                    }
                }
                ViewEntry::Elicitation(elicitation) => {
                    index.elicitations.insert(elicitation.elicitation, order);
                    if elicitation.state == ElicitationState::Pending {
                        index.waiting += 1;
                    }
                }
                ViewEntry::Step(step) => {
                    index.steps.insert(step.turn, order);
                }
                ViewEntry::LimitStop(stop) => {
                    index.limit_stops.insert(stop.turn, order);
                }
                ViewEntry::Header(_)
                | ViewEntry::AgentContent(_)
                | ViewEntry::Notice(_)
                | ViewEntry::Marker(_)
                | ViewEntry::MatterHeader(_)
                | ViewEntry::Version(_) => {}
            }
        }
        index
    }
}

/// `utf16_len` as the `u32` a `from_len` holds, saturating (a message never nears 4 GiB).
pub(crate) fn count_u16(text: &str) -> u32 {
    u32::try_from(utf16_len(text)).unwrap_or(u32::MAX)
}
