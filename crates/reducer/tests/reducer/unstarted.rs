//! A turn that fails before it starts (its agent's launch failed: `TurnFailed`, then
//! `TurnEnded { Failed }`, no `TurnStarted`) keeps its prompt and gets a failed step card, and
//! settles the status as a failed turn that started does (0002). U10's report: a relaunch that
//! failed showed nothing, the prompt gone and the thread idle.

use clauseo_protocol::{
    AgentSessionDetached, DetachReason, DomainEvent, Envelope, ErrorCode, MessageId, StepEntry,
    StopReason, ThreadStatus, TurnEnded, TurnFailed, TurnId, TurnStarted, UserMessageStatus,
    ViewEntry,
};
use clauseo_reducer::{StreamState, ViewChange, apply};

use crate::events::{thread_created, user_message};
use crate::fixtures::Log;
use crate::helpers::{fold, prompt_status, round_trip};

/// The codes a failed launch ends a turn with.
#[cfg(test)]
const CODES: [ErrorCode; 3] = [
    ErrorCode::AgentExited,
    ErrorCode::AgentLaunchFailed,
    ErrorCode::AgentProtocolError,
];

/// A turn's failure and its end, as the host logs a failed turn.
#[cfg(test)]
fn fail(log: &mut Log, turn: TurnId, code: ErrorCode) -> Envelope {
    let failed = TurnFailed {
        turn,
        code,
        detail: Some("the agent did not start".to_owned()),
    };
    log.push(failed).unwrap();
    let ended = TurnEnded {
        turn,
        stop: StopReason::Failed,
        usage: None,
        checkpoint_after: None,
    };
    log.push(ended).unwrap()
}

/// The step of `turn`, if any.
#[cfg(test)]
fn step(state: &StreamState, turn: TurnId) -> Option<&StepEntry> {
    state.entries().values().find_map(|entry| match entry {
        ViewEntry::Step(step) if step.turn == turn => Some(step),
        _ => None,
    })
}

#[cfg(test)]
fn status(state: &StreamState) -> ThreadStatus {
    state.as_thread().unwrap().status()
}

/// A thread with one prompt, `started` or not, whose turn fails with `code`. Returns the log,
/// the state before the end (its `TurnFailed` folded), the end's envelope, the turn and the prompt.
#[cfg(test)]
fn one_failed_turn(
    code: ErrorCode,
    started: bool,
) -> (Log, StreamState, Envelope, TurnId, MessageId) {
    let mut log = Log::thread(31).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let (turn, message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(turn, message, "Draft the notice."))
        .unwrap();
    if started {
        let checkpoint_before = log.fresh().unwrap();
        log.push(TurnStarted {
            turn,
            checkpoint_before,
        })
        .unwrap();
    }
    let ended = fail(&mut log, turn, code);
    let events = log.events();
    let before = fold(&events[..events.len() - 1]);
    (log, before, ended, turn, message)
}

#[test]
fn a_turn_failed_before_it_starts_keeps_its_prompt() {
    for code in CODES {
        let (log, _, _, _, message) = one_failed_turn(code, false);
        let state = fold(log.events());
        assert_eq!(
            prompt_status(&state, message),
            Some(UserMessageStatus::Sent),
            "{code:?}"
        );
    }
}

#[test]
fn a_turn_failed_before_it_starts_gets_a_failed_step_card() {
    for code in CODES {
        let (log, before, ended, turn, _) = one_failed_turn(code, false);
        let mut state = before;
        let out = apply(&mut state, &ended).unwrap();
        assert!(out.anomalies.is_empty(), "{code:?}: {:?}", out.anomalies);
        let card = step(&state, turn).unwrap_or_else(|| panic!("{code:?}: no step card"));
        assert_eq!(card.stop, StopReason::Failed, "{code:?}");
        assert_eq!(card.failure, Some(code));
        assert_eq!(
            card.ts, ended.ts,
            "{code:?}: the step's time is its TurnEnded's"
        );
        assert_eq!(card.order, ended.seq);
        assert_eq!(card.checkpoint_after, None);
        let sent = out.changes.iter().any(|change| {
            matches!(change, ViewChange::Upsert(entry)
                if matches!(&**entry, ViewEntry::Step(s) if s.turn == turn))
        });
        assert!(sent, "{code:?}: no step in the changes: {:?}", out.changes);
        // The same card as a turn that started and failed alike, but for its order and time.
        let (twin_log, ..) = one_failed_turn(code, true);
        let twin = fold(twin_log.events());
        let twin_card = step(&twin, turn).unwrap();
        assert_eq!(
            StepEntry {
                order: twin_card.order,
                ts: twin_card.ts,
                ..card.clone()
            },
            *twin_card,
            "{code:?}"
        );
        // The status settles as the started twin's does.
        assert_eq!(status(&state), status(&twin), "{code:?}");
        // From the log and through a snapshot alike.
        assert_eq!(state, fold(log.events()));
        assert_eq!(round_trip(&state), state);
    }
}

