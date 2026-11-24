//! The agent's wait to retry on the header (`retrying`): `AgentRetrying` sets it, its `since`
//! from the first of a wait and its words from the last; `AgentRetryEnded`, the turn's end, a
//! new turn's start and a detach clear it; with none set, none of them touches the header.

use clauseo_protocol::{
    AgentRetry, AgentRetryEnded, AgentRetrying, AgentSessionDetached, DetachReason, DomainEvent,
    HeaderEntry, TurnId, ViewEntry,
};
use clauseo_reducer::{StreamState, ViewChange, apply};

use crate::fixtures::{Log, start_turn, thread_created, turn_ended};
use crate::helpers::{fold, round_trip};

#[cfg(test)]
fn retrying(turn: TurnId, title: &str, details: &str) -> DomainEvent {
    AgentRetrying {
        turn,
        title: Some(title.to_owned()),
        details: Some(details.to_owned()),
    }
    .into()
}

#[cfg(test)]
fn header(state: &StreamState) -> &HeaderEntry {
    state.as_thread().unwrap().header().unwrap()
}

/// The header upserts `event` makes on `state`, as the next of `log`.
#[cfg(test)]
fn upserts(log: &mut Log, state: &mut StreamState, event: DomainEvent) -> Vec<HeaderEntry> {
    let envelope = log.push(event).unwrap();
    let applied = apply(state, &envelope).unwrap();
    applied
        .changes
        .into_iter()
        .filter_map(|change| match change {
            ViewChange::Upsert(entry) => match *entry {
                ViewEntry::Header(header) => Some(*header),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// A thread with a running turn, and that turn.
#[cfg(test)]
fn running(n: u64) -> (Log, StreamState, TurnId) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Read the lease.").unwrap();
    let state = fold(log.events());
    (log, state, turn)
}

#[test]
fn a_retry_shows_on_the_header_until_the_wait_ends() {
    let (mut log, mut state, turn) = running(1);
    assert_eq!(header(&state).retrying, None);
    let first = retrying(
        turn,
        "Reconnecting... 1/5",
        "Selected model is at capacity.",
    );
    let set = upserts(&mut log, &mut state, first);
    let since = log.events().last().unwrap().ts;
    let shown = AgentRetry {
        since,
        title: Some("Reconnecting... 1/5".to_owned()),
        details: Some("Selected model is at capacity.".to_owned()),
    };
    assert_eq!(set.len(), 1, "one header upsert");
    assert_eq!(set[0].retrying.as_ref(), Some(&shown));
    // A second retry takes its words and keeps the wait's start.
    let second = retrying(
        turn,
        "Reconnecting... 2/5",
        "Selected model is at capacity.",
    );
    let set = upserts(&mut log, &mut state, second);
    assert_eq!(set.len(), 1);
    let again = set[0].retrying.clone().unwrap();
    assert_eq!(again.since, since);
    assert_ne!(log.events().last().unwrap().ts, since);
    assert_eq!(again.title.as_deref(), Some("Reconnecting... 2/5"));
    // The snapshot keeps it.
    assert_eq!(header(&round_trip(&state)).retrying, Some(again));
    let ended = upserts(&mut log, &mut state, AgentRetryEnded { turn }.into());
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].retrying, None);
    // With no wait, its end again changes nothing.
    let none = upserts(&mut log, &mut state, AgentRetryEnded { turn }.into());
    assert_eq!(none, Vec::<HeaderEntry>::new());
    assert_eq!(fold(log.events()), state, "the fold of the log agrees");
}

#[test]
fn the_turns_end_a_new_turn_and_a_detach_clear_the_wait() {
    let (mut log, mut state, turn) = running(2);
    upserts(&mut log, &mut state, retrying(turn, "T", "D"));
    let checkpoint = log.fresh().unwrap();
    upserts(&mut log, &mut state, turn_ended(turn, checkpoint).unwrap());
    assert_eq!(header(&state).retrying, None, "the turn's end clears it");

    let next = start_turn(&mut log, "And the deed?").unwrap();
    let mut state = fold(log.events());
    upserts(&mut log, &mut state, retrying(next, "T", "D"));
    assert!(header(&state).retrying.is_some());
    let detached = AgentSessionDetached {
        reason: DetachReason::Exited {
            code: None,
            stderr_tail: String::new(),
        },
    };
    upserts(&mut log, &mut state, detached.into());
    assert_eq!(header(&state).retrying, None, "a detach clears it");
    assert_eq!(fold(log.events()), state);
}

#[test]
fn a_new_turns_start_clears_a_wait_left_over() {
    let (mut log, _, turn) = running(3);
    log.push(retrying(turn, "T", "D")).unwrap();
    // A log whose turn never ended (a crash): the next turn's start clears the wait.
    start_turn(&mut log, "Again.").unwrap();
    let state = fold(log.events());
    assert_eq!(header(&state).retrying, None);
}
