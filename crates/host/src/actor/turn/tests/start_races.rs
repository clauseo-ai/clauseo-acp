//! A start meets the lawyer or the host: a Cancel, a shutdown, or the next prompt's relaunch,
//! during the start itself or while its failure waits for the offline probe
//! (`offline/start.rs`). None of them may leave a start code behind, a probe waiting, or an old
//! agent's bound cutting a new start short.

use std::time::Duration;

use clauseo_protocol::{AgentKind, ErrorCode, HostCommand, StopReason, ThreadId, TurnId};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline::ended;
use super::rig::Rig;
use super::start_offline::{REFUSED, failing_start, start_rig};
use super::start_timeout::{SILENT, fails_after, fails_after_silence, launched, pass, stage};
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::TurnInput;
use crate::actor::turn::agents::Stage;
use crate::actor::turn::agents::start_timer::START_WAIT;
use crate::actor::turn::offline::probe::ReachProbe;

/// The codes a start's own failure ends with: none may follow a Cancel or a shutdown.
const START_CODES: [ErrorCode; 3] = [
    ErrorCode::AgentLaunchFailed,
    ErrorCode::AgentStartOffline,
    ErrorCode::AgentStartUnreachable,
];

/// Sends a prompt to the failing agent and holds the probe's answer to its failed start.
fn probing(rig: &mut Rig) -> (ThreadId, TurnId) {
    let thread = rig.thread();
    let turn = send_prompt(rig, 10, thread);
    let probe =
        |_: &Actor, input: &Input| matches!(input, Input::Turn(TurnInput::OfflineProbe { .. }));
    rig.hold_next("the probe's answer", probe);
    assert!(rig.actor.turn_path.start_probes.contains_key(&thread));
    (thread, turn)
}

/// Cancels `thread`'s turn as command `n` and lets its fallback run out.
fn cancel(rig: &mut Rig, n: u128, thread: ThreadId) {
    rig.dispatch(n, HostCommand::Cancel { thread }).unwrap();
    pass(rig, super::super::CANCEL_WAIT);
}

#[test]
fn a_cancel_during_a_start_ends_it_cancelled_and_its_bound_with_it() {
    let mut rig = start_rig("start-cancel", SILENT, None, ReachProbe::fixed(true));
    let (thread, turn) = launched(&mut rig);
    cancel(&mut rig, 11, thread);
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Cancelled), None)
    );
    // The bound's next look finds no starting agent and goes.
    pass(&mut rig, START_WAIT);
    assert!(rig.actor.turn_path.start_bounds.is_empty());
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Cancelled), None)
    );
    rig.finish();
}

#[test]
fn a_cancel_while_the_failed_start_waits_for_the_probe_leaves_no_start_code() {
    let probe = ReachProbe::fixed(false);
    let mut rig = failing_start(
        "start-cancel-probe",
        REFUSED,
        Some(AgentKind::Claude),
        probe,
    );
    let (thread, turn) = probing(&mut rig);
    cancel(&mut rig, 11, thread);
    rig.release_held();
    until_ended(&mut rig, thread);
    let (stop, code) = ended(&rig.log(thread), turn);
    assert_eq!(stop, Some(StopReason::Cancelled));
    assert!(
        code.is_none_or(|code| !START_CODES.contains(&code)),
        "{code:?}"
    );
    assert!(rig.actor.turn_path.start_probes.is_empty());
    rig.finish();
}

#[test]
fn a_shutdown_during_a_start_leaves_no_start_code() {
    let mut rig = start_rig("start-shutdown", SILENT, None, ReachProbe::fixed(true));
    let (thread, turn) = launched(&mut rig);
    let mut rig = rig.restart(1);
    let (_, code) = ended(&rig.log(thread), turn);
    assert!(
        code.is_none_or(|code| !START_CODES.contains(&code)),
        "{code:?}"
    );
    rig.finish();
}

#[test]
fn a_shutdown_while_the_failed_start_waits_for_the_probe_leaves_no_start_code() {
    let probe = ReachProbe::fixed(false);
    let mut rig = failing_start(
        "start-shutdown-probe",
        REFUSED,
        Some(AgentKind::Claude),
        probe,
    );
    let (thread, turn) = probing(&mut rig);
    let mut rig = rig.restart(1);
    let (_, code) = ended(&rig.log(thread), turn);
    assert!(
        code.is_none_or(|code| !START_CODES.contains(&code)),
        "{code:?}"
    );
    rig.finish();
}

/// The next prompt relaunches the agent after a start that timed out: the old agent's bound is
/// gone, so the new start gets its whole wait.
#[test]
fn a_relaunch_after_a_timed_out_start_gets_its_whole_bound() {
    let mut rig = start_rig("start-relaunch", SILENT, None, ReachProbe::fixed(true));
    let (thread, first) = launched(&mut rig);
    assert_eq!(
        fails_after_silence(&mut rig, thread, first),
        Some(ErrorCode::AgentLaunchFailed)
    );
    let turn = send_prompt(&mut rig, 11, thread);
    rig.run_until("the relaunch", |actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| agent.stage == Stage::Initializing)
    });
    // Half the bound in: a look of the old agent would have ended it by now.
    pass(&mut rig, Duration::from_secs(60));
    assert_eq!(stage(&rig, thread), Some(Stage::Initializing));
    assert_eq!(rig.actor.turn_path.start_bounds.len(), 1);
    assert_eq!(
        fails_after(
            &mut rig,
            thread,
            turn,
            START_WAIT.checked_sub(Duration::from_secs(60)).unwrap()
        ),
        Some(ErrorCode::AgentLaunchFailed)
    );
    rig.finish();
}
