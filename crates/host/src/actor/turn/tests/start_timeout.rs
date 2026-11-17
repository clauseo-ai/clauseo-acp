//! The start's bound (`agents/start_timer.rs`): an agent that never answers `initialize` is
//! stopped through the ladder once [`START_WAIT`] runs out, and the waiting turn fails
//! `agent.launch_failed`, or `agent.start_unreachable` when the host's probe cannot reach the
//! agent's service while the window says online. Driven by the manual clock; the agent's end is
//! held (`hold_next`), never drained, so the deadline's stop and the agent's exit cannot race the
//! test (#1050). The bound counts silence: an agent that keeps sending outlasts it, and time the
//! computer spent asleep (a look at the bound held up past its time) is left out of it.

use std::time::Duration;

use clauseo_protocol::{AgentKind, ErrorCode, StopReason, ThreadId, TurnId};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline::ended;
use super::rig::Rig;
use super::start_offline::start_rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::agents::Stage;
use crate::actor::turn::agents::start_timer::{START_WAIT, TICK};
use crate::actor::turn::offline::probe::ReachProbe;
use crate::agent::{AgentInput, Ended};

/// Reads `initialize` and never answers it; waits on stdin, and exits once it closes.
pub(super) const SILENT: &str = r#"{"expect":{"method":"initialize","as":"init","params":{"protocolVersion":1}}}
{"expect":{"method":"session/new","as":"new","params":{"cwd":"{{any}}"}}}
"#;

/// Answers `initialize`, sends three updates (a replay's stand-in) and reads `session/new`.
const PACED_START: &str = r#"{"expect":{"method":"initialize","as":"init","params":{"protocolVersion":1}}}
{"respond":{"to":"init","result":{"protocolVersion":1,"agentCapabilities":{"loadSession":false},"authMethods":[],"agentInfo":{"name":"fake_agent","version":"0.0.0"}}}}
{"update":{"update":{"sessionUpdate":"available_commands_update","availableCommands":[]},"session":"sess-1"}}
{"update":{"update":{"sessionUpdate":"available_commands_update","availableCommands":[]},"session":"sess-1"}}
{"update":{"update":{"sessionUpdate":"available_commands_update","availableCommands":[]},"session":"sess-1"}}
{"expect":{"method":"session/new","as":"new","params":{"cwd":"{{any}}"}}}
"#;

/// After [`PACED_START`], never answers `session/new`: it waits on stdin, so it ends only when
/// the ladder closes that, and its own exit cannot race the updates.
const PACED_SILENT: &str = r#"{"expect":{"method":"session/prompt","as":"never","params":{}}}
"#;

/// After [`PACED_START`], answers `session/new` and the question.
const PACED_ANSWERS: &str = r#"{"respond":{"to":"new","result":{"sessionId":"sess-1"}}}
{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
"#;

/// Less than the bound, three times over: 210 s in all, longer than [`START_WAIT`]. Short
/// enough that a timer the held updates keep waiting is never handled more than [`START_WAIT`]
/// late, which the bound would read as sleep (`fails_after_silence` handles it before its jump).
const STEP: Duration = Duration::from_secs(70);

const MS: Duration = Duration::from_millis(1);

/// Sends a prompt and runs to the agent's launch.
pub(super) fn launched(rig: &mut Rig) -> (ThreadId, TurnId) {
    let thread = rig.thread();
    let turn = send_prompt(rig, 10, thread);
    rig.run_until("the agent's launch", |actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| agent.stage == Stage::Initializing)
    });
    (thread, turn)
}

/// `thread`'s agent's stage.
pub(super) fn stage(rig: &Rig, thread: ThreadId) -> Option<Stage> {
    rig.actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .map(|agent| agent.stage)
}

/// Handles each of the agent's three updates [`STEP`] after the one before.
fn paced_updates(rig: &mut Rig) {
    let update = |_: &Actor, input: &Input| {
        matches!(
            input,
            Input::Agent {
                input: AgentInput::Event(_),
                ..
            }
        )
    };
    for _ in 0..3 {
        rig.hold_next("the agent's update", update);
        rig.clock.advance(STEP);
        rig.release_held();
    }
}

/// Lets `time` pass in steps of at most [`TICK`], each step's looks handled at their time, so
/// none reads as late (sleep). Nothing may stop meanwhile: draining races nothing then.
pub(super) fn pass(rig: &mut Rig, time: Duration) {
    let mut left = time;
    while !left.is_zero() {
        let step = left.min(TICK);
        rig.clock.advance(step);
        rig.drain();
        left = left.saturating_sub(step);
    }
}

