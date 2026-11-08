//! The preparation: its order, the launch check before any grant, each failure and cancel
//! undone, `PrepareRun`'s retries, one walk per matter, and the reservation's drop.

use super::fake::{FakeSteps, Held, MATTER, OTHER, World};
use super::*;

/// The walk's steps after the reserve, in order.
const WALK: [&str; 7] = [
    "plan",
    "check_launch",
    "check_network",
    "pool_processes",
    "before_launch",
    "prepare_run",
    "apply",
];
/// What grants the pool user anything or runs as it (or ends its processes).
const GRANTS: [&str; 6] = [
    "stop",
    "sweep",
    "prepare_run",
    "apply",
    "write_launch",
    "spawn_outer",
];

/// The calls of a preparation of `MATTER` for `s1`, after its reserve, and its result.
fn prepare_once(world: &Arc<World>, dir: &TempDir) -> (Vec<String>, Result<(), Error>) {
    let launchers = launchers(world);
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    world.calls();
    let preparing = launchers.prepare(&reservation, &policy_at(dir, "matter"));
    let result = preparing.wait(WAIT).unwrap();
    (world.calls(), result)
}

#[test]
fn the_walk_runs_in_order_and_grants_only_after_the_check() {
    let world = World::new();
    let dir = TempDir::new("launch-walk-order");
    let launchers = launchers(&world);
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    let preparing = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    preparing.wait(WAIT).unwrap().unwrap();
    let calls = world.calls();
    assert_eq!(calls, [&RESERVE[..], &WALK[..]].concat());
    let check = calls
        .iter()
        .position(|call| call == "check_launch")
        .unwrap();
    for (at, call) in calls.iter().enumerate() {
        if GRANTS.contains(&call.as_str()) {
            assert!(at > check, "{call} before the launch check");
        }
    }
    assert_eq!(
        (reservation.matter(), reservation.session()),
        (MATTER, "s1")
    );
    let progress = preparing.progress();
    assert_eq!(
        progress,
        Progress {
            stage: Stage::Finished,
            walked: 3,
            stopped: 0,
            done: true,
        }
    );
    assert!(matches!(preparing.result(), Some(Ok(()))), "every time");
}

/// A failed launch check undoes the reservation's work: nothing granted, nothing run, the slot
/// and the mark undone (`launch` then refuses it: `session.rs`).
#[test]
fn a_failed_check_drops_the_reservation() {
    let world = World::new();
    let dir = TempDir::new("launch-check-fails");
    world.fail_at("check_launch");
    let (calls, result) = prepare_once(&world, &dir);
    assert!(matches!(result, Err(Error::Policy(_))), "{result:?}");
    assert_eq!(calls, [&WALK[..2], &UNDO[..]].concat());
    assert!(world.claimed_slots().is_empty());
    assert!(world.open_sessions(MATTER).is_empty());
}

/// Each step's failure comes through as its error and undoes what was done: the run's group
/// once `PrepareRun` made one, then the slot and the mark.
#[test]
fn each_failure_comes_through_and_is_undone() {
    for (at, step) in WALK.iter().enumerate() {
        let world = World::new();
        let dir = TempDir::new(&format!("launch-fails-{step}"));
        world.fail_at(step);
        let (calls, result) = prepare_once(&world, &dir);
        let error = result.unwrap_err();
        let mut want: Vec<String> = WALK[..=at].iter().map(|s| (*s).to_owned()).collect();
        if *step == "apply" {
            want.push("end_run 1".to_owned());
        }
        want.extend(UNDO.map(str::to_owned));
        assert_eq!(calls, want, "{step}");
        let launch = matches!(&error, Error::Launch(clauseo_process::Error::Os { what, .. })
            if what.contains("couldn't prepare the run"));
        match *step {
            "prepare_run" => assert!(launch, "{error:?}"),
            _ => assert!(matches!(error, Error::Policy(_)), "{step}: {error:?}"),
        }
        assert!(world.claimed_slots().is_empty(), "{step}");
    }
}

/// `PrepareRun`'s loose-task refusal is the `loose_task` block.
#[test]
fn a_loose_task_blocks_the_launch() {
    let world = World::new();
    let dir = TempDir::new("launch-loose-task");
    world.run_answers([RunRefusal::LooseTask]);
    let (calls, result) = prepare_once(&world, &dir);
    let error = result.unwrap_err();
    assert!(
        matches!(error, Error::Blocked(crate::Blocked::LooseTask)),
        "{error:?}"
    );
    assert_eq!(calls, [&WALK[..6], &UNDO[..]].concat());
}

