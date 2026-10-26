//! Shared fixtures for the reducer's tests and its bench: deterministic event logs built from
//! small numbers, never from the clock.
//!
//! The reducer's tests include it with `#[path = "support/fixtures.rs"] pub mod fixtures;` and
//! `crates/reducer_bench/benches/apply.rs` with
//! `#[path = "../../reducer/tests/support/fixtures.rs"] pub mod fixtures;`, each beside the
//! same declaration of `events.rs` as `pub mod events`. `pub` keeps a file
//! that uses only some helpers free of unused warnings. The bench is not a test build, so
//! nothing here unwraps, panics or indexes: every fallible step returns [`Fx`] and uses `?`.
//!
//! Ids: `v7(n)` is a valid v7 UUID with a fixed timestamp and `n` in its low bits, built as text
//! and parsed, since the bench package has no `uuid`. The log of stream number `s` names its
//! stream `v7(s * ID_SPACE)` and takes every id it mints (events, turns, messages, tool calls,
//! checkpoints, requests) from one counter after it, so no two ids in a log collide.
//!
//! The event builders are in `events.rs` next to this file, a sibling module re-exported here.

use std::str::FromStr;

use clauseo_protocol::{
    CheckpointId, CheckpointKind, CheckpointRecorded, Count, DomainEvent, Envelope, Error, EventId,
    ImportSummary, MatterCreated, MatterId, MatterKind, MatterPath, MessageId, OutsideEdits,
    PermissionRequestId, RestoreScope, Seq, StreamId, TextKind, ThreadId, Timestamp, ToolCallId,
    ToolCallStatus, TurnId, TurnStarted, VersionRestored,
};

pub use super::events::*;

/// What every fallible fixture returns.
pub type Fx<T> = Result<T, Error>;

/// Ids each log may mint after its stream id: a log never reaches the next stream's ids.
pub const ID_SPACE: u64 = 1 << 32;

/// The stream number of the matter every fixture thread belongs to.
pub const MATTER: u64 = 1;

/// Text chunks per turn in [`long_thread`], before the last turn takes up the remainder.
pub const LONG_TURN_CHUNKS: usize = 10;

/// Events in [`scripted_turn`]: three to open it, 500 text chunks, 20 tool calls of three
/// events, three plans, a permission asked and answered, usage, files and the turn's end.
pub const SCRIPTED_TURN_EVENTS: usize = 3 + 500 + 20 * 3 + 3 + 2 + 3;

/// The text of `v7(n)`: version nibble 7, RFC 4122 variant bits, a fixed timestamp and the low
/// 62 bits of `n`, so the same `n` always gives the same id.
pub fn v7_text(n: u64) -> String {
    // 48-bit timestamp | version 7 | 12 bits zero | variant 0b10 | 62 bits of n.
    let timestamp: u128 = 0x0190_0000_0000;
    let bits = (timestamp << 80) | (0x7 << 76) | (0b10 << 62) | u128::from(n & ((1 << 62) - 1));
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        bits >> 96,
        (bits >> 80) & 0xffff,
        (bits >> 64) & 0xffff,
        (bits >> 48) & 0xffff,
        bits & 0xffff_ffff_ffff
    )
}

/// Any v7 id type from `v7(n)`.
pub fn id<T: FromStr<Err = Error>>(n: u64) -> Fx<T> {
    v7_text(n).parse()
}

/// A `ThreadId` from `v7(n)`.
pub fn thread_id(n: u64) -> Fx<ThreadId> {
    id(n)
}

/// A `MatterId` from `v7(n)`.
pub fn matter_id(n: u64) -> Fx<MatterId> {
    id(n)
}

/// A `TurnId` from `v7(n)`.
pub fn turn_id(n: u64) -> Fx<TurnId> {
    id(n)
}

/// A `MessageId` from `v7(n)`.
pub fn message_id(n: u64) -> Fx<MessageId> {
    id(n)
}

/// A `ToolCallId` from `v7(n)`.
pub fn tool_call_id(n: u64) -> Fx<ToolCallId> {
    id(n)
}

/// A `PermissionRequestId` from `v7(n)`.
pub fn permission_id(n: u64) -> Fx<PermissionRequestId> {
    id(n)
}

/// A `CheckpointId` from `v7(n)`.
pub fn checkpoint_id(n: u64) -> Fx<CheckpointId> {
    id(n)
}

