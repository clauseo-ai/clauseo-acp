//! The resume cue in the fold: `resuming_since` is the `ts` of the prompt that moves an
//! idle-stopped thread to `Starting`, is cleared when the status leaves `Starting` or the next
//! attach comes, and is never set for a first start or a start after an exit. A replay gives the
//! same header.

use clauseo_protocol::{
    AgentSessionDetached, DetachReason, DomainEvent, Envelope, ThreadStatus, TurnStarted, ViewEntry,
};
use clauseo_reducer::{
    Applied, REDUCER_SCHEMA_VERSION, Snapshot, SnapshotRef, StreamState, ViewChange, apply,
};

use crate::fixtures::{Log, start_turn, thread_created, turn_ended, user_message};
use crate::helpers::{apply_all, cancel, empty, fold, round_trip};

/// Pushes `event` and applies it, returning its envelope and what the apply gave.
#[cfg(test)]
fn push(
    log: &mut Log,
    state: &mut StreamState,
    event: impl Into<DomainEvent>,
) -> (Envelope, Applied) {
    let envelope = log.push(event).unwrap();
    let applied = apply(state, &envelope).unwrap();
    (envelope, applied)
}

/// The status of the thread in `state`.
#[cfg(test)]
fn status(state: &StreamState) -> ThreadStatus {
    state.as_thread().unwrap().status()
}

/// The thread's `resuming_since` in `state`.
#[cfg(test)]
fn since(state: &StreamState) -> Option<clauseo_protocol::Timestamp> {
    state.as_thread().unwrap().header().unwrap().resuming_since
}

/// A log of thread `n`, created, and its state folded.
#[cfg(test)]
fn created(n: u64) -> (Log, StreamState) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let state = fold(log.events());
    (log, state)
}

/// An idle stop: the session detached with `IdleStopped`.
#[cfg(test)]
fn idle_stop(log: &mut Log, state: &mut StreamState) {
    push(
        log,
        state,
        AgentSessionDetached {
            reason: DetachReason::IdleStopped,
        },
    );
}

/// Sends a prompt, which moves the thread to `Starting` when no turn runs. Returns its turn,
/// its envelope and what its apply gave.
#[cfg(test)]
fn send(
    log: &mut Log,
    state: &mut StreamState,
    text: &str,
) -> (clauseo_protocol::TurnId, Envelope, Applied) {
    let turn = log.fresh().unwrap();
    let message = log.fresh().unwrap();
    let (envelope, applied) = push(log, state, user_message(turn, message, text));
    (turn, envelope, applied)
}

#[test]
fn a_prompt_after_an_idle_stop_sets_it_to_its_own_ts() {
    let (mut log, mut state) = created(50);
    idle_stop(&mut log, &mut state);
    assert_eq!(since(&state), None);
    let (_, prompt, _) = send(&mut log, &mut state, "Go.");
    assert_eq!(status(&state), ThreadStatus::Starting);
    assert_eq!(since(&state), Some(prompt.ts));
    assert_eq!(state, fold(log.events()));
}

#[test]
fn setting_it_is_one_header_upsert() {
    let (mut log, mut state) = created(51);
    idle_stop(&mut log, &mut state);
    let (_, _, applied) = send(&mut log, &mut state, "Go.");
    let upserts = applied
        .changes
        .iter()
        .filter(|change| {
            matches!(change, ViewChange::Upsert(entry)
                if matches!(entry.as_ref(), ViewEntry::Header(h) if h.resuming_since.is_some()))
        })
        .count();
    assert_eq!(upserts, 1, "one header upsert: {applied:?}");
}

