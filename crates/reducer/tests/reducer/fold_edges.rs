//! Edges of the thread fold: the end of a turn that never started, and events that carry
//! nothing to change.

use clauseo_protocol::{
    ErrorCode, MessageId, StopReason, ThreadStatus, TurnFailed, TurnId, UsageUpdated,
    UserMessageStatus, ViewEntry,
};
use clauseo_reducer::{StreamState, ViewChange, apply};

use crate::fixtures::{
    Log, files_changed, start_turn, thread_created, turn_ended, usage, user_message,
};
use crate::helpers::fold;

/// The state's JSON with `seq` cleared, to compare states that folded different event counts.
#[cfg(test)]
fn without_seq(state: &StreamState) -> serde_json::Value {
    let mut value = serde_json::to_value(state).unwrap();
    value["thread"]["seq"] = serde_json::Value::Null;
    value
}

#[test]
fn turn_ended_of_unstarted_turn_drops_its_files_and_failure() {
    let mut log = Log::thread(11).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let created = fold(log.events());
    let turn = log.fresh().unwrap();
    log.push(files_changed(turn).unwrap()).unwrap();
    let failed = TurnFailed {
        turn,
        code: ErrorCode::ALL[0],
        detail: None,
    };
    log.push(failed).unwrap();
    let mut state = fold(log.events());
    // Kept for the turn's step until its end.
    let held = without_seq(&state);
    assert_ne!(held["thread"]["files"], serde_json::json!({}));
    assert_ne!(held["thread"]["failures"], serde_json::json!({}));

    let checkpoint = log.fresh().unwrap();
    let ended = log.push(turn_ended(turn, checkpoint).unwrap()).unwrap();
    let out = apply(&mut state, &ended).unwrap();
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    let steps = state.entries().values();
    assert!(!steps.clone().any(|e| matches!(e, ViewEntry::Step(_))));
    assert_eq!(state.as_thread().unwrap().status(), ThreadStatus::Idle);
    // Dropped: the state is the created thread's but for seq.
    assert_eq!(without_seq(&state), without_seq(&created));
}

/// The status of the user message `message` in `state`.
#[cfg(test)]
fn prompt_status(state: &StreamState, message: MessageId) -> Option<UserMessageStatus> {
    state.entries().values().find_map(|entry| match entry {
        ViewEntry::UserMessage(prompt) if prompt.message == message => Some(prompt.status),
        _ => None,
    })
}

/// A `TurnEnded` cancelled, of `turn`, pushed and applied.
#[cfg(test)]
fn cancel(log: &mut Log, state: &mut StreamState, turn: TurnId) -> clauseo_reducer::Applied {
    let checkpoint = log.fresh().unwrap();
    let mut ended = turn_ended(turn, checkpoint).unwrap();
    if let clauseo_protocol::DomainEvent::TurnEnded(event) = &mut ended {
        event.stop = StopReason::Cancelled;
        event.checkpoint_after = None;
        event.usage = None;
    }
    apply(state, &log.push(ended).unwrap()).unwrap()
}

#[test]
fn turn_ended_before_start_settles_and_removes_prompt() {
    // A prompt whose turn is cancelled before it starts, with no other turn running: the
    // thread settles to Idle (not Starting for good) and the prompt is Removed (0002).
    let mut log = Log::thread(11).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let (turn, message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(turn, message, "Draft the notice."))
        .unwrap();
    let mut state = fold(log.events());
    assert_eq!(state.as_thread().unwrap().status(), ThreadStatus::Starting);
    let out = cancel(&mut log, &mut state, turn);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(state.as_thread().unwrap().status(), ThreadStatus::Idle);
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Removed)
    );
    let removed = out.changes.iter().any(|change| {
        matches!(change, ViewChange::Upsert(entry)
            if matches!(&**entry, ViewEntry::UserMessage(p) if p.status == UserMessageStatus::Removed))
    });
    assert!(removed, "{:?}", out.changes);
    let steps = state.entries().values();
    assert!(!steps.clone().any(|e| matches!(e, ViewEntry::Step(_))));
    assert_eq!(state, fold(log.events()));

    // A prompt sent while a turn runs, whose turn is cancelled before it starts: Removed, and
    // the running turn keeps the status.
    let running = start_turn(&mut log, "Check clause 2.").unwrap();
    let (queued, prompt): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(queued, prompt, "Then clause 3."))
        .unwrap();
    let mut state = fold(log.events());
    let out = cancel(&mut log, &mut state, queued);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(state.as_thread().unwrap().status(), ThreadStatus::Running);
    assert_eq!(
        prompt_status(&state, prompt),
        Some(UserMessageStatus::Removed)
    );
    let first = log
        .events()
        .iter()
        .find_map(|envelope| match &envelope.event {
            clauseo_protocol::DomainEvent::UserMessage(event) if event.turn == running => {
                Some(event.message)
            }
            _ => None,
        });
    assert_eq!(
        prompt_status(&state, first.unwrap()),
        Some(UserMessageStatus::Sent)
    );
}

#[test]
fn usage_updated_without_fields_changes_nothing() {
    let mut log = Log::thread(11).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let mut state = fold(log.events());
    let empty = || UsageUpdated {
        context_used: None,
        context_size: None,
        cost: None,
    };
    let out = apply(&mut state, &log.push(empty()).unwrap()).unwrap();
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(state.as_thread().unwrap().header().unwrap().usage, None);

    apply(&mut state, &log.push(usage(900).unwrap()).unwrap()).unwrap();
    let before = state.as_thread().unwrap().header().unwrap().usage.clone();
    let out = apply(&mut state, &log.push(empty()).unwrap()).unwrap();
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(state.as_thread().unwrap().header().unwrap().usage, before);
}
