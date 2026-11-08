//! The sandbox runner's Escapee exit (R17 PR-E, stage C's #649): an agent whose process ends
//! with it ends its turn `LeftSafeWorkspace`, with no `TurnFailed`, on the normal stop path
//! (logged, so replayed), and its detach keeps the exit code. No real process exits with a
//! 32-bit code off Windows, so the test rewrites the fake agent's exit report before the actor
//! reads it, while the turn is still ending, as Windows reports an exit after the prompt's
//! `Closed` answer.

use std::cell::Cell;

use clauseo_protocol::{DetachReason, DomainEvent, StopReason};

use super::agents::{send_prompt, until_settled};
use super::rig::{Rig, kind};
use crate::actor::Actor;
use crate::actor::input::{Input, MatterTag};
use crate::actor::turn::TurnPhase;
use crate::agent::{AgentInput, Ended};

/// `clauseo-sandbox-run.exe`'s Escapee exit as Windows' `ExitStatus::code` gives it.
const ESCAPEE: i32 = 0xE0C1_500F_u32.cast_signed();

/// The agent exits 1 on the prompt without answering it.
const EXIT_ON_PROMPT: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
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

/// Whether `input` is an agent's end.
fn is_end(input: &Input) -> bool {
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
