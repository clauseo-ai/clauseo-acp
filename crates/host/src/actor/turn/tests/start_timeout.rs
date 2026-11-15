//! The start's bound (`agents/start_timer.rs`): an agent that never answers `initialize` is
//! stopped through the ladder once [`START_WAIT`] runs out, and the waiting turn fails
//! `agent.launch_failed`, or `agent.start_offline` when the host's probe cannot reach the
//! agent's service. Driven by the manual clock; the agent's end is held (`hold_next`), never
//! drained, so the deadline's stop and the agent's exit cannot race the test (#1050).

use clauseo_protocol::{AgentKind, ErrorCode, StopReason};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline::ended;
use super::rig::Rig;
use super::start_offline::start_rig;
use crate::actor::input::Input;
use crate::actor::turn::agents::{START_WAIT, Stage};
use crate::actor::turn::offline::probe::ReachProbe;
use crate::agent::{AgentInput, Ended};

/// Reads `initialize` and never answers it; waits on stdin, and exits once it closes.
const SILENT: &str = r#"{"expect":{"method":"initialize","as":"init","params":{"protocolVersion":1}}}
{"expect":{"method":"session/new","as":"new","params":{"cwd":"{{any}}"}}}
"#;

/// Runs a prompt to the silent agent's launch, lets the bound run out and gives the code its
/// turn failed with; the agent must have been stopped by the ladder.
fn timed_out(rig: &mut Rig) -> Option<ErrorCode> {
    let thread = rig.thread();
    let turn = send_prompt(rig, 10, thread);
    rig.run_until("the agent's launch", |actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| agent.stage == Stage::Initializing)
    });
    rig.clock.advance(
        START_WAIT
            .checked_sub(std::time::Duration::from_millis(1))
            .unwrap(),
    );
    rig.run_tasks_until_stalled();
    let stage = rig.actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .map(|agent| agent.stage);
    assert_eq!(stage, Some(Stage::Initializing), "stopped before the bound");
    // The clock alone moves: `hold_next` handles the deadline and holds the agent's end.
    rig.clock.advance(std::time::Duration::from_millis(1));
    let agent_end = |_: &crate::actor::Actor, input: &Input| {
        matches!(
            input,
            Input::Agent {
                input: AgentInput::Ended(_),
                ..
            }
        )
    };
    rig.hold_next("the agent's end", agent_end);
    assert!(
        matches!(
            rig.held.last(),
            Some(Input::Agent {
                input: AgentInput::Ended(Ended::Stopped { .. }),
                ..
            })
        ),
        "not stopped by the ladder: {:?}",
        rig.held.last()
    );
    rig.release_held();
    until_ended(rig, thread);
    let (stop, code) = ended(&rig.log(thread), turn);
    assert_eq!(stop, Some(StopReason::Failed));
    code
}

#[test]
fn a_start_that_never_answers_fails_at_the_bound() {
    let (probe, count) = ReachProbe::fixed(false).counted();
    let mut rig = start_rig("start-timeout", SILENT, None, probe);
    assert_eq!(timed_out(&mut rig), Some(ErrorCode::AgentLaunchFailed));
    // An agent of no known service is never probed.
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
    rig.finish();
}

#[test]
fn a_start_that_never_answers_and_whose_probe_fails_is_start_offline() {
    let (probe, count) = ReachProbe::fixed(false).counted();
    let mut rig = start_rig(
        "start-timeout-offline",
        SILENT,
        Some(AgentKind::Claude),
        probe,
    );
    assert_eq!(timed_out(&mut rig), Some(ErrorCode::AgentStartOffline));
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    rig.finish();
}

#[test]
fn a_start_that_never_answers_while_online_is_a_failed_start() {
    let (probe, _) = ReachProbe::fixed(true).counted();
    let mut rig = start_rig(
        "start-timeout-online",
        SILENT,
        Some(AgentKind::Claude),
        probe,
    );
    assert_eq!(timed_out(&mut rig), Some(ErrorCode::AgentLaunchFailed));
    rig.finish();
}
