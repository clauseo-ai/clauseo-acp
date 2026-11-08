//! The launch on a prepared reservation, and the session's end.

use super::fake::{MATTER, OTHER, World};
use super::*;
use crate::{Denial, DenialSink};

struct NoDenials;

impl DenialSink for NoDenials {
    fn denied(&self, _denial: Denial) {}
}

/// Launches `reservation` for `session` of `matter` under a policy for `matter_dir`.
fn launch(
    launchers: &Launchers<FakeSteps>,
    dir: &TempDir,
    reservation: Reservation,
    (matter, session, matter_dir): (&str, &str, &str),
) -> Result<(Arc<dyn clauseo_process::Launcher>, Vec<SetAside>), Error> {
    launchers.launch(
        reservation,
        policy_at(dir, matter_dir),
        matter,
        session,
        SpawnOptions::new(),
        Arc::new(NoDenials),
    )
}

/// The launch's error; it must have been refused.
fn refused<T, E>(result: Result<T, E>) -> E {
    let Err(error) = result else {
        panic!("launched");
    };
    error
}

/// The launch refuses a reservation of another matter or session, of other launchers, or for
/// another matter folder (`WrongReservation`), and one not prepared (`NotPrepared`). Each
/// refused reservation is released.
#[test]
fn the_launch_checks_its_reservation() {
    let world = World::new();
    let dir = TempDir::new("launch-checks");
    let launchers = launchers(&world);
    let cases = [
        (MATTER, "s1", "other", "WrongReservation"),
        (OTHER, "s1", "matter", "WrongReservation"),
        (MATTER, "s2", "matter", "WrongReservation"),
    ];
    for (matter, session, folder, want) in cases {
        let reservation = prepared(&launchers, &world, &dir, "s1");
        let error = refused(launch(
            &launchers,
            &dir,
            reservation,
            (matter, session, folder),
        ));
        assert_eq!(format!("{error:?}"), want);
        let calls = world.calls();
        assert!(calls[0].starts_with("end_run "), "{calls:?}");
        assert_eq!(calls[1..], UNDO, "released");
    }
    let reserved = launchers.reserve(MATTER, "s1", &[]).unwrap();
    world.calls();
    let error = refused(launch(&launchers, &dir, reserved, (MATTER, "s1", "matter")));
    assert!(matches!(error, Error::NotPrepared), "{error:?}");
    assert_eq!(world.calls(), UNDO);
    let others = Launchers::with_steps(FakeSteps(Arc::clone(&world))).unwrap();
    for foreign in [
        prepared(&others, &world, &dir, "s1"),
        Reservation::fixed(vec![46100], MATTER, "s1"),
        Reservation::unpooled(MATTER, "s1"),
    ] {
        world.calls();
        let error = refused(launch(&launchers, &dir, foreign, (MATTER, "s1", "matter")));
        assert!(matches!(error, Error::WrongReservation), "{error:?}");
    }
}

/// A reservation whose walk failed is refused (`NotPrepared`): the walk undid its work
/// already, so the refusal has nothing left to undo, and it frees the matter.
#[test]
fn the_launch_refuses_a_failed_walk() {
    let world = World::new();
    let dir = TempDir::new("launch-failed-walk");
    let launchers = launchers(&world);
    world.fail_at("check_launch");
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    let preparing = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    assert!(matches!(preparing.wait(WAIT), Some(Err(Error::Policy(_)))));
    assert!(matches!(preparing.result(), Some(Err(Error::NotPrepared))));
    let again = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    assert!(matches!(again.result(), Some(Err(Error::NotPrepared))));
    let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(
        matches!(busy, Error::MatterBusy),
        "held until dropped: {busy:?}"
    );
    world.calls();
    let error = refused(launch(
        &launchers,
        &dir,
        reservation,
        (MATTER, "s1", "matter"),
    ));
    assert!(matches!(error, Error::NotPrepared), "{error:?}");
    assert_eq!(world.calls(), Vec::<String>::new());
    world.fail_at("none");
    assert!(launchers.reserve(MATTER, "s2", &[]).is_ok());
}

/// A relaunch right after a session's end waits for the end sweeper the outer started: the
/// matter is busy (`MatterBusy`, the host retries) while it runs, with nothing claimed or run,
/// and launches once it is done, so the two never run together.
#[test]
fn a_relaunch_waits_for_the_end_sweeper() {
    let world = World::new();
    let dir = TempDir::new("launch-end-sweeper");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    // The outer starts its end sweeper as the session ends, before the host's pass.
    world.sweeper(true);
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    world.calls();
    for _ in 0..3 {
        let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
        assert!(matches!(busy, Error::MatterBusy), "{busy:?}");
        let calls = world.calls();
        assert_eq!(calls, [&RESERVE[..4], &["close_session"]].concat());
        assert!(world.claimed_slots().is_empty());
    }
    world.sweeper(false);
    let reservation = prepared(&launchers, &world, &dir, "s2");
    launch(&launchers, &dir, reservation, (MATTER, "s2", "matter")).unwrap();
}