/// The host's relaunch sequence: turn 1 fails mid-turn as its agent exits, turn 2's relaunch
/// fails before its session attaches. Two failed step cards, both prompts present.
#[test]
fn a_failed_relaunch_after_an_exit_shows_both_failed_turns() {
    let mut log = Log::thread(32).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let (first, first_message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(first, first_message, "Start the answer."))
        .unwrap();
    let checkpoint_before = log.fresh().unwrap();
    log.push(TurnStarted {
        turn: first,
        checkpoint_before,
    })
    .unwrap();
    fail(&mut log, first, ErrorCode::AgentExited);
    let reason = DetachReason::Exited {
        code: Some(1),
        stderr_tail: String::new(),
    };
    log.push(AgentSessionDetached { reason }).unwrap();
    assert_eq!(status(&fold(log.events())), ThreadStatus::AgentExited);

    let (second, second_message): (TurnId, MessageId) =
        (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(second, second_message, "Try again."))
        .unwrap();
    assert_eq!(status(&fold(log.events())), ThreadStatus::Starting);
    fail(&mut log, second, ErrorCode::AgentExited);
    let state = fold(log.events());

    for (turn, message) in [(first, first_message), (second, second_message)] {
        assert_eq!(
            prompt_status(&state, message),
            Some(UserMessageStatus::Sent)
        );
        let card = step(&state, turn).unwrap_or_else(|| panic!("no step card for {turn}"));
        assert_eq!(card.stop, StopReason::Failed);
        assert_eq!(card.failure, Some(ErrorCode::AgentExited));
    }
    let steps = state
        .entries()
        .values()
        .filter(|entry| matches!(entry, ViewEntry::Step(_)))
        .count();
    assert_eq!(steps, 2);
    assert_eq!(status(&state), ThreadStatus::Idle);
    assert!(
        !log.events()
            .iter()
            .any(|e| matches!(&e.event, DomainEvent::TurnStarted(s) if s.turn == second))
    );
    assert_eq!(round_trip(&state), state);
}

/// A turn runs while an earlier-sent turn ends `Failed` without starting: the card appears and
/// the running turn keeps the status. The host cannot produce this order today (a thread has one
/// current turn, and a later prompt waits for it to end); the fold still must not let the
/// unstarted turn's end take the status from the running one.
#[test]
fn a_turn_failed_before_it_starts_leaves_a_running_turns_status() {
    let mut log = Log::thread(33).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let (unstarted, message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(unstarted, message, "Draft the notice."))
        .unwrap();
    let running = crate::events::start_turn(&mut log, "Check clause 2.").unwrap();
    assert_eq!(status(&fold(log.events())), ThreadStatus::Running);
    fail(&mut log, unstarted, ErrorCode::AgentLaunchFailed);
    let state = fold(log.events());
    assert_eq!(status(&state), ThreadStatus::Running);
    let card = step(&state, unstarted).unwrap_or_else(|| panic!("no step card"));
    assert_eq!(card.failure, Some(ErrorCode::AgentLaunchFailed));
    assert!(step(&state, running).is_none());
    assert_eq!(
        prompt_status(&state, message),
        Some(UserMessageStatus::Sent)
    );
    assert_eq!(round_trip(&state), state);
}

/// Two prompts sent while no turn runs (the later one is the thread's last turn, `Starting`);
/// the earlier one's turn fails before it starts: it gets its card and the later prompt keeps
/// `Starting`. Without the guard the end would settle the thread to `Idle` under a waiting
/// prompt. The host cannot produce this order today either.
#[test]
fn a_turn_failed_before_it_starts_leaves_a_later_prompts_status() {
    let mut log = Log::thread(34).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let (first, first_message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(first, first_message, "Draft the notice."))
        .unwrap();
    let (later, later_message): (TurnId, MessageId) = (log.fresh().unwrap(), log.fresh().unwrap());
    log.push(user_message(later, later_message, "Then the cover letter."))
        .unwrap();
    fail(&mut log, first, ErrorCode::AgentExited);
    let state = fold(log.events());
    assert_eq!(status(&state), ThreadStatus::Starting);
    assert!(step(&state, first).is_some());
    assert!(step(&state, later).is_none());
    for message in [first_message, later_message] {
        assert_eq!(
            prompt_status(&state, message),
            Some(UserMessageStatus::Sent)
        );
    }
}
