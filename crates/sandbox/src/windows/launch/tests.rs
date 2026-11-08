//! `Launchers`' order over the fake steps (`fake.rs`): the reserve here, the preparation in
//! `prepare.rs`, the launch and the session's end in `session.rs`.

use std::sync::Arc;
use std::time::Duration;

use self::fake::{FakeSteps, MATTER, OTHER, World};
use super::*;
use crate::test_support::TempDir;
use crate::windows::pool::assign::{HeldBack, Why};
use crate::windows::sids::Sid;

mod fake;
mod held;
mod prepare;
mod session;

/// How long a test waits for a preparation.
const WAIT: Duration = Duration::from_secs(10);

/// Launchers over `world`, the start-up's calls taken from its log.
fn launchers(world: &Arc<World>) -> Launchers<FakeSteps> {
    let launchers = Launchers::with_steps(FakeSteps::new(world)).unwrap();
    assert_eq!(world.calls(), ["clear_stale"]);
    launchers
}

/// A policy whose matter is `rel` under `dir`.
fn policy_at(dir: &TempDir, rel: &str) -> Policy {
    Policy::builder(dir.dir(rel), dir.dir("state"))
        .build()
        .unwrap()
}

/// A reservation of `MATTER` for `session`, prepared; the calls taken from the log.
fn prepared(
    launchers: &Launchers<FakeSteps>,
    world: &World,
    dir: &TempDir,
    session: &str,
) -> Reservation {
    let reservation = launchers.reserve(MATTER, session, &[]).unwrap();
    let preparing = launchers.prepare(&reservation, &policy_at(dir, "matter"));
    preparing.wait(WAIT).unwrap().unwrap();
    world.calls();
    reservation
}

const RESERVE: [&str; 5] = [
    "setup_ready",
    "open_session",
    "assign",
    "end_sweeper",
    "claim_slot",
];
const UNDO: [&str; 2] = ["release_slot", "close_session"];

#[test]
fn the_reserve_is_bookkeeping_in_order() {
    let world = World::new();
    let launchers = launchers(&world);
    let reservation = launchers.reserve(MATTER, "s1", &[]).unwrap();
    assert_eq!(world.calls(), RESERVE);
    assert_eq!(
        (reservation.matter(), reservation.session()),
        (MATTER, "s1")
    );
    assert_eq!(reservation.slot(), Some(0));
    assert_eq!(reservation.ports(), [46100, 46101]);
    assert_eq!(reservation.pool_user(), Some(&fake::pool_user()));
    assert_eq!(world.claimed_slots().len(), 1);
    assert!(world.open_sessions(MATTER).contains("s1"));
}

/// A failed step of the reserve undoes what the steps before it did, and frees the matter.
#[test]
fn a_failed_reserve_undoes_what_it_did() {
    let undo = |done: &[&'static str]| {
        let mut calls = done.to_vec();
        if done.contains(&"claim_slot") {
            calls.push("close_session");
        }
        calls
    };
    for (step, done) in [
        ("setup_ready", &RESERVE[..1]),
        ("open_session", &RESERVE[..2]),
        ("claim_slot", &RESERVE[..]),
    ] {
        let world = World::new();
        let launchers = launchers(&world);
        world.fail_at(step);
        let error = launchers.reserve(MATTER, "s1", &[]).unwrap_err();
        assert!(matches!(error, Error::Policy(_)), "{step}: {error:?}");
        assert_eq!(world.calls(), undo(done), "{step}");
        assert!(world.open_sessions(MATTER).is_empty(), "{step}");
        assert!(world.claimed_slots().is_empty(), "{step}");
        world.fail_at("none");
        assert!(
            launchers.reserve(MATTER, "s2", &[]).is_ok(),
            "{step}: freed"
        );
    }
}

/// A refusal of `assign`'s, and a check of the error it gives.
type Case = (Assign, fn(&Error) -> bool);

fn sid(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-1111111111-2222222222-3333333333-{rid}")).unwrap()
}

/// `assign`'s refusals: a full pool is `PoolFull` with its counts, another open session
/// `MatterBusy`, records that can't be read `Probe` and a block `Blocked`. Each undoes the mark.
#[test]
fn assigns_refusals_map_to_their_errors() {
    let full = Assign::PoolFull {
        size: 2,
        held_back: vec![
            HeldBack {
                pool: sid(1001),
                why: Why::Open,
            },
            HeldBack {
                pool: sid(1002),
                why: Why::SetAside,
            },
        ],
    };
    let cases: [Case; 4] = [
        (full, |error| {
            matches!(error, Error::PoolFull { cause } if cause.size == 2
                && cause.open == 1 && cause.set_aside == 1)
        }),
        (Assign::Busy(MATTER.parse().unwrap()), |error| {
            matches!(error, Error::MatterBusy)
        }),
        (
            Assign::Records(SetupError::Failed("matters.json".to_owned())),
            |error| matches!(error, Error::Probe(_)),
        ),
        (
            Assign::Records(SetupError::Blocked(crate::Blocked::LogonRight)),
            |error| matches!(error, Error::Blocked(crate::Blocked::LogonRight)),
        ),
    ];
    for (refusal, is) in cases {
        let world = World::new();
        let launchers = launchers(&world);
        let shown = format!("{refusal:?}");
        world.assign_refuses(refusal);
        let error = launchers.reserve(MATTER, "s1", &[]).unwrap_err();
        assert!(is(&error), "{shown}: {error:?}");
        assert_eq!(world.calls(), [&RESERVE[..3], &["close_session"]].concat());
        assert!(world.open_sessions(MATTER).is_empty());
    }
}

