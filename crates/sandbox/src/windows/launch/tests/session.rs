//! The launch on a prepared reservation, and the session's end.

use super::fake::{MATTER, OTHER, World};
use super::*;
use crate::{Denial, DenialSink};

struct NoDenials;

impl DenialSink for NoDenials {
    fn denied(&self, _denial: Denial) {}
}

/// Launches `reservation` for `session` of `matter` under a policy for `matter_dir`.
pub(super) fn launch(
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
    let elsewhere = TempDir::new("launch-checks-folder");
    let others = Launchers::with_steps(FakeSteps::elsewhere(&world, elsewhere.path())).unwrap();
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
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s2", &[]);
    assert!(free.is_ok());
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
    let _launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
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

/// A spec the fake launcher can be given.
fn spec(dir: &TempDir) -> clauseo_process::LaunchSpec {
    let program = dir.file("bin/agent.exe", "");
    clauseo_process::LaunchSpec::new(
        program,
        Vec::new(),
        std::collections::BTreeMap::new(),
        dir.dir("matter"),
    )
    .unwrap()
}

/// A prepared reservation gives the launcher and what the launch pass moved; the launcher
/// writes the launch file, then starts the outer, once. The fake's outer never starts, so the
/// launcher undoes the session (the run's group, the slot, the mark) before its error returns,
/// and the matter is free.
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
    let spec = spec(&dir);
    let error = refused(launcher.launch(&spec));
    assert!(
        matches!(error, clauseo_process::Error::Closing),
        "{error:?}"
    );
    assert_eq!(
        world.calls(),
        [
            "create_pipe",
            "write_launch",
            "spawn_outer",
            "remove_launch",
            "end_run 1",
            UNDO[0],
            UNDO[1]
        ]
    );
    assert!(world.claimed_slots().is_empty());
    assert!(world.open_sessions(MATTER).is_empty());
    let policy = policy_at(&dir, "matter");
    assert_eq!(
        launchers.after_session(MATTER, "s1", &policy),
        Vec::<SetAside>::new()
    );
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s2", &[]);
    assert!(free.is_ok(), "free");
    world.calls();
    let again = refused(launcher.launch(&spec));
    assert!(
        matches!(again, clauseo_process::Error::InvalidSpec(_)),
        "{again:?}"
    );
    assert_eq!(world.calls(), Vec::<String>::new());
}

/// A launch file that can't be written fails the start before the outer: the session is undone
/// and the matter freed.
#[test]
fn a_failed_launch_file_undoes_the_session() {
    let world = World::new();
    let dir = TempDir::new("launch-file-fails");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let (launcher, _) = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    world.calls();
    world.fail_at("write_launch");
    let error = refused(launcher.launch(&spec(&dir)));
    assert!(
        matches!(error, clauseo_process::Error::Os { .. }),
        "{error:?}"
    );
    assert_eq!(
        world.calls(),
        ["create_pipe", "write_launch", "end_run 1", UNDO[0], UNDO[1]]
    );
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s2", &[]);
    assert!(free.is_ok(), "free");
}

/// A launcher whose session already ended (`after_session`) starts nothing: refused, as a
/// reservation no longer held.
#[test]
fn the_launcher_refuses_after_the_sessions_end() {
    let world = World::new();
    let dir = TempDir::new("launch-after-end");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let (launcher, _) = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    world.calls();
    let error = refused(launcher.launch(&spec(&dir)));
    assert!(
        matches!(&error, clauseo_process::Error::Os { what, .. }
            if what.contains("no longer held")),
        "{error:?}"
    );
    assert_eq!(
        world.calls(),
        Vec::<String>::new(),
        "nothing written or started"
    );
}

/// A panic while the launcher starts the outer is caught: the outer it may have started is
/// ended first, then the session undone and the matter freed. If the outer can't be ended, the
/// matter is stuck, never freed.
#[test]
fn a_panic_in_the_launcher_ends_the_outer_first() {
    for end_fails in [false, true] {
        let world = World::new();
        let dir = TempDir::new(&format!("launch-panic-{end_fails}"));
        let launchers = launchers(&world);
        let reservation = prepared(&launchers, &world, &dir, "s1");
        let (launcher, _) =
            launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
        world.calls();
        world.panic_at("write_launch");
        if end_fails {
            world.fail_at("end_outer");
        }
        let error = refused(launcher.launch(&spec(&dir)));
        assert!(
            matches!(&error, clauseo_process::Error::Os { what, .. } if what.contains("failed")),
            "{error:?}"
        );
        let next = launchers.reserve(MATTER, "s2", &[]);
        if end_fails {
            assert_eq!(world.calls(), ["create_pipe", "write_launch", "end_outer"]);
            assert!(matches!(next, Err(Error::MatterStuck)), "{next:?}");
        } else {
            let want = [
                "create_pipe",
                "write_launch",
                "end_outer",
                "end_run 1",
                UNDO[0],
                UNDO[1],
            ];
            assert_eq!(world.calls()[..6], want);
            assert!(next.is_ok(), "{next:?}");
        }
    }
}

