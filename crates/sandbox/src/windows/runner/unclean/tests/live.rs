//! F4 and a live session: an open marker whose outer still runs refuses the logon as busy, with
//! no stop requested and nothing written; any other open marker is a dead outer's, as before.

use super::*;

/// The outer the open markers name.
const OUTER: Outer = Outer {
    pid: 4242,
    created: 133_000_000_000_000_000,
};
/// What a dead outer's marker becomes: the fake's `StopNext`, read then.
const DEAD: MarkerState = MarkerState::AwaitingStop { stop: 5 };

/// An open marker of pool user A naming [`OUTER`], whose pid opens as `seen`.
fn open_with(seen: Option<OuterSeen>) -> Fake {
    let mut fake = Fake::new();
    fake.marker("s1", Some(POOL_A), Some(MarkerState::Open));
    fake.outers.insert("s1".to_owned(), OUTER);
    if let Some(seen) = seen {
        fake.processes.insert(OUTER.pid, seen);
    }
    fake
}

/// F4 took `fake`'s marker `s1` for a dead outer's: it awaits a stop, which was requested.
fn refused_as_dead(fake: &mut Fake) {
    let refused = logon_check(fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop, "{refused:?}");
    assert_eq!(fake.state("s1"), Some(DEAD));
    assert_eq!(fake.asked("stop"), 1);
}

#[test]
fn a_running_outer_with_the_markers_creation_time_is_alive_and_refuses_as_busy() {
    let running = OuterSeen::Running {
        created: OUTER.created,
    };
    let mut fake = open_with(Some(running));
    // Another open marker of the same pool user, a dead outer's: still nothing is written.
    fake.marker("s0", Some(POOL_A), Some(MarkerState::Open));
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::PoolBusy);
    assert_eq!(refused.why, BUSY);
    assert!(
        !refused.why.contains("4242"),
        "the pid is for the log alone"
    );
    assert_eq!(fake.asked("stop"), 0, "a stop would end the live session");
    assert_eq!(fake.asked("read StopNext"), 0);
    assert_eq!(fake.state("s1"), Some(MarkerState::Open));
    assert_eq!(fake.state("s0"), Some(MarkerState::Open));
    assert_eq!(
        fake.asked("note the session s1's outer (pid 4242) still runs"),
        1
    );
    // Another pool user's logon isn't held by it.
    assert_eq!(logon_check(&mut fake, &sid(POOL_B)), Ok(()));
}

#[test]
fn an_outer_whose_pid_is_gone_is_dead() {
    refused_as_dead(&mut open_with(None));
    refused_as_dead(&mut open_with(Some(OuterSeen::Gone)));
}

#[test]
fn an_outer_created_at_another_time_is_dead() {
    // A later process given the same pid.
    let reused = OuterSeen::Running {
        created: OUTER.created + 1,
    };
    refused_as_dead(&mut open_with(Some(reused)));
}

#[test]
fn an_outer_the_lawyer_cant_open_is_dead() {
    refused_as_dead(&mut open_with(Some(OuterSeen::Denied)));
    refused_as_dead(&mut open_with(Some(OuterSeen::Failed)));
}

#[test]
fn an_outer_that_exited_is_dead_though_its_object_is_kept() {
    refused_as_dead(&mut open_with(Some(OuterSeen::Exited)));
}

#[test]
fn an_open_marker_from_before_the_outer_was_recorded_is_a_dead_outers() {
    let mut fake = Fake::new();
    fake.marker("s1", Some(POOL_A), Some(MarkerState::Open));
    refused_as_dead(&mut fake);
    assert!(
        !fake.log.iter().any(|line| line.starts_with("open ")),
        "no pid to open: {:?}",
        fake.log
    );
}

#[test]
fn only_an_open_marker_is_checked_for_a_live_outer() {
    // A marker that waits for a task's run names no outer once settled; one naming it anyway
    // (written by hand) isn't taken for a live session.
    let running = OuterSeen::Running {
        created: OUTER.created,
    };
    let mut fake = open_with(Some(running));
    fake.marker(
        "s1",
        Some(POOL_A),
        Some(MarkerState::AwaitingStop { stop: 3 }),
    );
    fake.done = Sequences { stop: 2, sweep: 0 };
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop);
}