/// A leftover process of the pool user's is stopped (the stop task, then the sweep) and looked
/// for again, after the launch check; the walk goes on, the stop's count in the progress.
#[test]
fn a_leftover_is_stopped_and_the_walk_goes_on() {
    let world = World::new();
    let dir = TempDir::new("launch-leftover");
    world.leftovers(2, false);
    let launchers = launchers(&world);
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    world.calls();
    let preparing = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    preparing.wait(WAIT).unwrap().unwrap();
    assert_eq!(preparing.progress().stopped, 2);
    let calls = world.calls();
    let want = [&WALK[..4], &["stop", "sweep", "pool_processes"], &WALK[4..]].concat();
    assert_eq!(calls, want);
    let check = calls
        .iter()
        .position(|call| call == "check_launch")
        .unwrap();
    for (at, call) in calls.iter().enumerate() {
        if GRANTS.contains(&call.as_str()) {
            assert!(at > check, "{call} before the launch check");
        }
    }
    assert_eq!(world.pauses(), 0);
}

/// A leftover that survives every stop is refused plainly once the bound has passed; the stop
/// is tried again after each pause meanwhile, and nothing is set aside or granted.
#[test]
fn a_leftover_that_survives_is_refused_at_the_bound() {
    let world = World::new();
    let dir = TempDir::new("launch-leftover-stays");
    world.leftovers(1, true);
    let (calls, result) = prepare_once(&world, &dir);
    let error = result.unwrap_err();
    assert!(
        matches!(error, Error::LeftoverProcess { count: 1 }),
        "{error:?}"
    );
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    assert!(world.now() >= PREPARE_BOUND);
    let pauses = PREPARE_BOUND.as_secs() / RETRY_PAUSE.as_secs();
    assert_eq!(u64::from(world.pauses()), pauses);
    let stops = calls.iter().filter(|call| *call == "stop").count();
    assert_eq!(u64::try_from(stops).unwrap(), pauses);
    for never in ["before_launch", "prepare_run", "apply"] {
        assert!(!calls.contains(&never.to_owned()), "{never}");
    }
    assert_eq!(calls[calls.len() - 2..], UNDO);
}

/// A running set-up keeps the preparation waiting: `PrepareRun` is tried again after each
/// pause, and the walk goes on once it answers.
#[test]
fn prepare_run_is_retried_while_a_set_up_runs() {
    let world = World::new();
    let dir = TempDir::new("launch-retry");
    world.run_answers([RunRefusal::SetUpRunning, RunRefusal::SetUpRunning]);
    let (calls, result) = prepare_once(&world, &dir);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(world.pauses(), 2);
    let tries = calls.iter().filter(|call| *call == "prepare_run").count();
    assert_eq!(tries, 3);
    assert_eq!(calls.last().map(String::as_str), Some("apply"));
}

/// A set-up still running at the bound fails the preparation plainly, undone.
#[test]
fn prepare_run_gives_up_at_the_bound() {
    let world = World::new();
    let dir = TempDir::new("launch-bound");
    world.setup_always_running();
    let (calls, result) = prepare_once(&world, &dir);
    let error = result.unwrap_err();
    assert!(
        matches!(&error, Error::Launch(clauseo_process::Error::Os { what, .. })
            if what.contains("still running")),
        "{error:?}"
    );
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    assert!(world.now() >= PREPARE_BOUND);
    let pauses = PREPARE_BOUND.as_secs() / RETRY_PAUSE.as_secs();
    assert_eq!(u64::from(world.pauses()), pauses);
    assert!(!calls.contains(&"apply".to_owned()));
    assert_eq!(calls[calls.len() - 2..], UNDO);
}

/// A preparation of `MATTER` held at `step`, with its handle and its reservation.
fn held_at(
    world: &Arc<World>,
    dir: &TempDir,
    step: &'static str,
) -> (Launchers<FakeSteps>, Preparing, Held, Reservation) {
    let launchers = launchers(world);
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    world.calls();
    let held = world.hold(step);
    let preparing = launchers.prepare(&reservation, &policy_at(dir, "matter"));
    held.reached();
    (launchers, preparing, held, reservation)
}

/// The calls of the walk up to `step`, then its undo.
fn undone_at(step: &str) -> Vec<String> {
    let at = WALK.iter().position(|each| *each == step).unwrap();
    let mut want: Vec<String> = WALK[..=at].iter().map(|s| (*s).to_owned()).collect();
    if at >= 5 {
        want.push("end_run 1".to_owned());
    }
    want.extend(UNDO.map(str::to_owned));
    want
}

/// A cancel at any step stops the walk there, with `Cancelled`, and undoes what was done.
#[test]
fn a_cancel_at_each_step_undoes_what_was_done() {
    for step in WALK {
        let world = World::new();
        let dir = TempDir::new(&format!("launch-cancel-{step}"));
        let (_launchers, preparing, held, _reservation) = held_at(&world, &dir, step);
        preparing.cancel();
        held.go();
        let result = preparing.wait(WAIT).unwrap();
        assert!(
            matches!(result, Err(Error::Cancelled)),
            "{step}: {result:?}"
        );
        assert_eq!(world.calls(), undone_at(step), "{step}");
        assert!(world.claimed_slots().is_empty(), "{step}");
        assert!(world.open_sessions(MATTER).is_empty(), "{step}");
    }
}