/// The agent stays silent for the `left` of its bound: just short of it it still starts; at it
/// it is stopped through the ladder and its turn fails. Gives the code it failed with.
pub(super) fn fails_after(
    rig: &mut Rig,
    thread: ThreadId,
    turn: TurnId,
    left: Duration,
) -> Option<ErrorCode> {
    // A look already due waits behind the held updates: handle it now, at its time.
    rig.drain();
    pass(rig, left.checked_sub(MS).unwrap());
    assert!(
        matches!(
            stage(rig, thread),
            Some(Stage::Initializing | Stage::Loading)
        ),
        "stopped before the bound: {:?}",
        stage(rig, thread)
    );
    // The clock alone moves: `hold_next` handles the deadline and holds the agent's end.
    rig.clock.advance(MS);
    let agent_end = |_: &Actor, input: &Input| {
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

/// [`fails_after`] the whole bound.
pub(super) fn fails_after_silence(
    rig: &mut Rig,
    thread: ThreadId,
    turn: TurnId,
) -> Option<ErrorCode> {
    fails_after(rig, thread, turn, START_WAIT)
}

/// Runs a prompt to the silent agent's launch and lets the bound run out.
pub(super) fn timed_out(rig: &mut Rig) -> Option<ErrorCode> {
    let (thread, turn) = launched(rig);
    fails_after_silence(rig, thread, turn)
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
fn a_start_that_never_answers_and_whose_probe_misses_while_online_is_start_unreachable() {
    let (probe, count) = ReachProbe::fixed(false).counted();
    let mut rig = start_rig(
        "start-timeout-offline",
        SILENT,
        Some(AgentKind::Claude),
        probe,
    );
    assert_eq!(timed_out(&mut rig), Some(ErrorCode::AgentStartUnreachable));
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

/// A long replay: the agent keeps sending for longer than the bound in all, never silent for
/// it, and its session attaches.
#[test]
fn a_start_that_keeps_sending_past_the_bound_succeeds() {
    let steps = format!("{PACED_START}{PACED_ANSWERS}");
    let mut rig = start_rig("start-paced", &steps, None, ReachProbe::fixed(true));
    let (thread, turn) = launched(&mut rig);
    paced_updates(&mut rig);
    until_ended(&mut rig, thread);
    let (stop, code) = ended(&rig.log(thread), turn);
    assert_eq!((stop, code), (Some(StopReason::EndTurn), None));
    rig.finish();
}

/// The same agent going silent after its updates fails once it was silent for the bound.
#[test]
fn a_start_that_goes_silent_after_sending_fails_at_the_bound() {
    let steps = format!("{PACED_START}{PACED_SILENT}");
    let mut rig = start_rig("start-paced-silent", &steps, None, ReachProbe::fixed(true));
    let (thread, turn) = launched(&mut rig);
    paced_updates(&mut rig);
    assert_eq!(
        fails_after_silence(&mut rig, thread, turn),
        Some(ErrorCode::AgentLaunchFailed)
    );
    rig.finish();
}

/// A lid closed 10 s into a start and opened 3 minutes later, on a clock that ran on while the
/// computer slept (Windows'): the look due at 15 s comes 175 s late, so that time is left out.
/// The start goes on, and fails only once the agent was silent 120 s awake: 105 s after waking.
#[test]
fn a_long_sleep_mid_start_is_left_out_of_the_silence() {
    let mut rig = start_rig("start-slept", SILENT, None, ReachProbe::fixed(true));
    let (thread, turn) = launched(&mut rig);
    pass(&mut rig, Duration::from_secs(10));
    rig.clock.advance(Duration::from_secs(180));
    rig.drain();
    assert_eq!(
        stage(&rig, thread),
        Some(Stage::Initializing),
        "failed on waking"
    );
    assert_eq!(
        fails_after(&mut rig, thread, turn, Duration::from_secs(105)),
        Some(ErrorCode::AgentLaunchFailed)
    );
    rig.finish();
}

/// A sleep from 10 s to 100 s, over before the bound was due: the look due at 15 s comes 85 s
/// late, so the agent keeps its waking time and fails at 205 s, not at 120 s.
#[test]
fn a_short_sleep_over_before_the_bound_is_left_out_too() {
    let mut rig = start_rig("start-slept-short", SILENT, None, ReachProbe::fixed(true));
    let (thread, turn) = launched(&mut rig);
    pass(&mut rig, Duration::from_secs(10));
    rig.clock.advance(Duration::from_secs(90));
    rig.drain();
    assert_eq!(
        fails_after(&mut rig, thread, turn, Duration::from_secs(105)),
        Some(ErrorCode::AgentLaunchFailed)
    );
    rig.finish();
}
