//! A turn that ends before it starts says why (`TurnEnded.cause`), and its message settles by
//! that cause: `Removed` for a Stop, a removal, a sign-in or an install (whose copy is queued
//! again) and for a log with no cause; `NotSent` for a send now and for a held steer whose
//! answer stopped. The view keeps the cause on the message, written only when it is set.

use clauseo_protocol::{
    DomainEvent, MessageId, StopReason, ThreadStatus, TurnId, UnsentCause, UserMessageStatus,
    ViewEntry,
};
use clauseo_reducer::{Applied, StreamState, ViewChange, apply};

use crate::fixtures::{Log, start_turn, thread_created, turn_ended, user_message};
use crate::helpers::{fold, prompt_status, round_trip};

/// A thread with one prompt whose turn has not started.
#[cfg(test)]
fn queued_prompt() -> (Log, TurnId, MessageId) {
    let mut log = Log::thread(41).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let (turn, message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(turn, message, "Draft the notice."))
        .unwrap();
    (log, turn, message)
}

/// `turn` ends `Cancelled` before it started, with `cause`; pushed and applied.
#[cfg(test)]
fn end_before_start(
    log: &mut Log,
    state: &mut StreamState,
    turn: TurnId,
    cause: Option<UnsentCause>,
) -> Applied {
    let checkpoint = log.fresh().unwrap();
    let mut ended = turn_ended(turn, checkpoint).unwrap();
    if let DomainEvent::TurnEnded(event) = &mut ended {
        event.stop = StopReason::Cancelled;
        event.checkpoint_after = None;
        event.usage = None;
        event.cause = cause;
    }
    apply(state, &log.push(ended).unwrap()).unwrap()
}

/// The view's entry of `message`.
#[cfg(test)]
fn prompt_entry(state: &StreamState, message: MessageId) -> clauseo_protocol::UserMessageEntry {
    state
        .entries()
        .values()
        .find_map(|entry| match entry {
            ViewEntry::UserMessage(prompt) if prompt.message == message => Some(prompt.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn each_cause_settles_the_unstarted_prompt_by_its_status() {
    let cases = [
        (None, UserMessageStatus::Removed),
        (Some(UnsentCause::Stopped), UserMessageStatus::Removed),
        (Some(UnsentCause::Removed), UserMessageStatus::Removed),
        (Some(UnsentCause::SignIn), UserMessageStatus::Removed),
        (Some(UnsentCause::Update), UserMessageStatus::Removed),
        (Some(UnsentCause::Unknown), UserMessageStatus::Removed),
        (Some(UnsentCause::SentNow), UserMessageStatus::NotSent),
        (Some(UnsentCause::NotSent), UserMessageStatus::NotSent),
    ];
    for (cause, status) in cases {
        let (mut log, turn, message) = queued_prompt();
        let mut state = fold(log.events());
        let out = end_before_start(&mut log, &mut state, turn, cause);
        assert!(out.anomalies.is_empty(), "{cause:?}: {:?}", out.anomalies);
        assert_eq!(prompt_status(&state, message), Some(status), "{cause:?}");
        assert_eq!(prompt_entry(&state, message).unsent, cause, "{cause:?}");
        let upserted = out.changes.iter().any(|change| {
            matches!(change, ViewChange::Upsert(entry)
                if matches!(&**entry, ViewEntry::UserMessage(p)
                    if p.message == message && p.status == status && p.unsent == cause))
        });
        assert!(upserted, "{cause:?}: {:?}", out.changes);
        assert_eq!(state, fold(log.events()), "{cause:?}");
        assert_eq!(round_trip(&state), state, "{cause:?}");
    }
}

#[test]
fn an_unset_cause_is_not_written_on_the_message() {
    let (mut log, turn, message) = queued_prompt();
    let mut state = fold(log.events());
    end_before_start(&mut log, &mut state, turn, None);
    let json = serde_json::to_value(ViewEntry::UserMessage(prompt_entry(&state, message))).unwrap();
    assert_eq!(json["user_message"].get("unsent"), None, "{json}");

    let (mut log, turn, message) = queued_prompt();
    let mut state = fold(log.events());
    end_before_start(&mut log, &mut state, turn, Some(UnsentCause::SentNow));
    let json = serde_json::to_value(ViewEntry::UserMessage(prompt_entry(&state, message))).unwrap();
    assert_eq!(json["user_message"]["unsent"], "sent_now", "{json}");
}

#[test]
fn a_prompt_sent_now_over_a_running_turn_is_not_sent_and_the_turn_keeps_running() {
    let mut log = Log::thread(42).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let running = start_turn(&mut log, "Check clause 2.").unwrap();
    let (queued, message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(queued, message, "Then clause 3."))
        .unwrap();
    let mut state = fold(log.events());
    let out = end_before_start(&mut log, &mut state, queued, Some(UnsentCause::SentNow));
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(state.as_thread().unwrap().status(), ThreadStatus::Running);
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::NotSent)
    );
    assert_eq!(state.as_thread().unwrap().running(), Some(running));
    assert_eq!(state, fold(log.events()));
}
