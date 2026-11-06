//! The idle stop in the fold: an `IdleStopped` detach shows `IdleStopped` (once the running turn
//! ends, when one runs), the next attach leaves `IdleStopped` for the settled status, and a log
//! replayed from any snapshot rebuilds the state folded from its start.

use clauseo_protocol::{
    AgentCapabilities, AgentSessionAttached, AgentSessionDetached, DetachReason, DomainEvent,
    ThreadStatus, TurnStarted, ViewEntry,
};
use clauseo_reducer::{Applied, StreamState, ViewChange, apply, view_snapshot};

use crate::fixtures::{Log, start_turn, thread_created, turn_ended, user_message};
use crate::helpers::{apply_all, empty, fold, round_trip};

/// Pushes `event` and applies it, returning what the apply gave.
#[cfg(test)]
fn push(log: &mut Log, state: &mut StreamState, event: impl Into<DomainEvent>) -> Applied {
    let envelope = log.push(event).unwrap();
    apply(state, &envelope).unwrap()
}

/// The thread's status in `state`.
#[cfg(test)]
fn status(state: &StreamState) -> ThreadStatus {
    state.as_thread().unwrap().status()
}

/// Whether `applied` has a header `Upsert` showing `to`.
fn shows_status(applied: &Applied, to: &ThreadStatus) -> bool {
    applied.changes.iter().any(|change| {
        matches!(change, ViewChange::Upsert(entry)
            if matches!(&**entry, ViewEntry::Header(h) if h.status == *to))
    })
}

/// A detach of the agent session for `reason`.
fn detach(reason: DetachReason) -> AgentSessionDetached {
    AgentSessionDetached { reason }
}

/// An attach of a new agent session that has no capabilities.
#[cfg(test)]
fn attach() -> AgentSessionAttached {
    AgentSessionAttached {
        session: "sess-1".parse().unwrap(),
        agent_version: None,
        capabilities: AgentCapabilities::default(),
    }
}

#[test]
fn idle_stop_with_no_turn_shows_idle_stopped() {
    let mut log = Log::thread(40).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let mut state = fold(log.events());
    let applied = push(&mut log, &mut state, detach(DetachReason::IdleStopped));
    assert_eq!(status(&state), ThreadStatus::IdleStopped);
    assert!(shows_status(&applied, &ThreadStatus::IdleStopped));
    assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
}

#[test]
fn idle_stop_during_a_turn_shows_once_the_turn_ends() {
    let mut log = Log::thread(41).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Go.").unwrap();
    let mut state = fold(log.events());
    assert_eq!(status(&state), ThreadStatus::Running);

    let applied = push(&mut log, &mut state, detach(DetachReason::IdleStopped));
    assert_eq!(status(&state), ThreadStatus::Running);
    assert!(!shows_status(&applied, &ThreadStatus::IdleStopped));
    assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);

    let after = log.fresh().unwrap();
    let applied = push(&mut log, &mut state, turn_ended(turn, after).unwrap());
    assert_eq!(status(&state), ThreadStatus::IdleStopped);
    assert!(shows_status(&applied, &ThreadStatus::IdleStopped));
    assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
}

#[test]
fn attach_during_a_turn_drops_the_stop_held_for_its_end() {
    let mut log = Log::thread(46).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Go.").unwrap();
    let mut state = fold(log.events());
    push(&mut log, &mut state, detach(DetachReason::IdleStopped));
    assert_eq!(status(&state), ThreadStatus::Running);

    // The new session is live, so the held stop is over: the turn's end settles to `Idle`.
    let applied = push(&mut log, &mut state, attach());
    assert_eq!(status(&state), ThreadStatus::Running);
    assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);

    let after = log.fresh().unwrap();
    let applied = push(&mut log, &mut state, turn_ended(turn, after).unwrap());
    assert_eq!(status(&state), ThreadStatus::Idle);
    assert!(shows_status(&applied, &ThreadStatus::Idle));
    assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
}