#[test]
fn an_attach_clears_it_and_the_turn_start_carries_on() {
    let (mut log, mut state) = created(52);
    idle_stop(&mut log, &mut state);
    let (turn, _, _) = send(&mut log, &mut state, "Go.");
    assert!(since(&state).is_some());
    let attach = clauseo_protocol::AgentSessionAttached {
        session: "sess-2".parse().unwrap(),
        agent_version: None,
        capabilities: clauseo_protocol::AgentCapabilities::default(),
    };
    let (_, applied) = push(&mut log, &mut state, attach);
    assert_eq!(since(&state), None);
    assert_eq!(status(&state), ThreadStatus::Starting);
    assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
    let checkpoint_before = log.fresh().unwrap();
    push(
        &mut log,
        &mut state,
        TurnStarted {
            turn,
            checkpoint_before,
        },
    );
    assert_eq!(status(&state), ThreadStatus::Running);
    assert_eq!(since(&state), None);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_failed_resume_clears_it_with_no_attach() {
    // A launch that failed: its turn ends `Failed` before it started.
    let (mut log, mut state) = created(53);
    idle_stop(&mut log, &mut state);
    let (turn, _, _) = send(&mut log, &mut state, "Go.");
    assert!(since(&state).is_some());
    let after = log.fresh().unwrap();
    let failed = clauseo_protocol::TurnEnded {
        turn,
        stop: clauseo_protocol::StopReason::Failed,
        checkpoint_after: Some(after),
        usage: None,
    };
    push(&mut log, &mut state, DomainEvent::TurnEnded(failed));
    assert_eq!(since(&state), None);
    assert_eq!(status(&state), ThreadStatus::Idle);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_cancelled_resume_clears_it_with_no_attach() {
    let (mut log, mut state) = created(54);
    idle_stop(&mut log, &mut state);
    let (turn, _, _) = send(&mut log, &mut state, "Go.");
    assert!(since(&state).is_some());
    cancel(&mut log, &mut state, turn);
    assert_eq!(since(&state), None);
    assert_eq!(status(&state), ThreadStatus::Idle);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_first_start_never_sets_it() {
    let (mut log, mut state) = created(55);
    send(&mut log, &mut state, "Go.");
    assert_eq!(status(&state), ThreadStatus::Starting);
    assert_eq!(since(&state), None);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_start_after_an_exit_never_sets_it() {
    let (mut log, mut state) = created(56);
    push(
        &mut log,
        &mut state,
        AgentSessionDetached {
            reason: DetachReason::Exited {
                code: Some(1),
                stderr_tail: String::new(),
            },
        },
    );
    assert_eq!(status(&state), ThreadStatus::AgentExited);
    send(&mut log, &mut state, "Go.");
    assert_eq!(status(&state), ThreadStatus::Starting);
    assert_eq!(since(&state), None);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_second_prompt_while_starting_keeps_the_first_since() {
    let (mut log, mut state) = created(57);
    idle_stop(&mut log, &mut state);
    let (_, first, _) = send(&mut log, &mut state, "One.");
    send(&mut log, &mut state, "Two.");
    assert_eq!(status(&state), ThreadStatus::Starting);
    assert_eq!(since(&state), Some(first.ts));
}

#[test]
fn a_stop_during_a_turn_then_a_prompt_sets_it_once_the_turn_ends() {
    // The stop waits for the running turn (`after_turn`); the turn's end shows `IdleStopped`,
    // and the next prompt starts the resume.
    let mut log = Log::thread(58).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Go.").unwrap();
    let mut state = fold(log.events());
    idle_stop(&mut log, &mut state);
    assert_eq!(status(&state), ThreadStatus::Running);
    let after = log.fresh().unwrap();
    push(&mut log, &mut state, turn_ended(turn, after).unwrap());
    assert_eq!(status(&state), ThreadStatus::IdleStopped);
    assert_eq!(since(&state), None);
    let (_, prompt, _) = send(&mut log, &mut state, "Again.");
    assert_eq!(since(&state), Some(prompt.ts));
    assert_eq!(state, fold(log.events()));
}

#[test]
fn a_prompt_sent_during_the_stop_stays_starting_and_resumes_from_the_detach() {
    // The prompt comes while the agent is stopping: the thread is `Starting` with no cue. The
    // detach that follows keeps `Starting` and starts the cue at its own `ts`; the new
    // session's attach ends the cue, and the turn's start shows `Running`.
    let (mut log, mut state) = created(61);
    let (turn, _, _) = send(&mut log, &mut state, "Go.");
    assert_eq!(status(&state), ThreadStatus::Starting);
    assert_eq!(since(&state), None);
    let (detach, applied) = push(
        &mut log,
        &mut state,
        AgentSessionDetached {
            reason: DetachReason::IdleStopped,
        },
    );
    assert_eq!(status(&state), ThreadStatus::Starting);
    assert_eq!(since(&state), Some(detach.ts));
    assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
    let attach = clauseo_protocol::AgentSessionAttached {
        session: "sess-2".parse().unwrap(),
        agent_version: None,
        capabilities: clauseo_protocol::AgentCapabilities::default(),
    };
    push(&mut log, &mut state, attach);
    assert_eq!(status(&state), ThreadStatus::Starting);
    assert_eq!(since(&state), None);
    let checkpoint_before = log.fresh().unwrap();
    push(
        &mut log,
        &mut state,
        TurnStarted {
            turn,
            checkpoint_before,
        },
    );
    assert_eq!(status(&state), ThreadStatus::Running);
    assert_eq!(state, fold(log.events()));

    // The replay property: a snapshot cut at every point, then the tail, gives the same state.
    let events = log.events();
    let folded = fold(events);
    for cut in 1..events.len() {
        let mut head = empty(events);
        apply_all(&mut head, &events[..cut]);
        let mut rebuilt = round_trip(&head);
        apply_all(&mut rebuilt, &events[cut..]);
        assert_eq!(rebuilt, folded, "cut after {cut} events");
    }
}

#[test]
fn a_schema_10_snapshot_is_refolded() {
    assert_eq!(REDUCER_SCHEMA_VERSION, 11);
    let (mut log, mut state) = created(59);
    idle_stop(&mut log, &mut state);
    send(&mut log, &mut state, "Go.");
    let old = SnapshotRef {
        reducer_schema: 10,
        state: &state,
    };
    let read = Snapshot::from_bytes(&old.to_bytes().unwrap());
    assert!(
        matches!(
            read,
            Err(clauseo_reducer::Error::SchemaMismatch {
                found: 10,
                expected: REDUCER_SCHEMA_VERSION
            })
        ),
        "{read:?}"
    );
}

#[test]
fn a_replay_at_every_cut_gives_the_same_since() {
    // A log that sets, keeps, clears and sets the cue again, cut at every point.
    let mut log = Log::thread(60).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let mut state = fold(log.events());
    idle_stop(&mut log, &mut state);
    send(&mut log, &mut state, "One.");
    send(&mut log, &mut state, "Two.");
    let attach = clauseo_protocol::AgentSessionAttached {
        session: "sess-3".parse().unwrap(),
        agent_version: None,
        capabilities: clauseo_protocol::AgentCapabilities::default(),
    };
    push(&mut log, &mut state, attach);
    idle_stop(&mut log, &mut state);
    send(&mut log, &mut state, "Three.");
    let events = log.events();
    let folded = fold(events);
    assert!(since(&folded).is_some());
    for cut in 1..events.len() {
        let mut head = empty(events);
        apply_all(&mut head, &events[..cut]);
        let mut rebuilt = round_trip(&head);
        apply_all(&mut rebuilt, &events[cut..]);
        assert_eq!(rebuilt, folded, "cut after {cut} events");
        assert_eq!(since(&rebuilt), since(&folded), "cut after {cut} events");
    }
}
