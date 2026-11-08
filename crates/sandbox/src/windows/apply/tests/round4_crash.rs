//! The S15 `PR3b` round 4 fix for a crash during a take-back, from the second VM run's kill
//! during a rollback: entries are marked removing before any removal, and reconcile finishes
//! their removal rather than take them for written.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;

/// The steps of a take-back a crash can stop: a removal, a walk after it, the drop at the end.
const STEPS: [&str; 3] = ["remove", "walk", "drop"];

/// Makes `acl` panic at `step`, as a crash there would stop the process.
fn crash_at(acl: &mut FakeAcl, step: &str, w: &World) {
    match step {
        "remove" => acl.panic_remove = true,
        "walk" => acl.panic_walk = Some(w.canonical("sib1")),
        _ => acl.panic_present = Some(w.canonical("sib1")),
    }
}

/// The next start: no more crashes, the ledger read again.
fn restart(acl: &mut FakeAcl, w: &World) -> Ledger {
    acl.panic_remove = false;
    acl.panic_walk = None;
    acl.panic_present = None;
    super::ledger(w)
}

/// Round 4, the kill during a rollback: reconcile couldn't tell a launch that crashed from a
/// rollback that crashed, so a crash while a refused launch rolled back left that launch's
/// grants `applied`. Now, after a crash at each step of the rollback, reconcile finishes the
/// removal: none of the refused launch's grants is applied or left, and no entry stays.
#[test]
fn a_crash_at_each_step_of_a_rollback_leaves_no_grant_of_the_refused_launch() {
    for step in STEPS {
        let w = world(&format!("r4-crash-rollback-{step}"), &[]);
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        acl.fail_add = Some(w.canonical(NOTES));
        crash_at(&mut acl, step, &w);
        let crashed = catch_unwind(AssertUnwindSafe(|| {
            let mut plan = launch_plan(&w, &["sib1"]);
            drop(apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)));
        }));
        assert!(crashed.is_err(), "{step}: it crashed");
        let mut ledger = restart(&mut acl, &w);
        assert!(
            ledger.entries().all(|e| e.state == EntryState::Removing),
            "{step}: marked before any removal"
        );
        let done = reconcile(&mut ledger, &mut acl).unwrap();
        assert_eq!(done.applied, 0, "{step}: {done:?}");
        let left: Vec<_> = ledger.entries().collect();
        assert!(left.is_empty(), "{step}: {left:?}");
        assert!(acl.aces.is_empty(), "{step}: {:?}", acl.aces);
    }
}

/// Round 4, the same for forget: after a crash at each of its steps, reconcile takes none of the
/// matter's entries for written and removes every permission entry. The entries stay, marked
/// removing, while the hand-back is owed (N5); forget run again then finishes.
#[test]
fn a_crash_at_each_step_of_forget_leaves_no_grant_applied_and_forget_again_finishes() {
    for step in STEPS {
        let w = world(&format!("r4-crash-forget-{step}"), &[]);
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        let mut plan = launch_plan(&w, &["sib1"]);
        apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
        crash_at(&mut acl, step, &w);
        let crashed = catch_unwind(AssertUnwindSafe(|| {
            drop(forget_matter(
                &mut ledger,
                matter_one(),
                &mut acl,
                &mut around(&w),
            ));
        }));
        assert!(crashed.is_err(), "{step}: it crashed");
        let mut ledger = restart(&mut acl, &w);
        let done = reconcile(&mut ledger, &mut acl).unwrap();
        assert_eq!(done.applied, 0, "{step}: {done:?}");
        assert!(acl.aces.is_empty(), "{step}: {:?}", acl.aces);
        assert!(
            ledger.entries().all(|e| e.state == EntryState::Removing),
            "{step}: kept for the retried forget"
        );
        forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
        assert_eq!(ledger.entries().count(), 0, "{step}");
        assert!(acl.aces.is_empty(), "{step}");
    }
}

/// Round 4: a launch that lists an entry a take-back is still removing sets it back to pending
/// before it writes, so the take-back's end and reconcile leave its permission entry.
#[test]
fn a_launch_takes_back_an_entry_being_removed_and_reconcile_leaves_it() {
    let w = world("r4-crash-relaunch", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = launch_plan(&w, &["sib1"]);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    acl.panic_remove = true;
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        drop(forget_matter(
            &mut ledger,
            matter_one(),
            &mut acl,
            &mut around(&w),
        ));
    }));
    assert!(crashed.is_err());
    let mut ledger = restart(&mut acl, &w);
    let mut again = launch_plan(&w, &["sib1"]);
    apply(&mut ledger, &mut again, &mut acl, &mut around(&w)).unwrap();
    assert!(ledger.entries().all(|e| e.state == EntryState::Applied));
    let done = reconcile(&mut ledger, &mut acl).unwrap();
    assert_eq!(done.removed, 0, "{done:?}");
    for e in ledger.entries() {
        assert!(acl.aces.contains(&acl.key_of(&e.grant())), "{e:?}");
    }
}