/// An `EventId` from `v7(n)`.
pub fn event_id(n: u64) -> Fx<EventId> {
    id(n)
}

/// The thread stream of stream number `n`: `v7(n * ID_SPACE)`.
pub fn thread_stream(n: u64) -> Fx<StreamId> {
    Ok(StreamId::Thread(thread_id(n.saturating_mul(ID_SPACE))?))
}

/// The matter stream of stream number `n`: `v7(n * ID_SPACE)`.
pub fn matter_stream(n: u64) -> Fx<StreamId> {
    Ok(StreamId::Matter(matter_id(n.saturating_mul(ID_SPACE))?))
}

/// The matter every fixture thread belongs to, the one [`matter_versions`] logs.
pub fn fixture_matter() -> Fx<MatterId> {
    matter_id(MATTER.saturating_mul(ID_SPACE))
}

/// One stream's log under construction: gapless seqs from 1, `ts` = seq * 1000 ms, no command.
#[derive(Debug, Clone)]
pub struct Log {
    stream: StreamId,
    next_id: u64,
    events: Vec<Envelope>,
}

impl Log {
    /// An empty log of the thread stream number `n`.
    pub fn thread(n: u64) -> Fx<Self> {
        Ok(Self::of(thread_stream(n)?, n))
    }

    /// An empty log of the matter stream number `n`.
    pub fn matter(n: u64) -> Fx<Self> {
        Ok(Self::of(matter_stream(n)?, n))
    }

    fn of(stream: StreamId, n: u64) -> Self {
        Self {
            stream,
            next_id: n.saturating_mul(ID_SPACE).saturating_add(1),
            events: Vec::new(),
        }
    }

    /// The stream.
    pub const fn stream(&self) -> StreamId {
        self.stream
    }

    /// A number no id of this log has used yet.
    pub fn fresh_number(&mut self) -> u64 {
        let n = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        n
    }

    /// A v7 id of any type that no id of this log has used yet.
    pub fn fresh<T: FromStr<Err = Error>>(&mut self) -> Fx<T> {
        let n = self.fresh_number();
        id(n)
    }

    /// The seq the next pushed event gets.
    pub fn next_seq(&self) -> u64 {
        u64::try_from(self.events.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1)
    }

    /// Wraps `event` in the next envelope, records it and returns a copy.
    pub fn push(&mut self, event: impl Into<DomainEvent>) -> Fx<Envelope> {
        let seq = self.next_seq();
        let millis = i64::try_from(seq.saturating_mul(1000)).unwrap_or(i64::MAX);
        let envelope = Envelope::new(
            self.fresh()?,
            self.stream,
            Seq::new(seq)?,
            Timestamp::new(millis)?,
            None,
            event.into(),
        );
        self.events.push(envelope.clone());
        Ok(envelope)
    }

    /// The envelopes so far, in seq order.
    pub fn events(&self) -> &[Envelope] {
        &self.events
    }

    /// The number of envelopes so far.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether nothing is logged yet.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The envelopes, in seq order.
    pub fn into_events(self) -> Vec<Envelope> {
        self.events
    }
}

/// `fake_agent`'s `prompt-stream` scenario as the host logs it: a thread, one prompt, three thought
/// chunks on one message, five answer chunks on another, and an `EndTurn` with its checkpoint.
pub fn prompt_stream() -> Fx<Vec<Envelope>> {
    let mut log = Log::thread(2)?;
    log.push(thread_created()?)?;
    let turn = start_turn(&mut log, "Summarise the indemnity clause.")?;
    let thought = log.fresh()?;
    for chunk in ["Reading the ", "indemnity clause ", "first."] {
        log.push(text(turn, thought, TextKind::Thought, chunk.to_owned()))?;
    }
    let answer = log.fresh()?;
    let chunks = [
        "The clause ",
        "caps liability ",
        "at the fees ",
        "paid in the ",
        "last twelve months.",
    ];
    for chunk in chunks {
        log.push(text(turn, answer, TextKind::Answer, chunk.to_owned()))?;
    }
    let checkpoint_after = log.fresh()?;
    log.push(turn_ended(turn, checkpoint_after)?)?;
    Ok(log.into_events())
}