/// A process of the pool user's found at the launch is refused plainly, the reservation
/// released: the preparation stops leftovers, and nothing should run as it since.
#[test]
fn the_launch_refuses_a_leftover_process() {
    let world = World::new();
    let dir = TempDir::new("launch-leftover-late");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    world.leftovers(3, false);
    let error = refused(launch(
        &launchers,
        &dir,
        reservation,
        (MATTER, "s1", "matter"),
    ));
    assert!(
        matches!(error, Error::LeftoverProcess { count: 3 }),
        "{error:?}"
    );
    assert_eq!(
        world.calls(),
        ["pool_processes", "end_run 1", UNDO[0], UNDO[1]]
    );
    assert!(world.claimed_slots().is_empty());
}

/// A prepared reservation gives the launcher and what the launch pass moved; the launcher
/// writes the launch file, then starts the outer, once.
#[test]
fn the_launch_hands_out_one_agents_launcher() {
    let world = World::new();
    let dir = TempDir::new("launch-launcher");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let (launcher, moved) =
        launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    assert_eq!(moved, [fake::moved("CLAUDE.md")]);
    // The leftover check; no release: the reservation is the session's now.
    assert_eq!(world.calls(), ["pool_processes"]);
    let program = dir.file("bin/agent.exe", "");
    let spec = clauseo_process::LaunchSpec::new(
        program,
        Vec::new(),
        std::collections::BTreeMap::new(),
        dir.dir("matter"),
    )
    .unwrap();
    let error = refused(launcher.launch(&spec));
    assert!(
        matches!(error, clauseo_process::Error::Closing),
        "{error:?}"
    );
    assert_eq!(world.calls(), ["write_launch", "spawn_outer"]);
    let again = refused(launcher.launch(&spec));
    assert!(
        matches!(again, clauseo_process::Error::InvalidSpec(_)),
        "{again:?}"
    );
    assert_eq!(world.calls(), Vec::<String>::new());
}

/// The session's end, in 0049's order; then the slot goes back and the matter is free.
#[test]
fn after_session_runs_in_order_and_frees_the_matter() {
    let world = World::new();
    let dir = TempDir::new("launch-after");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    world.calls();
    let moved = launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    assert_eq!(moved, [fake::moved("AGENTS.md")]);
    assert_eq!(
        world.calls(),
        [
            "session_cursor",
            "repair",
            "end_reads",
            "set_aside_after",
            "write_clean_close cursor=true repaired=true passed=true",
            "end_run 1",
            "release_slot",
        ]
    );
    assert!(world.claimed_slots().is_empty());
    assert!(world.open_sessions(MATTER).is_empty());
    assert!(launchers.reserve(MATTER, "s2", &[]).is_ok());
}

/// A failed repair or pass is handed to the clean-close record as failed, and the end goes on.
#[test]
fn after_session_goes_on_past_failures() {
    for step in ["repair", "set_aside_after"] {
        let world = World::new();
        let dir = TempDir::new(&format!("launch-after-{step}"));
        let launchers = launchers(&world);
        let reservation = prepared(&launchers, &world, &dir, "s1");
        launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
        world.fail_at(step);
        let moved = launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
        let calls = world.calls();
        let close = if step == "repair" {
            "write_clean_close cursor=true repaired=false passed=true"
        } else {
            assert_eq!(moved, Vec::<SetAside>::new());
            "write_clean_close cursor=true repaired=true passed=false"
        };
        assert!(calls.contains(&close.to_owned()), "{step}: {calls:?}");
        assert_eq!(
            calls[calls.len() - 2..],
            ["end_run 1", "release_slot"],
            "{step}"
        );
        assert!(launchers.reserve(MATTER, "s2", &[]).is_ok(), "{step}");
    }
}

/// One live agent per matter: busy while the session runs, still busy once its job is gone
/// and its after-session pass is running, free once the pass has finished.
#[test]
fn the_matter_is_busy_until_its_after_session_pass_finishes() {
    let world = World::new();
    let dir = TempDir::new("launch-busy");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    let live = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(live, Error::MatterBusy), "{live:?}");
    let held = world.hold("set_aside_after");
    let policy = policy_at(&dir, "matter");
    std::thread::scope(|scope| {
        let ending = scope.spawn(|| launchers.after_session(MATTER, "s1", &policy));
        held.reached();
        let ending_busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
        assert!(matches!(ending_busy, Error::MatterBusy), "{ending_busy:?}");
        held.go();
        ending.join().unwrap();
    });
    assert!(launchers.reserve(MATTER, "s2", &[]).is_ok());
}

/// A session these launchers didn't launch is left alone, and so is its matter's holder.
#[test]
fn after_session_leaves_a_session_it_didnt_launch() {
    let world = World::new();
    let dir = TempDir::new("launch-after-unknown");
    let launchers = launchers(&world);
    let policy = policy_at(&dir, "matter");
    assert_eq!(
        launchers.after_session(MATTER, "s1", &policy),
        Vec::<SetAside>::new()
    );
    let reservation = prepared(&launchers, &world, &dir, "s1");
    // Prepared, not launched: its end isn't `after_session`'s.
    assert_eq!(
        launchers.after_session(MATTER, "s1", &policy),
        Vec::<SetAside>::new()
    );
    assert_eq!(world.calls(), Vec::<String>::new());
    drop(reservation);
    assert!(launchers.registry.wait_free(MATTER, WAIT));
    assert_eq!(world.calls(), ["end_run 1", UNDO[0], UNDO[1]]);
}
