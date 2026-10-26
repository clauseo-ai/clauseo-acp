//! The 20k-event reopen fixture: the one generator of the shape both this crate's reopen bench
//! and `host`'s whole-reopen bench time, so the two measure the same thread.
//!
//! Behind the `test-support` feature; production builds never compile it.

#![expect(
    clippy::expect_used,
    reason = "test support: fixed ids, paths and sizes that always pass protocol's checks"
)]

use clauseo_protocol::{
    AgentId, ChangeKind, CheckpointId, CompactString, ContentBlock, ContentId, Count, Delivery,
    DomainEvent, Envelope, EventId, FileChange, FilesChanged, MatterId, MatterKind, MatterPath,
    MessageId, Seq, StopReason, StreamId, TextAppended, TextKind, ThreadCreated, ThreadId,
    Timestamp, ToolCallId, ToolCallPatch, ToolCallStarted, ToolCallStatus, ToolCallUpdated,
    ToolKind, TurnEnded, TurnId, TurnStarted, UserMessage,
};

use crate::{Commit, MatterRow, SnapshotIn, ThreadRow};

/// Events in the fixture's thread.
pub const EVENTS: u64 = 20_000;
/// Turns, each of [`EVENTS_PER_TURN`] events.
pub const TURNS: u64 = 80;
/// Events per turn: one `UserMessage`, one `TurnStarted`, 196 `TextAppended` of 400 bytes,
/// 25 `ToolCallStarted`, 25 `ToolCallUpdated`, one `FilesChanged` and one `TurnEnded`.
pub const EVENTS_PER_TURN: u64 = 250;
/// The `seq` of the one snapshot: the tail after it is 1,999 events, the most the
/// every-2,000-events rule allows.
pub const SNAPSHOT_SEQ: u64 = 18_001;
/// The bytes of each `TextAppended`'s text.
pub const TEXT_BYTES: usize = 400;

/// The fixture's thread.
pub fn thread() -> ThreadId {
    id(1)
}

/// The fixture's matter.
pub fn matter() -> MatterId {
    id(2)
}

/// The fixture's thread stream.
pub fn stream() -> StreamId {
    StreamId::Thread(thread())
}

/// The commits that write the 20,000-event thread, in order: one per turn, with turn 73 split
/// after its first event, which carries the snapshot at seq 18,001. Built deterministically
/// (fixed ids and times). `snapshot` is called once, with events 1 to 18,001 in order, and
/// returns the reducer's encoded state after them; the fixture compresses it with
/// [`SnapshotIn::compress`] under `reducer_schema`. The first commit also carries the thread's
/// `folder` matter row (fixed paths that need not exist) and its thread row.
pub fn reopen_20k(
    snapshot: impl FnOnce(&[Envelope]) -> Vec<u8>,
    reducer_schema: u32,
) -> Vec<Commit> {
    let events: Vec<Envelope> = (1..=EVENTS).map(envelope).collect();
    let state = snapshot(events.get(..to_index(SNAPSHOT_SEQ)).expect("20,000 events"));
    let snapshot = SnapshotIn::compress(stream(), seq(SNAPSHOT_SEQ), reducer_schema, &state);
    let mut snapshot = Some(snapshot);
    let mut commits = Vec::new();
    let mut events = events.into_iter().peekable();
    let mut start = 1;
    while events.peek().is_some() {
        let turn_end = (start - 1) / EVENTS_PER_TURN * EVENTS_PER_TURN + EVENTS_PER_TURN;
        let end = if start <= SNAPSHOT_SEQ {
            turn_end.min(SNAPSHOT_SEQ)
        } else {
            turn_end
        };
        let batch: Vec<Envelope> = events.by_ref().take(to_index(end - start + 1)).collect();
        let mut commit = Commit {
            events: batch,
            ..Commit::default()
        };
        if start == 1 {
            commit.matter = Some(matter_row());
            commit.thread = Some(thread_row());
        }
        if end == SNAPSHOT_SEQ {
            commit.snapshot = snapshot.take();
        }
        commits.push(commit);
        start = end + 1;
    }
    commits
}

/// The fixture's matter row.
pub fn matter_row() -> MatterRow {
    MatterRow {
        id: matter(),
        name: "Fixture matter".to_owned(),
        kind: MatterKind::Folder,
        folder_path: "/clauseo-fixture/matter".to_owned(),
        folder_locator: None,
        shadow_repo_path: "/clauseo-fixture/shadow".to_owned(),
        created: time(0),
    }
}

