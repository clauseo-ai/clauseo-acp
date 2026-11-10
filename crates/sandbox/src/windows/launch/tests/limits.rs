//! The work steps' limits: progress counted (a tick with nothing done is none), the 20 min cap,
//! the session end under the walk's stall rule, and the lawyer's Cancel at every kind of step.

use std::sync::Mutex;

use super::fake::{MATTER, World};
use super::prepare::undone_at;
use super::session::launch;
use super::*;
use crate::windows::launch::prepare::Shared;

/// The walk's calls to the leftover's stop and sweep, with one leftover.
const UNDONE_LEFTOVER: [&str; 6] = [
    "plan",
    "check_launch",
    "check_network",
    "pool_processes",
    "stop",
    "sweep",
];

/// Only a rise in the counted progress moves the stall timer: a step that loops calling
/// `advance(0)` stalls, one that counts items doesn't; and the cap stops either.
#[test]
fn only_counted_progress_holds_off_the_stall() {
    let clock = Mutex::new(Duration::ZERO);
    let now = || *clock.lock().unwrap();
    let set = |at: Duration| *clock.lock().unwrap() = at;
    let shared = Shared::new(Duration::ZERO);
    let walk = Walk {
        shared: &shared,
        now: &now,
    };
    for _ in 0..7 {
        set(now() + Duration::from_secs(10));
        walk.advance(0);
    }
    assert!(
        walk.cancelled(),
        "a tick with nothing counted is no progress"
    );
    let shared = Shared::new(Duration::ZERO);
    let walk = Walk {
        shared: &shared,
        now: &now,
    };
    set(Duration::ZERO);
    while now() < WORK_CEILING.saturating_sub(Duration::from_secs(10)) {
        set(now() + Duration::from_secs(10));
        walk.advance(1);
        assert!(!walk.cancelled(), "progressing at {:?}", now());
    }
    set(WORK_CEILING);
    walk.advance(1);
    assert!(walk.cancelled(), "the cap holds even while progressing");
}

/// A walk that keeps progressing up to the 20 min cap fails there with `PrepareStalled`, and
/// what it did is undone.
#[test]
fn a_walk_past_the_cap_fails_and_undoes() {
    let world = World::new();
    let dir = TempDir::new("launch-ceiling");
    world.progressing("apply", WORK_CEILING);
    let launchers = launchers(&world);
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    world.calls();
    let preparing = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    let result = preparing.wait(WAIT).unwrap();
    assert!(matches!(result, Err(Error::PrepareStalled)), "{result:?}");
    assert_eq!(world.calls(), undone_at("apply"));
    assert!(world.claimed_slots().is_empty());
}

/// The lawyer's Cancel (the reservation dropped) at the stop and the sweep, the waits' steps
/// that `a_drop_at_each_step_keeps_the_matter_busy_until_the_walk_undid` doesn't reach: the
/// walk stops there, undoes, and frees the matter, with nothing granted after.
#[test]
fn a_drop_during_the_leftover_stop_undoes() {
    for step in ["stop", "sweep"] {
        let world = World::new();
        let dir = TempDir::new(&format!("launch-drop-{step}"));
        world.leftovers(1, false);
        let launchers = launchers(&world);
        let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
        world.calls();
        let held = world.hold(step);
        let preparing = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
        held.reached();
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
        let at = UNDONE_LEFTOVER
            .iter()
            .position(|each| *each == step)
            .unwrap();
        let want = [&UNDONE_LEFTOVER[..=at], &UNDO[..]].concat();
        assert_eq!(world.calls(), want, "{step}");
        assert!(world.claimed_slots().is_empty(), "{step}");
        // Held to the end: a dropped reservation's undo would log on its own thread.
        let free = launchers.reserve(MATTER, "s2", &[]);
        assert!(free.is_ok(), "{step}: free");
    }
}

/// A session's end whose repair keeps progressing well past the old 240 s wall limit is busy
/// all along, never stuck, and finishes normally, freeing the matter.
#[test]
fn a_slow_end_that_progresses_finishes() {
    let world = World::new();
    let dir = TempDir::new("launch-slow-end");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let _launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    world.calls();
    world.progressing("repair", Duration::from_secs(600));
    let held = world.hold("set_aside_after");
    let policy = policy_at(&dir, "matter");
    std::thread::scope(|scope| {
        let ending = scope.spawn(|| launchers.after_session(MATTER, "s1", &policy));
        held.reached();
        assert!(world.now() >= PREPARE_BOUND + STUCK_GRACE + STUCK_GRACE);
        let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
        assert!(matches!(busy, Error::MatterBusy), "{busy:?}");
        held.go();
        ending.join().unwrap();
    });
    let calls = world.calls();
    let close = "write_clean_close cursor=true repaired=true passed=true";
    assert!(calls.contains(&close.to_owned()), "{calls:?}");
    assert_eq!(calls[calls.len() - 2..], ["end_run 1", "release_slot"]);
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s2", &[]);
    assert!(free.is_ok(), "free");
}

/// A session's end past the 20 min cap stops its work steps (the repair, then the pass, both
/// failed in the clean-close record) and still ends the run and frees the matter.
#[test]
fn an_end_past_the_cap_stops_its_steps_and_frees() {
    let world = World::new();
    let dir = TempDir::new("launch-end-ceiling");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let _launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    world.calls();
    world.progressing("repair", WORK_CEILING);
    let moved = launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    assert_eq!(moved, Vec::<SetAside>::new());
    let calls = world.calls();
    let close = "write_clean_close cursor=true repaired=false passed=false";
    assert!(calls.contains(&close.to_owned()), "{calls:?}");
    assert_eq!(calls[calls.len() - 2..], ["end_run 1", "release_slot"]);
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s2", &[]);
    assert!(free.is_ok(), "free");
}
