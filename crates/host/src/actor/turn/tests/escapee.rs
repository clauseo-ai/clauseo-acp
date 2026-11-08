//! The sandbox runner's Escapee exit (R17 PR-E, stage C's #649): an agent whose process ends
//! with it ends its turn `LeftSafeWorkspace`, with no `TurnFailed`, on the normal stop path
//! (logged, so replayed), and its detach keeps the exit code. No real process exits with a
//! 32-bit code off Windows, so the test rewrites the fake agent's exit report before the actor
//! reads it, while the turn is still ending, as Windows reports an exit after the prompt's
//! `Closed` answer. An exit reported after the after-turn checkpoint answered still ends the
//! turn `LeftSafeWorkspace`: the close waits for it, up to `EXIT_WAIT` on the host clock
//! (`exit_wait.rs`); past that the turn ends as before and the late exit is only logged. Only an
//! agent launched while the computer had tier B runs under the runner: each test sets it.

use std::cell::Cell;

use clauseo_protocol::{DetachReason, DomainEvent, Protection, ProtectionTier, StopReason};

use super::agents::{send_prompt, until_settled};
use super::rig::{Rig, kind};
use crate::actor::Actor;
use crate::actor::input::{Input, MatterTag};
use crate::actor::protection::{Observed, ProtectionHost};
use crate::actor::turn::TurnPhase;
use crate::actor::turn::exit_wait::EXIT_WAIT;
use crate::agent::{AgentInput, Ended};

/// `clauseo-sandbox-run.exe`'s Escapee exit as Windows' `ExitStatus::code` gives it.
pub(super) const ESCAPEE: i32 = 0xE0C1_500F_u32.cast_signed();

/// The agent exits 1 on the prompt without answering it.
pub(super) const EXIT_ON_PROMPT: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"exit":{"code":1}}
"#;

/// Whether `thread`'s turn is ending: its after-turn checkpoint sent, not answered.
fn ending(actor: &Actor, thread: clauseo_protocol::ThreadId) -> bool {
    actor.threads[&thread]
        .turn
        .current
        .as_ref()
        .is_some_and(|current| current.phase == TurnPhase::Ending)
}

/// The computer has tier B, so the agents the rig launches from now on run under the sandbox
/// runner (`protection::tier_b`).
pub(super) fn tier_b(rig: &mut Rig) {
    let computer = Protection::Protected {
        tier: ProtectionTier::B,
    };
    let observed = Observed {
        computer,
        asks_admin: false,
        set_up_at: None,
        set_up_state: None,
    };
    rig.actor.protection = ProtectionHost::new(None, observed);
}

/// Whether `input` is an agent's end.
pub(super) fn is_end(input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Ended(_),
            ..
        }
    )
}

#[test]
fn an_escapee_exit_ends_the_turn_left_safe_workspace_with_no_failure() {
    let mut rig = Rig::start("turn-escapee", EXIT_ON_PROMPT);
    tier_b(&mut rig);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    let saw_end = Cell::new(false);
    // Hold the agent's end and, once the turn ends, its after-turn checkpoint's answer, so the
    // end is read while the turn is still ending.
    rig.run_holding(
        "the agent's end while the turn ends",
        |actor, input| {
            if is_end(input) {
                saw_end.set(true);
                return true;
            }
            let reply = matches!(
                input,
                Input::Matter {
                    tag: MatterTag::Turn(_),
                    ..
                }
            );
            reply && ending(actor, thread)
        },
        |actor| saw_end.get() && ending(actor, thread),
    );
    let mut held = std::mem::take(&mut rig.held);
    let at = held.iter().position(is_end).unwrap();
    let mut end = held.remove(at);
    let Input::Agent {
        input: AgentInput::Ended(Ended::Exited(DetachReason::Exited { code, .. })),
        ..
    } = &mut end
    else {
        panic!("not an exit: {end:?}");
    };
    assert_eq!(*code, Some(1));
    *code = Some(ESCAPEE);
    rig.held = std::iter::once(end).chain(held).collect();
    rig.release_held();
    until_settled(&mut rig, thread);
    let log = rig.log(thread);
    let kinds = || log.iter().map(kind).collect::<Vec<_>>();
    let ended = log.iter().find_map(|event| {
        if let DomainEvent::TurnEnded(ended) = event
            && ended.turn == turn
        {
            return Some(ended.stop);
        }
        None
    });
    assert_eq!(ended, Some(StopReason::LeftSafeWorkspace), "{:?}", kinds());
    assert!(
        !log.iter()
            .any(|event| matches!(event, DomainEvent::TurnFailed(f) if f.turn == turn)),
        "{:?}",
        kinds()
    );
    let detached = log.iter().any(|event| {
        matches!(event, DomainEvent::AgentSessionDetached(d)
            if matches!(d.reason, DetachReason::Exited { code: Some(ESCAPEE), .. }))
    });
    assert!(detached, "{:?}", kinds());
    // No new agent starts for it: the lawyer's next turn is a normal launch.
    assert!(rig.actor.threads[&thread].turn.agent.is_none());
    rig.finish();
}