/// `assign` itself refuses a second open session of the matter: `MatterBusy`.
#[test]
fn assign_finds_another_open_session_busy() {
    let world = World::new();
    let first = launchers(&world);
    let held = first.reserve(MATTER, "s1", &[]).unwrap();
    // Other launchers over the same records, with a folder of their own (on the same folder
    // they'd be refused: `a_second_set_of_launchers_is_refused`).
    let elsewhere = TempDir::new("launch-assign-busy");
    let second = Launchers::with_steps(FakeSteps::elsewhere(&world, elsewhere.path())).unwrap();
    world.calls();
    let error = second.reserve(MATTER, "s2", &["s1"]).unwrap_err();
    assert!(matches!(error, Error::MatterBusy), "{error:?}");
    assert_eq!(world.calls(), [&RESERVE[..3], &["close_session"]].concat());
    drop(held);
}

/// One session holds a matter: a second reservation is `MatterBusy` with no step called;
/// another matter is reserved meanwhile; a dropped reservation undoes itself and frees it.
#[test]
fn a_reservation_holds_the_matter_until_dropped() {
    let world = World::new();
    let launchers = launchers(&world);
    let first = launchers.reserve(MATTER, "s1", &[]).unwrap();
    world.calls();
    let busy = launchers.reserve(MATTER, "s2", &[]).unwrap_err();
    assert!(matches!(busy, Error::MatterBusy), "{busy:?}");
    assert!(world.calls().is_empty(), "no step for a busy matter");
    let other = launchers.reserve(OTHER, "s3", &[]).unwrap();
    assert_eq!(other.slot(), Some(1));
    world.calls();
    drop(first);
    // The undo runs off the dropping thread; the matter is busy until it is done.
    assert!(launchers.registry.wait_free(MATTER, WAIT));
    assert_eq!(world.calls(), UNDO);
    assert_eq!(world.claimed_slots().into_iter().collect::<Vec<_>>(), [1]);
    assert!(world.open_sessions(MATTER).is_empty());
    let again = launchers.reserve(MATTER, "s2", &[]).unwrap();
    assert_eq!(again.slot(), Some(0));
}

/// A crash (reservations never dropped, the launchers gone) strands no slot: new launchers
/// clear the stale claims first, and the crashed session's mark goes at the next reserve.
#[test]
fn a_crash_strands_no_slot() {
    let world = World::new();
    let crashed = launchers(&world);
    // Never dropped, as in a crash.
    let _held = [
        std::mem::ManuallyDrop::new(crashed.reserve(MATTER, "s1", &[]).unwrap()),
        std::mem::ManuallyDrop::new(crashed.reserve(OTHER, "s2", &[]).unwrap()),
    ];
    // The process ends, and its lock with it.
    crashed.registry.crash();
    drop(crashed);
    assert_eq!(world.claimed_slots().len(), usize::from(fake::SLOTS));
    world.calls();
    let restarted = launchers(&world);
    assert!(world.claimed_slots().is_empty(), "cleared at start-up");
    let reservation = restarted.reserve(MATTER, "s3", &[]).unwrap();
    assert_eq!(reservation.slot(), Some(0));
    let open: Vec<String> = world.open_sessions(MATTER).into_iter().collect();
    assert_eq!(open, ["s3"]);
}

/// Start-up's failure is the launchers' failure.
#[test]
fn a_failed_start_up_makes_no_launchers() {
    let world = World::new();
    world.fail_at("clear_stale");
    assert!(Launchers::with_steps(FakeSteps::new(&world)).is_err());
}

/// One set of launchers per sandbox folder: a second (another Clauseo, or another set in this
/// one) is refused at once with `LaunchersHeld`, and clears nothing, so the first's slot claims
/// and session marks stay. Once the first is gone, new launchers start.
#[test]
fn a_second_set_of_launchers_is_refused() {
    let world = World::new();
    let first = launchers(&world);
    let reservation = first.reserve(MATTER, "s1", &[]).unwrap();
    world.calls();
    let Err(error) = Launchers::with_steps(FakeSteps::new(&world)) else {
        panic!("a second set of launchers started");
    };
    assert!(matches!(error, Error::LaunchersHeld), "{error:?}");
    assert_eq!(world.calls(), Vec::<String>::new(), "no clear_stale");
    assert_eq!(world.claimed_slots().len(), 1, "the first's claim stays");
    assert!(world.open_sessions(MATTER).contains("s1"));
    assert!(world.dir().join(LAUNCHERS_LOCK).is_file());
    drop(reservation);
    drop(first);
    let world = World::new();
    drop(launchers(&world));
    let next = Launchers::with_steps(FakeSteps::new(&world));
    assert!(next.is_ok(), "the lock went with the first");
}

/// The lock file is never followed through a link: a link at its name refuses the start-up.
#[cfg(unix)]
#[test]
fn a_link_at_the_lock_is_refused() {
    let world = World::new();
    let folder = TempDir::new("launch-lock-link");
    let target = folder.file("target", "kept\n");
    folder.symlink(LAUNCHERS_LOCK, &target);
    let started = Launchers::with_steps(FakeSteps::elsewhere(&world, folder.path()));
    assert!(matches!(started, Err(Error::Probe(_))), "{started:?}");
    assert_eq!(world.calls(), Vec::<String>::new(), "no clear_stale");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "kept\n");
}
