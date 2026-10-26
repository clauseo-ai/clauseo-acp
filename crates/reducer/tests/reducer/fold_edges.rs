//! Edges of the thread fold: the end of a turn that never started, and events that carry
//! nothing to change.

use clauseo_protocol::{ErrorCode, ThreadStatus, TurnFailed, UsageUpdated, ViewEntry};
use clauseo_reducer::{StreamState, apply};

use crate::fixtures::{Log, files_changed, thread_created, turn_ended, usage};
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
