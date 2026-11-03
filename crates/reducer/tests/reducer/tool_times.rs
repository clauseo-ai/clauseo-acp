//! Tool call times on entries: `started_at` is the `ts` of the `tool_call_started` envelope that
//! opened the entry, and `finished_at` the `ts` of the first event that gave the call a final
//! status. Both come from the envelopes, so a replay and a snapshot keep them.

use clauseo_protocol::{
    DomainEvent, Envelope, Seq, ToolCallEntry, ToolCallId, ToolCallStatus, TurnId, ViewEntry,
};
use clauseo_reducer::{StreamState, ViewChange, apply};

use crate::events::{tool_diff, tool_started, tool_status};
use crate::fixtures::{Fx, Log, start_turn, thread_created};
use crate::helpers::{fold, round_trip};
use crate::times::{clock, retimed};

/// The entry of `call` in `state`.
#[cfg(test)]
fn tool(state: &StreamState, call: ToolCallId) -> ToolCallEntry {
    state
        .entries()
        .values()
        .find_map(|entry| match entry {
            ViewEntry::ToolCall(found) if found.tool_call == call => Some(found.as_ref().clone()),
            _ => None,
        })
        .expect("the state holds the tool call")
}

/// A thread with one turn, and the log of it, which the caller adds to.
#[cfg(test)]
fn turn_log(n: u64) -> (Log, TurnId) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Redline the lease.").unwrap();
    (log, turn)
}

/// The seq of the event that pushed `envelope`.
#[cfg(test)]
fn seq_of(envelope: Fx<Envelope>) -> Seq {
    envelope.unwrap().seq
}

#[test]
fn started_at_is_the_start_event_time_and_finished_at_is_the_first_final_status() {
    let (mut log, turn) = turn_log(41);
    let call = log.fresh().unwrap();
    let started = seq_of(log.push(tool_started(turn, call, 1).unwrap()));
    let running = log
        .push(tool_status(call, ToolCallStatus::InProgress))
        .unwrap()
        .seq;
    let done = seq_of(log.push(tool_status(call, ToolCallStatus::Completed)));
    // A later update that is not a status change, and a later status, keep the first finish.
    let diff = seq_of(log.push(tool_diff(call, 1).unwrap()));
    let failed = seq_of(log.push(tool_status(call, ToolCallStatus::Failed)));
    let events = retimed(log.into_events());
    let at = |seq: Seq| {
        let upto = events.iter().position(|e| e.seq == seq).unwrap() + 1;
        fold(&events[..upto])
    };
    let entry = tool(&at(started), call);
    assert_eq!(entry.started_at, clock(started));
    assert_eq!(entry.finished_at, None);
    let entry = tool(&at(running), call);
    assert_eq!(
        (entry.started_at, entry.finished_at),
        (clock(started), None)
    );
    let entry = tool(&at(done), call);
    assert_eq!(entry.finished_at, Some(clock(done)));
    assert_eq!(entry.started_at, clock(started));
    assert_ne!(clock(done), clock(started));
    let entry = tool(&at(diff), call);
    assert_eq!(entry.finished_at, Some(clock(done)));
    let entry = tool(&at(failed), call);
    assert_eq!(entry.status, ToolCallStatus::Failed);
    assert_eq!(entry.finished_at, Some(clock(done)));
}

#[test]
fn a_finish_is_sent_in_the_upsert_of_the_update_that_gave_it() {
    let (mut log, turn) = turn_log(42);
    let call = log.fresh().unwrap();
    log.push(tool_started(turn, call, 2).unwrap()).unwrap();
    let done = log
        .push(tool_status(call, ToolCallStatus::Completed))
        .unwrap()
        .seq;
    let events = retimed(log.into_events());
    let (last, before) = events.split_last().unwrap();
    assert_eq!(last.seq, done);
    let mut state = fold(before);
    let applied = apply(&mut state, last).unwrap();
    let sent = applied.changes.iter().find_map(|change| match change {
        ViewChange::Upsert(entry) => match entry.as_ref() {
            ViewEntry::ToolCall(found) => Some(found.finished_at),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(sent, Some(Some(clock(last.seq))), "{applied:?}");
}

#[test]
fn a_call_that_starts_final_is_finished_at_its_start() {
    let (mut log, turn) = turn_log(43);
    let call = log.fresh().unwrap();
    let mut started = match tool_started(turn, call, 3).unwrap() {
        DomainEvent::ToolCallStarted(started) => started,
        other => panic!("not a tool call start: {other:?}"),
    };
    started.status = ToolCallStatus::Failed;
    let seq = seq_of(log.push(started));
    let state = fold(&retimed(log.into_events()));
    let entry = tool(&state, call);
    assert_eq!(entry.started_at, clock(seq));
    assert_eq!(entry.finished_at, Some(clock(seq)));
}

#[test]
fn replays_and_snapshots_keep_both_times() {
    let (mut log, turn) = turn_log(44);
    let call = log.fresh().unwrap();
    let started = seq_of(log.push(tool_started(turn, call, 4).unwrap()));
    log.push(tool_status(call, ToolCallStatus::Completed))
        .unwrap();
    let events = retimed(log.into_events());
    let state = fold(&events);
    let replayed = fold(&events);
    assert_eq!(tool(&replayed, call), tool(&state, call));
    let read = round_trip(&state);
    assert_eq!(read, state);
    let entry = tool(&read, call);
    assert_eq!(entry.started_at, clock(started));
    assert!(entry.finished_at.is_some(), "{entry:?}");
}
