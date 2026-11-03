//! An agent whose connection is gone before its `Ended` reaches the actor: the next prompt
//! waits for a new agent instead of going to the closed connection, whichever way the actor
//! learnt of it (a call's `Closed` answer, or the connection's end while the agent was idle),
//! and the stop that ends it keeps the agent's own exit as the detach.

use clauseo_protocol::{DetachReason, DomainEvent, TurnId};

use super::agents::{send_prompt, until_settled};
use super::rig::{Rig, kind};
use crate::actor::input::Input;
use crate::actor::turn::TurnPhase;
use crate::actor::turn::agents::Stage;
use crate::agent::AgentInput;

/// The agent exits on the prompt without answering it.
const EXIT_ON_PROMPT: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"exit":{"code":1}}
"#;

/// The agent answers the prompt, then exits while idle.
const EXIT_AFTER_ANSWER: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"exit":{"code":1}}
"#;

/// An agent's `Ended`, the input the tests hold back.
fn ended(_: &crate::actor::Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Ended(_),
            ..
        }
    )
}

/// Sends a second prompt while the first agent's `Ended` is held, and checks that the turn
/// waits for a new agent while the old one stops.
fn second_prompt_waits(rig: &mut Rig, thread: clauseo_protocol::ThreadId) -> TurnId {
    let second = send_prompt(rig, 11, thread);
    rig.run_holding("the second turn waiting or ended", ended, |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none()
            || state
                .agent
                .as_ref()
                .is_some_and(|agent| agent.stage == Stage::Stopping)
    });
    let state = &rig.actor.threads[&thread].turn;
    assert!(
        state
            .current
            .as_ref()
            .is_some_and(|current| current.turn == second && current.phase == TurnPhase::Launching),
        "the second turn went to the closed connection: {:?}",
        rig.log(thread).iter().map(kind).collect::<Vec<_>>()
    );
    second
}

/// Settles, and checks that a second agent attached and that `second` ended on it.
fn second_agent_took(rig: &mut Rig, thread: clauseo_protocol::ThreadId, second: TurnId) {
    rig.release_held();
    until_settled(rig, thread);
    let log = rig.log(thread);
    let kinds = || log.iter().map(kind).collect::<Vec<_>>();
    let attached: Vec<usize> = log
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, DomainEvent::AgentSessionAttached(_)))
        .map(|(at, _)| at)
        .collect();
    let ended = log
        .iter()
        .position(|event| matches!(event, DomainEvent::TurnEnded(e) if e.turn == second));
    assert_eq!(attached.len(), 2, "{:?}", kinds());
    assert!(ended.is_some_and(|at| at > attached[1]), "{:?}", kinds());
}

/// The prompt's `Closed` answer reaches the actor before the agent's `Ended` (as designed), and
/// on Windows the next prompt can come between the two (CI run 41712908024). That prompt must
/// wait for a new agent, not go to the closed connection and fail with `agent.exited` at once.
#[test]
fn a_prompt_between_a_closed_answer_and_the_end_starts_a_new_agent() {
    let mut rig = Rig::start("turn-closed-then-prompt", EXIT_ON_PROMPT);
    let thread = rig.thread();
    let first = send_prompt(&mut rig, 10, thread);
    rig.run_holding("the first turn's end", ended, |actor| {
        actor.threads[&thread].turn.current.is_none() && actor.work.commits == 0
    });
    assert!(
        rig.actor.threads[&thread].turn.agent.is_some(),
        "the agent's end is held"
    );
    let second = second_prompt_waits(&mut rig, thread);
    second_agent_took(&mut rig, thread, second);
    let log = rig.log(thread);
    let failed = log
        .iter()
        .position(|event| matches!(event, DomainEvent::TurnFailed(f) if f.turn == first));
    let attached = log
        .iter()
        .rposition(|event| matches!(event, DomainEvent::AgentSessionAttached(_)));
    assert!(
        failed < attached,
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
    rig.finish();
}

/// No call is in flight when the agent's connection ends: the agent answered its prompt and
/// exited while idle. Its `Ended` is held until after the next prompt (as on Windows, which
/// reports the exit late), and that prompt must still wait for a new agent.
#[test]
fn a_prompt_after_an_idle_agent_s_connection_ended_starts_a_new_agent() {
    let mut rig = Rig::start("turn-idle-closed-then-prompt", EXIT_AFTER_ANSWER);
    let thread = rig.thread();
    let first = send_prompt(&mut rig, 10, thread);
    // `Ended` comes once the event forwarder saw the channel close, so once it is held the
    // connection is known gone.
    rig.run_holding("the first turn's end and the agent's", ended, |actor| {
        actor.threads[&thread].turn.current.is_none() && actor.work.commits == 0
    });
    if rig.held.is_empty() {
        rig.hold_next("the agent's end", ended);
    }
    let log = rig.log(thread);
    assert!(
        !log.iter()
            .any(|event| matches!(event, DomainEvent::TurnFailed(f) if f.turn == first)),
        "the first turn was answered: {:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
    let second = second_prompt_waits(&mut rig, thread);
    second_agent_took(&mut rig, thread, second);
    rig.finish();
}

/// The agent's process outlives its pipes: it closes them, then exits 7 a second later. The
/// stop the next prompt sends can come before that exit is reported; the detach must still be
/// the agent's own exit, never Clauseo's close.
#[cfg(unix)]
#[test]
fn a_stopped_agent_whose_connection_was_gone_detaches_with_its_own_exit() {
    let script = r#""$0" --scenario "$1"; exec >&- 2>&-; sleep 1; exit 7"#;
    let mut rig = Rig::start_wrapped("turn-closed-keeps-exit", EXIT_ON_PROMPT, script);
    let thread = rig.thread();
    let _first = send_prompt(&mut rig, 10, thread);
    rig.run_until("the first turn's end", |actor| {
        actor.threads[&thread].turn.current.is_none() && actor.work.commits == 0
    });
    let _second = send_prompt(&mut rig, 11, thread);
    until_settled(&mut rig, thread);
    let detaches: Vec<DetachReason> = rig
        .log(thread)
        .into_iter()
        .filter_map(|event| {
            if let DomainEvent::AgentSessionDetached(detached) = event {
                Some(detached.reason)
            } else {
                None
            }
        })
        .collect();
    assert!(
        matches!(
            detaches.first(),
            Some(DetachReason::Exited { code: Some(7), .. })
        ),
        "{detaches:?}"
    );
    rig.finish();
}
