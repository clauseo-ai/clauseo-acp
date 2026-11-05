//! The queued prompts' order (decision record 0048, P4b): a turn put back at the head (its
//! `TurnQueued` is `at_head`) goes first, the latest put back first, as the host runs them;
//! the rest keep `order`. A log with no `at_head` orders exactly by `order`.

use clauseo_protocol::{
    DomainEvent, MessageId, ThreadId, TurnId, TurnQueued, TurnStarted, ViewEntry,
};
use clauseo_reducer::{StreamState, apply, queued_order};

use crate::fixtures::{Log, thread_created, turn_ended, user_message};
use crate::helpers::{fold, round_trip};

/// A thread whose first turn runs with "Two" and "Three" queued behind it, then ends; returns
/// the log, the state, the thread and the queued prompts' messages.
#[cfg(test)]
fn two_queued() -> (Log, StreamState, ThreadId, TurnId, [MessageId; 2]) {
    let mut log = Log::thread(31).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let mut state = fold(log.events());
    let thread = state.as_thread().unwrap().thread();
    let (first, _) = prompt(&mut log, &mut state, "One");
    let checkpoint_before = log.fresh().unwrap();
    let started = TurnStarted {
        turn: first,
        checkpoint_before,
    };
    push(&mut log, &mut state, started);
    let mut messages = Vec::new();
    for text in ["Two", "Three"] {
        let (turn, message) = prompt(&mut log, &mut state, text);
        push(&mut log, &mut state, queued(thread, turn, first, false));
        messages.push(message);
    }
    let checkpoint = log.fresh().unwrap();
    push(&mut log, &mut state, turn_ended(first, checkpoint).unwrap());
    (log, state, thread, first, [messages[0], messages[1]])
}

#[cfg(test)]
fn push(log: &mut Log, state: &mut StreamState, event: impl Into<DomainEvent>) {
    let applied = apply(state, &log.push(event).unwrap()).unwrap();
    assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
}

#[cfg(test)]
fn prompt(log: &mut Log, state: &mut StreamState, text: &str) -> (TurnId, MessageId) {
    let (turn, message) = (log.fresh().unwrap(), log.fresh().unwrap());
    push(log, state, user_message(turn, message, text));
    (turn, message)
}

/// `turn` queued behind `behind` of its own thread, put back at the head when `at_head`.
#[cfg(test)]
fn queued(thread: ThreadId, turn: TurnId, behind: TurnId, at_head: bool) -> TurnQueued {
    let waits = TurnQueued::new(thread, turn, thread, Some(behind)).unwrap();
    if at_head { waits.at_head() } else { waits }
}

#[cfg(test)]
fn order(state: &StreamState) -> Vec<MessageId> {
    state.as_thread().unwrap().queued_order()
}

#[test]
fn a_turn_put_back_at_the_head_goes_first() {
    let (mut log, mut state, thread, first, [two, three]) = two_queued();
    let (again, again_message) = prompt(&mut log, &mut state, "One");
    push(&mut log, &mut state, queued(thread, again, first, true));
    assert_eq!(order(&state), [again_message, two, three]);
    let prompt = state.entries().values().find_map(|entry| match entry {
        ViewEntry::UserMessage(prompt) if prompt.message == again_message => Some(prompt),
        _ => None,
    });
    assert!(prompt.is_some_and(|prompt| prompt.at_head));
    // A snapshot keeps the mark, and a fold from the log agrees.
    assert_eq!(order(&round_trip(&state)), [again_message, two, three]);
    assert_eq!(order(&fold(log.events())), [again_message, two, three]);
}

#[test]
fn the_latest_put_back_goes_first() {
    let (mut log, mut state, thread, first, [two, three]) = two_queued();
    let (back_1, message_1) = prompt(&mut log, &mut state, "Back one");
    push(&mut log, &mut state, queued(thread, back_1, first, true));
    let (back_2, message_2) = prompt(&mut log, &mut state, "Back two");
    push(&mut log, &mut state, queued(thread, back_2, first, true));
    assert_eq!(order(&state), [message_2, message_1, two, three]);
}

#[test]
fn without_at_head_the_queue_keeps_order() {
    let (mut log, mut state, thread, first, [two, three]) = two_queued();
    let (later, later_message) = prompt(&mut log, &mut state, "Four");
    push(&mut log, &mut state, queued(thread, later, first, false));
    assert_eq!(order(&state), [two, three, later_message]);
    // The free function reads any entries, a view window's included, in any order.
    let mut entries: Vec<&ViewEntry> = state.entries().values().collect();
    entries.reverse();
    assert_eq!(queued_order(entries), [two, three, later_message]);
    // Nothing is marked, so the entries serialise as before.
    let json = serde_json::to_string(&state.entries().values().collect::<Vec<_>>()).unwrap();
    assert!(!json.contains("at_head"), "{json}");
}