/// The fixture's thread row.
pub fn thread_row() -> ThreadRow {
    ThreadRow {
        id: thread(),
        matter: matter(),
        agent: AgentId::new("claude-acp").expect("a valid agent id"),
        title: Some("Fixture thread".to_owned()),
        acp_session_id: None,
        archived: false,
        created: time(0),
        updated: time(EVENTS),
    }
}

/// The envelope at `seq`.
fn envelope(n: u64) -> Envelope {
    Envelope::new(
        id::<EventId>(1_000_000 + n),
        stream(),
        seq(n),
        time(n),
        None,
        event(n),
    )
}

/// The event at `seq`: seq 1 is `ThreadCreated`; then each turn of 250 events is a
/// `UserMessage`, a `TurnStarted`, 25 runs of 7 `TextAppended`, a `ToolCallStarted` and its
/// `ToolCallUpdated`, then 21 `TextAppended`, a `FilesChanged` and a `TurnEnded`. Seq 1
/// takes the place of turn 1's first event, so every later event keeps its place.
fn event(n: u64) -> DomainEvent {
    if n == 1 {
        return ThreadCreated {
            matter: matter(),
            agent: AgentId::new("claude-acp").expect("a valid agent id"),
            title: Some(CompactString::new("Fixture thread")),
        }
        .into();
    }
    let turn_index = (n - 1) / EVENTS_PER_TURN;
    let at = (n - 1) % EVENTS_PER_TURN;
    let turn: TurnId = id(10_000 + turn_index);
    let message: MessageId = id(20_000 + turn_index);
    match at {
        0 => UserMessage {
            turn,
            message: id(30_000 + turn_index),
            acp_message_id: None,
            content: vec![ContentBlock::Text { text: text(n) }],
            delivery: Delivery::default(),
        }
        .into(),
        1 => TurnStarted {
            turn,
            checkpoint_before: id::<CheckpointId>(40_000 + 2 * turn_index),
        }
        .into(),
        248 => FilesChanged {
            turn,
            files: vec![
                FileChange::new(
                    MatterPath::new("drafts/lease.docx").expect("a valid path"),
                    ChangeKind::Modified,
                    Some(ContentId::new("ab12").expect("a valid content id")),
                    Some(ContentId::new("cd34").expect("a valid content id")),
                    Count::from(2048_u32),
                )
                .expect("a valid change"),
            ],
            skipped: vec![],
        }
        .into(),
        249 => TurnEnded {
            turn,
            stop: StopReason::EndTurn,
            usage: None,
            checkpoint_after: Some(id::<CheckpointId>(40_001 + 2 * turn_index)),
        }
        .into(),
        // 2..=176: 25 runs of 7 texts, a tool call started and updated.
        2..=176 if (at - 2) % 9 >= 7 => {
            let call: ToolCallId = id(50_000 + turn_index * 25 + (at - 2) / 9);
            if (at - 2) % 9 == 7 {
                DomainEvent::from(Box::new(ToolCallStarted {
                    turn,
                    tool_call: call,
                    acp_id: CompactString::new(format!("toolu_{n}")),
                    title: CompactString::new("Read the lease"),
                    kind: ToolKind::Read,
                    tool_name: Some(CompactString::new("Read")),
                    status: ToolCallStatus::InProgress,
                    locations: vec![],
                    raw_input: None,
                    raw_output: None,
                    content: vec![],
                }))
            } else {
                DomainEvent::from(Box::new(ToolCallUpdated {
                    tool_call: call,
                    patch: ToolCallPatch {
                        status: Some(ToolCallStatus::Completed),
                        ..ToolCallPatch::default()
                    },
                }))
            }
        }
        _ => TextAppended {
            turn,
            message,
            acp_message_id: None,
            kind: TextKind::Answer,
            text: text(n),
        }
        .into(),
    }
}

/// 400 bytes of text that differ per event.
fn text(n: u64) -> String {
    let mut text = format!("Event {n}: ");
    while text.len() < TEXT_BYTES {
        text.push_str("the tenant shall keep the premises in good repair. ");
    }
    text.truncate(TEXT_BYTES);
    text
}

/// A fixed v7 id whose low bits are `n`.
fn id<T: std::str::FromStr>(n: u64) -> T
where
    T::Err: std::fmt::Debug,
{
    format!("01900000-0000-7000-8000-{n:012x}")
        .parse()
        .expect("a valid v7 uuid")
}

fn seq(n: u64) -> Seq {
    Seq::new(n).expect("a seq from 1")
}

fn time(n: u64) -> Timestamp {
    Timestamp::new(1_800_000_000_000 + i64::try_from(n).expect("a small n")).expect("a valid time")
}

fn to_index(n: u64) -> usize {
    usize::try_from(n).expect("a small count")
}