/// One busy turn, [`SCRIPTED_TURN_EVENTS`] events: 500 text chunks of 40 bytes over five
/// messages (the first a thought) between 20 tool calls (started, a status update, a content
/// update with a diff), three plans, a permission asked and answered during the ninth tool
/// call, then usage, files and the turn's end.
pub fn scripted_turn() -> Fx<Vec<Envelope>> {
    let mut log = Log::thread(3)?;
    log.push(thread_created()?)?;
    let turn = start_turn(&mut log, "Review the lease and fix the indemnity clauses.")?;
    let mut message = log.fresh()?;
    for call in 0..20 {
        for chunk in 0..25 {
            let index = call * 25 + chunk;
            if index > 0 && index % 100 == 0 {
                message = log.fresh()?;
            }
            let kind = if index < 100 {
                TextKind::Thought
            } else {
                TextKind::Answer
            };
            log.push(text(
                turn,
                message,
                kind,
                format!("Chunk {index:04} of the scripted answer text. "),
            ))?;
        }
        let tool_call = log.fresh()?;
        log.push(tool_started(turn, tool_call, call)?)?;
        if call == 8 {
            let request = log.fresh()?;
            log.push(permission_requested(request, Some(tool_call)))?;
            log.push(permission_resolved(request))?;
        }
        log.push(tool_diff(tool_call, call)?)?;
        log.push(tool_status(tool_call, ToolCallStatus::Completed))?;
        if call % 5 == 4 && call < 19 {
            log.push(plan(call / 5 + 1))?;
        }
    }
    log.push(usage(42_000)?)?;
    log.push(files_changed(turn)?)?;
    let checkpoint_after = log.fresh()?;
    log.push(turn_ended(turn, checkpoint_after)?)?;
    Ok(log.into_events())
}

/// The events of one closed turn of [`long_thread`]: prompt, start, `chunks` text chunks on one
/// answer, `tools` tool calls (started, completed), end.
fn long_turn(log: &mut Log, number: usize, chunks: usize, tools: usize) -> Fx<Vec<DomainEvent>> {
    let turn: TurnId = log.fresh()?;
    let message = log.fresh()?;
    let checkpoint_before = log.fresh()?;
    let answer = log.fresh()?;
    let mut events = vec![
        user_message(
            turn,
            message,
            &format!("Question {number} about the lease."),
        ),
        TurnStarted {
            turn,
            checkpoint_before,
        }
        .into(),
    ];
    for _ in 0..chunks {
        events.push(text(turn, answer, TextKind::Answer, prose(40)));
    }
    for tool in 0..tools {
        let tool_call = log.fresh()?;
        events.push(tool_started(turn, tool_call, tool)?);
        events.push(tool_status(tool_call, ToolCallStatus::Completed));
    }
    events.push(turn_ended(turn, log.fresh()?)?);
    Ok(events)
}

/// The log behind [`long_thread`].
pub fn long_thread_log(n: usize) -> Fx<Log> {
    let mut log = Log::thread(4)?;
    if n == 0 {
        return Ok(log);
    }
    log.push(thread_created()?)?;
    let mut number = 0;
    while log.len() < n {
        let remaining = n - log.len();
        let tools = 1 + number % 2;
        let fixed = 3 + 2 * tools;
        let usual = fixed + LONG_TURN_CHUNKS;
        // The largest turn is LONG_TURN_CHUNKS + 7 events; keeping that much back means the
        // last turn always has room for its fixed events once n is past one turn.
        let chunks = if remaining < usual + LONG_TURN_CHUNKS + 7 {
            remaining.saturating_sub(fixed)
        } else {
            LONG_TURN_CHUNKS
        };
        for event in long_turn(&mut log, number, chunks, tools)?
            .into_iter()
            .take(remaining)
        {
            log.push(event)?;
        }
        number += 1;
    }
    Ok(log)
}

/// A thread of exactly `n` events: `ThreadCreated`, then closed turns of a prompt, ten text
/// chunks, one or two tool calls and an end; the last turn takes the remainder as extra chunks.
/// Below 6 events the one turn is cut short.
pub fn long_thread(n: usize) -> Fx<Vec<Envelope>> {
    Ok(long_thread_log(n)?.into_events())
}

