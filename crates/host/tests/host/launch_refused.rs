//! `HostHandle::set_launch_refused` (decision record 0048, P4d): desktop refuses an agent id's
//! new launches with a code from the host's list. A turn that would launch the agent fails with
//! an ordinary `TurnFailed` of that code, and nothing launches; a session already running goes
//! on. `None` lets launches start again.

use std::sync::Arc;

use clauseo_protocol::{AgentId, ErrorCode, StopReason, StreamId};

use crate::codex::{Recording, turn_failed};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, wait};

/// The fake agent's id.
fn fake() -> AgentId {
    AgentId::new(FAKE).unwrap()
}

#[test]
fn a_refused_launch_fails_its_turn_with_the_code_and_a_running_session_goes_on() {
    let dir = Dir::new(
        "launch_refused",
        &[(FAKE, Scenario::Local("launch-refused"))],
    );
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let host = dir.start_launching(0, Arc::new(recording), |config| config);
    let (matter, running, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(running));
    let (first, _) = host.prompt(running, "Look at the matter.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);

    let refused = Some(ErrorCode::SigninPolicyUnreadable);
    wait(
        "set_launch_refused",
        host.handle.set_launch_refused(fake(), refused),
    )
    .unwrap();
    // The running session takes the next prompt.
    let (second, _) = host.prompt(running, "Look again.");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);

    // A new thread's launch is refused: its turn fails with the code, and nothing launches.
    let other = host.create_thread(matter, FAKE);
    let mut other_events = host.events(StreamId::Thread(other));
    let (refused_turn, _) = host.prompt(other, "Look at the matter.");
    assert_eq!(
        other_events.until_turn_ended(refused_turn).stop,
        StopReason::Failed
    );
    let failed = turn_failed(&other_events, refused_turn)
        .unwrap_or_else(|| panic!("{}", other_events.dump()));
    assert_eq!(failed.code, ErrorCode::SigninPolicyUnreadable);
    assert_eq!(launches.lock().unwrap().len(), 1, "nothing launched");

    // Cleared: the next launch starts.
    wait(
        "set_launch_refused",
        host.handle.set_launch_refused(fake(), None),
    )
    .unwrap();
    let (again, _) = host.prompt(other, "Look at the matter.");
    assert_eq!(
        other_events.until_turn_ended(again).stop,
        StopReason::EndTurn
    );
    assert_eq!(launches.lock().unwrap().len(), 2);
    host.settle();
    events.drain_ready();
    other_events.drain_ready();
    events.assert_no_mismatch();
    other_events.assert_no_mismatch();
    host.shutdown();
}