/// The hand-over once the outer has started: the claim is let go of without an undo, the
/// session stays the registry's, and `after_session` ends it and frees the matter.
#[test]
fn the_hand_over_lets_a_started_session_go() {
    let world = World::new();
    let dir = TempDir::new("launch-hand-over");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let (_, id) = reservation.claim_id().unwrap();
    let folder = policy_at(&dir, "matter").matter().to_path_buf();
    launchers
        .registry
        .launch(MATTER, "s1", id, &folder)
        .unwrap();
    let claim = reservation.take_claim();
    tier_b::hand_over(&launchers.registry, claim, true);
    assert_eq!(world.calls(), Vec::<String>::new(), "no undo");
    assert!(launchers.registry.is_launched(id));
    let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(busy, Error::MatterBusy), "{busy:?}");
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    let calls = world.calls();
    assert_eq!(calls[calls.len() - 2..], ["end_run 1", "release_slot"]);
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s2", &[]);
    assert!(free.is_ok());
}

/// A launcher dropped unused undoes the session off the dropping thread; the matter is busy
/// until that's done, then free, and `after_session` finds nothing left to end.
#[test]
fn an_unused_launcher_frees_the_matter() {
    let world = World::new();
    let dir = TempDir::new("launch-unused");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    world.calls();
    let undoing = world.hold("close_session");
    // Would hang here if the drop waited for the undo.
    drop(launched);
    undoing.reached();
    let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(busy, Error::MatterBusy), "{busy:?}");
    undoing.go();
    assert!(launchers.registry.wait_free(MATTER, WAIT));
    assert_eq!(world.calls(), ["end_run 1", UNDO[0], UNDO[1]]);
    let policy = policy_at(&dir, "matter");
    assert_eq!(
        launchers.after_session(MATTER, "s1", &policy),
        Vec::<SetAside>::new()
    );
    assert_eq!(world.calls(), Vec::<String>::new());
}

/// A panic in a step of the session's end still ends the run's group, gives the slot back and
/// frees the matter, then goes on up.
#[test]
fn after_session_frees_the_matter_on_a_panic() {
    let world = World::new();
    let dir = TempDir::new("launch-after-panic");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let _launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    world.calls();
    world.panic_at("repair");
    let policy = policy_at(&dir, "matter");
    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        launchers.after_session(MATTER, "s1", &policy)
    }));
    assert!(ended.is_err(), "the panic goes on up");
    assert_eq!(
        world.calls(),
        ["session_cursor", "repair", "end_run 1", "release_slot"]
    );
    assert!(world.claimed_slots().is_empty());
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s2", &[]);
    assert!(free.is_ok(), "free");
}

/// The session's end, in 0049's order; then the slot goes back and the matter is free.
#[test]
fn after_session_runs_in_order_and_frees_the_matter() {
    let world = World::new();
    let dir = TempDir::new("launch-after");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
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
    // The end was `after_session`'s: the launcher's claim, dropped now, undoes nothing.
    drop(launched);
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s2", &[]);
    assert!(free.is_ok());
    assert_eq!(world.calls(), RESERVE);
}

/// A failed repair or pass is handed to the clean-close record as failed, and the end goes on.
#[test]
fn after_session_goes_on_past_failures() {
    for step in ["repair", "set_aside_after"] {
        let world = World::new();
        let dir = TempDir::new(&format!("launch-after-{step}"));
        let launchers = launchers(&world);
        let reservation = prepared(&launchers, &world, &dir, "s1");
        let _launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
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
        // Held to the end: a dropped reservation's undo would log on its own thread.
        let free = launchers.reserve(MATTER, "s2", &[]);
        assert!(free.is_ok(), "{step}");
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
    let _launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
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
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s2", &[]);
    assert!(free.is_ok());
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
