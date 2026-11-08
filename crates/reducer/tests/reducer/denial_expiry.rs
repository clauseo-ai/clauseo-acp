//! When a pending denial card stops waiting (R18): it survives the end of its own turn, and it
//! expires when the next turn starts, when the agent session ends (an idle stop), when a restart
//! fails or is cut short (which also clears `restarting_since`), and when the thread is archived.
//! A replay gives the same state each time.

use clauseo_protocol::{
    AgentSessionDetached, DenialId, DenialKind, DenialRecorded, DenialSource, DenialState,
    DetachReason, DomainEvent, ErrorCode, StopReason, ThreadArchived, Timestamp, TurnEnded,
    TurnFailed, TurnId, TurnStarted, ViewEntry,
};
use clauseo_reducer::{Applied, StreamState, apply};

use crate::fixtures::{Log, start_turn, thread_created, turn_ended, user_message};
use crate::helpers::fold;

/// Pushes `event` and applies it, returning what the apply gave.
#[cfg(test)]
fn push(log: &mut Log, state: &mut StreamState, event: impl Into<DomainEvent>) -> Applied {
    let envelope = log.push(event).unwrap();
    let out = apply(state, &envelope).unwrap();
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    out
}

/// A thread `n` with a running turn that has one pending card.
#[cfg(test)]
fn with_card(n: u64) -> (Log, StreamState, TurnId, DenialId) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Read the deals folder.").unwrap();
    let denial: DenialId = log.fresh().unwrap();
    log.push(DenialRecorded {
        turn: Some(turn),
        denial,
        kind: DenialKind::Folder {
            path: "/Users/a/Deals".to_owned(),
            other_matter: None,
        },
        source: DenialSource::Sandbox,
        offered: true,
        count: 1,
    })
    .unwrap();
    let state = fold(log.events());
    assert_eq!(card(&state, denial), DenialState::Pending);
    (log, state, turn, denial)
}

#[cfg(test)]
fn card(state: &StreamState, denial: DenialId) -> DenialState {
    state
        .entries()
        .values()
        .find_map(|entry| match entry {
            ViewEntry::Denial(found) if found.denial == denial => Some(found.state),
            _ => None,
        })
        .unwrap()
}

#[cfg(test)]
fn restarting_since(state: &StreamState) -> Option<Timestamp> {
    state
        .as_thread()
        .unwrap()
        .header()
        .unwrap()
        .restarting_since
}

#[cfg(test)]
fn end(log: &mut Log, state: &mut StreamState, turn: TurnId) {
    let checkpoint = log.fresh().unwrap();
    push(log, state, turn_ended(turn, checkpoint).unwrap());
}

#[cfg(test)]
fn detach(log: &mut Log, state: &mut StreamState, reason: DetachReason) {
    push(log, state, AgentSessionDetached { reason });
}

#[test]
fn a_pending_card_survives_its_turn_and_expires_when_the_next_starts() {
    let (mut log, mut state, turn, denial) = with_card(101);
    end(&mut log, &mut state, turn);
    assert_eq!(card(&state, denial), DenialState::Pending);
    // A prompt queued for the next turn does not end it: the start does.
    let next: TurnId = log.fresh().unwrap();
    let message = log.fresh().unwrap();
    push(&mut log, &mut state, user_message(next, message, "Go on."));
    assert_eq!(card(&state, denial), DenialState::Pending);
    let checkpoint_before = log.fresh().unwrap();
    let out = push(
        &mut log,
        &mut state,
        TurnStarted {
            turn: next,
            checkpoint_before,
        },
    );
    assert_eq!(card(&state, denial), DenialState::Expired);
    assert!(out.changes.len() >= 2, "{:?}", out.changes);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn an_idle_stop_expires_a_card_that_outlived_its_turn() {
    let (mut log, mut state, turn, denial) = with_card(102);
    end(&mut log, &mut state, turn);
    detach(&mut log, &mut state, DetachReason::IdleStopped);
    assert_eq!(card(&state, denial), DenialState::Expired);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn archiving_the_thread_expires_a_pending_card() {
    let (mut log, mut state, turn, denial) = with_card(103);
    end(&mut log, &mut state, turn);
    push(&mut log, &mut state, ThreadArchived {});
    assert_eq!(card(&state, denial), DenialState::Expired);
    assert_eq!(state, fold(log.events()));
}

/// The relaunch after a restart fails before its turn starts: the host logs `TurnFailed` and
/// `TurnEnded { Failed }` for the turn that would go on (a sign-in refusal or
/// `sandbox.required` through `fail_all`, or `agent.launch_failed`). The restart ends and the
/// card expires at the failure.
#[test]
fn a_failed_relaunch_ends_the_restart_and_expires_the_card() {
    for (n, code) in [
        (104, ErrorCode::AgentAuthRequired),
        (105, ErrorCode::SandboxRequired),
        (106, ErrorCode::AgentLaunchFailed),
    ] {
        let (mut log, mut state, turn, denial) = with_card(n);
        end(&mut log, &mut state, turn);
        detach(&mut log, &mut state, DetachReason::Restarting);
        assert!(restarting_since(&state).is_some());
        let next: TurnId = log.fresh().unwrap();
        let message = log.fresh().unwrap();
        push(&mut log, &mut state, user_message(next, message, "Go on."));
        assert_eq!(card(&state, denial), DenialState::Pending, "{code:?}");
        let failed = TurnFailed {
            turn: next,
            code,
            detail: None,
            at: None,
        };
        push(&mut log, &mut state, failed);
        assert_eq!(restarting_since(&state), None, "{code:?}");
        assert_eq!(card(&state, denial), DenialState::Expired, "{code:?}");
        push(
            &mut log,
            &mut state,
            TurnEnded {
                turn: next,
                stop: StopReason::Failed,
                usage: None,
                checkpoint_after: None,
            },
        );
        assert_eq!(restarting_since(&state), None);
        assert_eq!(state, fold(log.events()), "{code:?}");
    }
}

/// Clauseo quits mid-restart: the running turn ends (here cancelled) before any attach. The
/// restart ends and the card expires.
#[test]
fn a_quit_mid_restart_ends_the_restart_and_expires_the_card() {
    let (mut log, mut state, turn, denial) = with_card(107);
    detach(&mut log, &mut state, DetachReason::Restarting);
    assert!(restarting_since(&state).is_some());
    assert_eq!(card(&state, denial), DenialState::Pending);
    push(
        &mut log,
        &mut state,
        TurnEnded {
            turn,
            stop: StopReason::Cancelled,
            usage: None,
            checkpoint_after: None,
        },
    );
    assert_eq!(restarting_since(&state), None);
    assert_eq!(card(&state, denial), DenialState::Expired);
    assert_eq!(state, fold(log.events()));
}

/// Without a restart shown, a turn's failure leaves its card waiting.
#[test]
fn a_failure_without_a_restart_keeps_the_card() {
    let (mut log, mut state, turn, denial) = with_card(108);
    let failed = TurnFailed {
        turn,
        code: ErrorCode::AgentLaunchFailed,
        detail: None,
        at: None,
    };
    push(&mut log, &mut state, failed);
    end(&mut log, &mut state, turn);
    assert_eq!(card(&state, denial), DenialState::Pending);
    assert_eq!(state, fold(log.events()));
}