/// Whether `thread`'s ending turn holds its after-turn checkpoint's answer for the agent's exit.
fn waiting_for_exit(actor: &Actor, thread: clauseo_protocol::ThreadId) -> bool {
    actor.threads[&thread]
        .turn
        .current
        .as_ref()
        .is_some_and(|current| current.exit_wait.holds_checkpoint())
}

/// Runs `rig`'s prompt until its turn's close waits for the agent's exit, the agent's `Ended`
/// held back, then rewrites that exit's code to the Escapee's.
pub(super) fn until_waiting_with_a_held_escapee(rig: &mut Rig, thread: clauseo_protocol::ThreadId) {
    rig.run_holding(
        "the turn's close waiting for the agent's exit",
        |_, input| is_end(input),
        |actor| waiting_for_exit(actor, thread),
    );
    if rig.held.is_empty() {
        rig.hold_next("the agent's end", |_, input| is_end(input));
    }
    let [end] = rig.held.as_mut_slice() else {
        panic!("not one held input: {:?}", rig.held);
    };
    let Input::Agent {
        input: AgentInput::Ended(Ended::Exited(DetachReason::Exited { code, .. })),
        ..
    } = end
    else {
        panic!("not an exit: {end:?}");
    };
    assert_eq!(*code, Some(1));
    *code = Some(ESCAPEE);
}

/// The stop and the failures `thread`'s log holds for `turn`, and where its `TurnEnded` and
/// the Escapee detach are in it.
pub(super) fn ended_in(
    log: &[DomainEvent],
    turn: clauseo_protocol::TurnId,
) -> (Option<StopReason>, bool, Option<usize>, Option<usize>) {
    let ended = log
        .iter()
        .position(|event| matches!(event, DomainEvent::TurnEnded(ended) if ended.turn == turn));
    let stop = ended.and_then(|at| match log.get(at) {
        Some(DomainEvent::TurnEnded(ended)) => Some(ended.stop),
        _ => None,
    });
    let failed = log
        .iter()
        .any(|event| matches!(event, DomainEvent::TurnFailed(f) if f.turn == turn));
    let detached = log.iter().position(|event| {
        matches!(event, DomainEvent::AgentSessionDetached(d)
            if matches!(d.reason, DetachReason::Exited { code: Some(ESCAPEE), .. }))
    });
    (stop, failed, ended, detached)
}

/// Windows reports the exit after the after-turn checkpoint answered: the close waits for it,
/// so the turn still ends `LeftSafeWorkspace` with no `TurnFailed`, its detach after
/// `TurnEnded`.
#[test]
fn an_escapee_exit_reported_after_the_checkpoint_still_ends_the_turn_left_safe_workspace() {
    let mut rig = Rig::start("turn-escapee-late", EXIT_ON_PROMPT);
    tier_b(&mut rig);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_waiting_with_a_held_escapee(&mut rig, thread);
    // Nothing ends the turn while the exit is due, short of the bound.
    rig.clock.advance(
        EXIT_WAIT
            .checked_sub(std::time::Duration::from_millis(1))
            .unwrap(),
    );
    rig.drain();
    assert!(
        waiting_for_exit(&rig.actor, thread),
        "the close stopped waiting early"
    );
    rig.release_held();
    until_settled(&mut rig, thread);
    let log = rig.log(thread);
    let kinds = || log.iter().map(kind).collect::<Vec<_>>();
    let (stop, failed, ended, detached) = ended_in(&log, turn);
    assert_eq!(stop, Some(StopReason::LeftSafeWorkspace), "{:?}", kinds());
    assert!(!failed, "{:?}", kinds());
    assert!(ended < detached, "{:?}", kinds());
    assert!(rig.actor.threads[&thread].turn.agent.is_none());
    rig.finish();
}

/// No exit report within `EXIT_WAIT`: the turn ends as before (`Failed`, `agent.exited`), and
/// the Escapee exit that comes later is only logged, its detach after `TurnEnded`.
#[test]
fn an_escapee_exit_past_the_wait_leaves_the_turn_failed_and_is_logged() {
    let mut rig = Rig::start("turn-escapee-past-wait", EXIT_ON_PROMPT);
    tier_b(&mut rig);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_waiting_with_a_held_escapee(&mut rig, thread);
    rig.clock.advance(EXIT_WAIT);
    rig.run_holding(
        "the turn's end at the bound",
        |_, input| is_end(input),
        |actor| actor.threads[&thread].turn.current.is_none() && actor.work.commits == 0,
    );
    rig.release_held();
    until_settled(&mut rig, thread);
    let log = rig.log(thread);
    let kinds = || log.iter().map(kind).collect::<Vec<_>>();
    let (stop, failed, ended, detached) = ended_in(&log, turn);
    assert_eq!(stop, Some(StopReason::Failed), "{:?}", kinds());
    assert!(failed, "{:?}", kinds());
    assert!(detached.is_some() && ended < detached, "{:?}", kinds());
    rig.finish();
}