/// Dropping the reservation while its walk is blocked mid-step returns at once (the walk holds
/// nothing the drop waits for), cancels the walk, and the walk undoes and frees the matter when
/// it stops.
#[test]
fn dropping_the_reservation_cancels_its_walk_at_once() {
    for step in ["before_launch", "apply"] {
        let world = World::new();
        let dir = TempDir::new(&format!("launch-drop-{step}"));
        let (launchers, preparing, held, reservation) = held_at(&world, &dir, step);
        // Would hang here if the drop waited for the walk, which waits for `go`.
        drop(reservation);
        let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
        assert!(matches!(busy, Error::MatterBusy), "{step}: {busy:?}");
        held.go();
        let result = preparing.wait(WAIT).unwrap();
        assert!(
            matches!(result, Err(Error::Cancelled)),
            "{step}: {result:?}"
        );
        assert!(launchers.registry.wait_free(MATTER, WAIT), "{step}");
        assert_eq!(world.calls(), undone_at(step), "{step}");
        assert!(world.claimed_slots().is_empty(), "{step}");
    }
}

/// Dropping a prepared reservation returns while its undo is still blocked mid-step: the undo
/// runs on a thread of its own, and the matter stays busy until it is done.
#[test]
fn a_drop_never_waits_for_its_undo() {
    let world = World::new();
    let dir = TempDir::new("launch-drop-undo");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let held = world.hold("close_session");
    drop(reservation);
    let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(busy, Error::MatterBusy), "{busy:?}");
    held.reached();
    held.go();
    assert!(launchers.registry.wait_free(MATTER, WAIT));
    assert_eq!(world.calls(), ["end_run 1", UNDO[0], UNDO[1]]);
    assert!(launchers.reserve(MATTER, "s2", &[]).is_ok());
}

/// One walk per matter: while one runs, the matter refuses another reservation, and a second
/// `prepare` of the same reservation joins it; another matter prepares meanwhile.
#[test]
fn a_matter_walks_once_at_a_time() {
    let world = World::new();
    let dir = TempDir::new("launch-one-walk");
    let (launchers, preparing, held, reservation) = held_at(&world, &dir, "apply");
    assert_eq!(preparing.progress().stage, Stage::Walking);
    assert!(!preparing.progress().done);
    let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(busy, Error::MatterBusy), "{busy:?}");
    let joined = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    let other = launchers.reserve(OTHER, "s3", &[]).unwrap();
    held.go();
    assert!(matches!(preparing.wait(WAIT), Some(Ok(()))));
    assert!(matches!(joined.wait(WAIT), Some(Ok(()))));
    let walks = world.calls().iter().filter(|call| *call == "apply").count();
    assert_eq!(walks, 1, "one walk");
    let theirs = launchers.prepare(&other, &policy_at(&dir, "other"));
    assert!(matches!(theirs.wait(WAIT), Some(Ok(()))));
}

/// A dropped handle doesn't stop the walk (the lawyer's Stop ends the wait only): a later
/// `prepare` of the reservation finds it prepared, with no walk again.
#[test]
fn a_dropped_handle_doesnt_stop_the_walk() {
    let world = World::new();
    let dir = TempDir::new("launch-dropped-handle");
    let (launchers, preparing, held, reservation) = held_at(&world, &dir, "apply");
    drop(preparing);
    held.go();
    let again = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    assert!(matches!(again.wait(WAIT), Some(Ok(()))));
    assert_eq!(world.calls(), WALK);
    let done = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    assert!(done.progress().done);
    assert!(matches!(done.result(), Some(Ok(()))));
    assert_eq!(world.calls(), Vec::<String>::new(), "prepared already");
}

/// A reservation these launchers don't hold is refused at once, nothing called; the end
/// sweeper is never taken for a leftover.
#[test]
fn prepare_refuses_reservations_it_doesnt_hold() {
    let world = World::new();
    let dir = TempDir::new("launch-foreign");
    let launchers = launchers(&world);
    let others = Launchers::with_steps(FakeSteps(Arc::clone(&world))).unwrap();
    let theirs = others.reserve(OTHER, "s9", &[]).unwrap();
    world.calls();
    for reservation in [
        &Reservation::fixed(vec![46100], MATTER, "s1"),
        &Reservation::unpooled(MATTER, "s1"),
        &theirs,
    ] {
        let preparing = launchers.prepare(reservation, &policy_at(&dir, "matter"));
        assert!(preparing.progress().done);
        let result = preparing.result().unwrap();
        assert!(matches!(result, Err(Error::WrongReservation)), "{result:?}");
    }
    assert_eq!(world.calls(), Vec::<String>::new());
}

/// The detached end sweeper runs as the pool user but is known exactly: it is never a leftover,
/// so it triggers no stop and no refusal.
#[test]
fn the_end_sweeper_is_never_a_leftover() {
    let world = World::new();
    let dir = TempDir::new("launch-sweeper-not-leftover");
    let launchers = launchers(&world);
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    world.calls();
    world.sweeper(true);
    let preparing = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    assert!(matches!(preparing.wait(WAIT), Some(Ok(()))));
    assert_eq!(world.calls(), WALK);
    assert_eq!(preparing.progress().stopped, 0);
}