#[test]
fn attach_after_idle_stop_settles_to_idle() {
    let mut log = Log::thread(42).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let mut state = fold(log.events());
    push(&mut log, &mut state, detach(DetachReason::IdleStopped));
    assert_eq!(status(&state), ThreadStatus::IdleStopped);

    let applied = push(&mut log, &mut state, attach());
    assert_eq!(status(&state), ThreadStatus::Idle);
    assert!(shows_status(&applied, &ThreadStatus::Idle));
    assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);

    // Settled now: a further attach with the same capabilities changes nothing.
    let applied = push(&mut log, &mut state, attach());
    assert_eq!(status(&state), ThreadStatus::Idle);
    assert!(applied.changes.is_empty(), "{:?}", applied.changes);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn attach_after_an_exit_keeps_agent_exited() {
    let mut log = Log::thread(43).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let mut state = fold(log.events());
    let exited = DetachReason::Exited {
        code: Some(1),
        stderr_tail: String::new(),
    };
    push(&mut log, &mut state, detach(exited));
    assert_eq!(status(&state), ThreadStatus::AgentExited);
    push(&mut log, &mut state, attach());
    assert_eq!(status(&state), ThreadStatus::AgentExited);
}

#[test]
fn resume_after_idle_stop_reads_like_the_fold_without_it() {
    // The status after each event of a resume: a prompt, an attach, the turn's start and end.
    // Only the stop itself differs; once a prompt is in, the statuses are the same.
    let statuses = |stop: bool| {
        let mut log = Log::thread(44).unwrap();
        log.push(thread_created().unwrap()).unwrap();
        let mut state = fold(log.events());
        if stop {
            push(&mut log, &mut state, detach(DetachReason::IdleStopped));
        }
        let mut seen = vec![status(&state)];
        let turn = log.fresh().unwrap();
        let message = log.fresh().unwrap();
        push(&mut log, &mut state, user_message(turn, message, "Go."));
        seen.push(status(&state));
        push(&mut log, &mut state, attach());
        seen.push(status(&state));
        let checkpoint_before = log.fresh().unwrap();
        push(
            &mut log,
            &mut state,
            TurnStarted {
                turn,
                checkpoint_before,
            },
        );
        seen.push(status(&state));
        let after = log.fresh().unwrap();
        push(&mut log, &mut state, turn_ended(turn, after).unwrap());
        seen.push(status(&state));
        assert_eq!(state, fold(log.events()));
        seen
    };
    let with_stop = statuses(true);
    let without = statuses(false);
    assert_eq!(with_stop[0], ThreadStatus::IdleStopped);
    assert_eq!(without[0], ThreadStatus::Idle);
    assert_eq!(with_stop[1..], without[1..]);
    assert_eq!(with_stop[4], ThreadStatus::Idle);
}

#[test]
fn replayed_idle_stop_rebuilds_the_same_state_from_any_snapshot() {
    let mut log = Log::thread(45).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Go.").unwrap();
    log.push(detach(DetachReason::IdleStopped)).unwrap();
    let after = log.fresh().unwrap();
    log.push(turn_ended(turn, after).unwrap()).unwrap();
    log.push(attach()).unwrap();
    let events = log.events();
    let folded = fold(events);
    assert_eq!(status(&folded), ThreadStatus::Idle);
    let window = view_snapshot(&folded);

    // Event by event from the empty state, which is the replay itself.
    let mut replayed = empty(events);
    apply_all(&mut replayed, events);
    assert_eq!(replayed, folded);

    // A snapshot cut at every point, then the tail: the same state and the same view.
    for cut in 1..events.len() {
        let mut head = empty(events);
        apply_all(&mut head, &events[..cut]);
        let mut state = round_trip(&head);
        apply_all(&mut state, &events[cut..]);
        assert_eq!(state, folded, "cut after {cut} events");
        assert_eq!(view_snapshot(&state), window, "cut after {cut} events");
    }
}
