//! What holds a matter or an account past its session: a step that never returns (stuck), a
//! failed `EndRun` (the account held back), and an undo that never frees a newer claim's slot.

use super::fake::{MATTER, OTHER, World};
use super::prepare::{WALK, held_at, undone_at};
use super::session::launch;
use super::*;

/// A walk whose step never returns keeps the matter busy, then, once its progress hasn't moved
/// for the stall timer plus the grace, stuck: `MatterStuck`, not retryable, until Clauseo
/// restarts. When the step returns at last, the walk undoes on its own thread, and the matter
/// stays stuck.
#[test]
fn a_walk_hung_in_a_step_makes_the_matter_stuck() {
    let world = World::new();
    let dir = TempDir::new("launch-stuck-walk");
    let (launchers, preparing, held, reservation) = held_at(&world, &dir, "apply");
    world.advance((PREPARE_STALL + STUCK_GRACE).saturating_sub(Duration::from_secs(1)));
    let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(busy, Error::MatterBusy), "{busy:?}");
    world.advance(Duration::from_secs(1));
    let stuck = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(stuck, Error::MatterStuck), "{stuck:?}");
    assert!(!stuck.code().is_retryable());
    // Dropping the reservation undoes nothing from here: the step could still grant.
    drop(reservation);
    assert_eq!(world.calls(), WALK, "no undo yet");
    held.go();
    let result = preparing.wait(WAIT).unwrap();
    assert!(matches!(result, Err(Error::MatterStuck)), "{result:?}");
    assert_eq!(world.calls(), undone_at("apply")[WALK.len()..]);
    let still = launchers.reserve(MATTER, "s3", &[]).unwrap_err();
    assert!(matches!(still, Error::MatterStuck), "{still:?}");
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(OTHER, "s4", &[]);
    assert!(free.is_ok(), "other matters go on");
}

/// A session's end whose step never returns makes the matter stuck past the bound plus the
/// grace; it stays stuck once the step returns and the end finishes.
#[test]
fn an_end_hung_in_a_step_makes_the_matter_stuck() {
    let world = World::new();
    let dir = TempDir::new("launch-stuck-end");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let _launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    let held = world.hold("set_aside_after");
    let policy = policy_at(&dir, "matter");
    std::thread::scope(|scope| {
        let ending = scope.spawn(|| launchers.after_session(MATTER, "s1", &policy));
        held.reached();
        world.advance(PREPARE_BOUND + STUCK_GRACE);
        let stuck = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
        assert!(matches!(stuck, Error::MatterStuck), "{stuck:?}");
        held.go();
        ending.join().unwrap();
    });
    let calls = world.calls();
    assert_eq!(calls[calls.len() - 2..], ["end_run 1", "release_slot"]);
    let still = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(still, Error::MatterStuck), "{still:?}");
}

/// A failed `EndRun` keeps the account and its slot held back: the next reserve tries it again
/// and, while it fails, is refused with the account counted as needing a reset; once it
/// succeeds, the slot goes back and the reserve goes on.
#[test]
fn a_failed_end_run_holds_the_account_back() {
    let world = World::new();
    let dir = TempDir::new("launch-end-run-fails");
    let launchers = launchers(&world);
    let reservation = prepared(&launchers, &world, &dir, "s1");
    let _launched = launch(&launchers, &dir, reservation, (MATTER, "s1", "matter")).unwrap();
    world.calls();
    world.end_run_fails(2);
    launchers.after_session(MATTER, "s1", &policy_at(&dir, "matter"));
    let calls = world.calls();
    assert_eq!(
        calls.last().map(String::as_str),
        Some("end_run 1"),
        "{calls:?}"
    );
    assert_eq!(world.claimed_slots().len(), 1, "the slot stays claimed");
    let error = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(
        matches!(&error, Error::PoolFull { cause } if cause.needs_reset == 1),
        "{error:?}"
    );
    let retried = [&RESERVE[..3], &["end_run 1", "close_session"]].concat();
    assert_eq!(world.calls(), retried);
    let reservation = launchers.reserve(MATTER, "s3", &[]).unwrap();
    let ended = [&RESERVE[..3], &["end_run 1", "release_slot"], &RESERVE[3..]].concat();
    assert_eq!(world.calls(), ended);
    assert_eq!(reservation.slot(), Some(0));
}

/// A failed walk's undo holds the matter (undoing) until it ends, even once its reservation is
/// dropped: a reserve of the matter is `MatterBusy` meanwhile. The slot it gave back can go to
/// another claim, whose record no late release of the old claim ever frees.
#[test]
fn an_old_undo_never_frees_a_newer_claims_slot() {
    let world = World::new();
    let dir = TempDir::new("launch-undo-slot");
    let launchers = launchers(&world);
    world.fail_at("apply");
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    let (_, old) = reservation.claim_id().unwrap();
    let undoing = world.hold("close_session");
    let preparing = launchers.prepare(&reservation, &policy_at(&dir, "matter"));
    undoing.reached();
    drop(reservation);
    let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(busy, Error::MatterBusy), "{busy:?}");
    let other = launchers.reserve(OTHER, "s3", &[]).unwrap();
    let (_, newer) = other.claim_id().unwrap();
    assert_eq!(other.slot(), Some(0), "the slot the undo gave back");
    undoing.go();
    assert!(matches!(preparing.wait(WAIT), Some(Err(Error::Policy(_)))));
    assert!(launchers.registry.wait_free(MATTER, WAIT));
    assert_eq!(world.slot_claim(0), Some(newer));
    let stale = Slot {
        index: 0,
        ports: other.ports().to_vec(),
        claim: old,
    };
    launchers.registry.steps.release_slot(&stale);
    assert_eq!(
        world.slot_claim(0),
        Some(newer),
        "a late release frees nothing"
    );
    // Held to the end: a dropped reservation's undo would log on its own thread.
    let free = launchers.reserve(MATTER, "s4", &[]);
    assert!(free.is_ok());
}