/// [`long_thread`] plus an open turn: a prompt, its start, an answer of `answer_bytes` in
/// chunks of up to 4 KiB, and a running tool call. Its methods build the next event.
#[derive(Debug, Clone)]
pub struct OpenTurn {
    /// The log, open turn included.
    pub log: Log,
    /// The open turn.
    pub turn: TurnId,
    /// The answer being streamed.
    pub answer: MessageId,
    /// The running tool call.
    pub tool_call: ToolCallId,
}

impl OpenTurn {
    /// `long_thread(n)` with an open turn whose answer holds `answer_bytes`.
    pub fn on_long_thread(n: usize, answer_bytes: usize) -> Fx<Self> {
        let mut log = long_thread_log(n)?;
        let turn = start_turn(&mut log, "Now redraft the indemnity clause.")?;
        let answer = log.fresh()?;
        let mut left = answer_bytes;
        while left > 0 {
            let len = left.min(4096);
            log.push(text(turn, answer, TextKind::Answer, prose(len)))?;
            left -= len;
        }
        let tool_call = log.fresh()?;
        log.push(tool_started(turn, tool_call, 0)?)?;
        Ok(Self {
            log,
            turn,
            answer,
            tool_call,
        })
    }

    /// 40 more bytes of the answer.
    pub fn text_appended(&self) -> DomainEvent {
        text(self.turn, self.answer, TextKind::Answer, prose(40))
    }

    /// New content for the running tool call.
    pub fn tool_call_updated(&self) -> Fx<DomainEvent> {
        tool_diff(self.tool_call, 0)
    }

    /// A request to allow the running tool call.
    pub fn permission_requested(&mut self) -> Fx<DomainEvent> {
        let request = self.log.fresh()?;
        Ok(permission_requested(request, Some(self.tool_call)))
    }

    /// The open turn's end.
    pub fn turn_ended(&mut self) -> Fx<DomainEvent> {
        let checkpoint_after = self.log.fresh()?;
        turn_ended(self.turn, checkpoint_after)
    }
}

/// The fixture matter's stream: created from a folder import, a checkpoint of every kind with
/// the thread and turn each kind names, outside edits filling their checkpoint, and a restore.
pub fn matter_versions() -> Fx<Vec<Envelope>> {
    let mut log = Log::matter(MATTER)?;
    let import = ImportSummary {
        files: 12,
        bytes: Count::new(4_000_000)?,
        cloud_only: 1,
    };
    log.push(MatterCreated::new(
        "Smith v Jones",
        MatterKind::Folder,
        Some(import),
    )?)?;
    let thread = thread_id(2u64.saturating_mul(ID_SPACE))?;
    let turn: TurnId = log.fresh()?;
    let imported = log.fresh()?;
    let before = log.fresh()?;
    let checkpoints = [
        (imported, CheckpointKind::Import, None, None),
        (before, CheckpointKind::BeforeTurn, Some(thread), Some(turn)),
        (
            log.fresh()?,
            CheckpointKind::AfterTurn,
            Some(thread),
            Some(turn),
        ),
    ];
    for (checkpoint, kind, thread, turn) in checkpoints {
        log.push(CheckpointRecorded::new(checkpoint, kind, thread, turn)?)?;
    }
    // Outside edits before an undo of the turn name its thread and step.
    let outside = log.fresh()?;
    let kind = CheckpointKind::OutsideEdits;
    log.push(CheckpointRecorded::new(
        outside,
        kind,
        Some(thread),
        Some(turn),
    )?)?;
    log.push(OutsideEdits {
        checkpoint: outside,
        files: file_changes()?,
    })?;
    let undo = log.fresh()?;
    log.push(CheckpointRecorded::new(
        undo,
        CheckpointKind::Undo,
        Some(thread),
        Some(turn),
    )?)?;
    // Outside edits before a restore name nothing.
    let outside = log.fresh()?;
    log.push(CheckpointRecorded::new(
        outside,
        CheckpointKind::OutsideEdits,
        None,
        None,
    )?)?;
    let restore = log.fresh()?;
    log.push(CheckpointRecorded::new(
        restore,
        CheckpointKind::Restore,
        None,
        None,
    )?)?;
    log.push(VersionRestored {
        from: before,
        checkpoint: restore,
        scope: RestoreScope::Files {
            paths: vec![MatterPath::new("lease.docx")?],
        },
    })?;
    Ok(log.into_events())
}
