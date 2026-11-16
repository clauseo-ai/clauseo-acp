//! S15 `PR4i`: a refusal the elevated clean-up can lift records its item in the ledger for
//! `--reclaim`; a launch of the matter that passes clears the record.

use super::*;
use crate::windows::ledger::{ReclaimOwed, Stuck};

fn owed(ledger: &Ledger) -> Vec<(MatterId, PathBuf, Stuck)> {
    let all = ledger.reclaims_owed().into_iter();
    all.map(|o: ReclaimOwed| (o.matter, o.path, o.why))
        .collect()
}

#[test]
fn a_pool_owned_child_is_recorded_and_a_launch_that_passes_clears_it() {
    let w = world("stuck-child", &[]);
    let child = dunce::canonicalize(w.dir.file("matter/a/moved.txt", "x")).unwrap();
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.owners
        .insert(child.clone(), OTHER_POOL_SIDS[0].to_owned());
    acl.keeps_owner.insert(child.clone());

    let refused = apply(
        &mut ledger,
        &mut launch_plan(&w, &[]),
        &mut acl,
        &mut around(&w),
    );

    assert!(refused.is_err());
    let want = vec![(matter_one(), child.clone(), Stuck::PoolOwnedChild)];
    assert_eq!(owed(&ledger), want);
    assert_eq!(owed(&super::ledger(&w)), want, "on disk");

    // `--reclaim` made the lawyer its owner; the next launch hands it back and passes.
    acl.keeps_owner.clear();
    acl.owners.insert(child, super::fake::LAWYER.to_owned());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &[]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();

    assert_eq!(owed(&ledger), []);
    assert_eq!(owed(&super::ledger(&w)), []);
}

#[test]
fn a_shut_child_that_couldnt_be_set_aside_is_recorded() {
    let w = world("stuck-shut", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let child = w.canonical("sib1").join("child");
    acl.shut = vec![child.clone()];

    let plan = &mut launch_plan(&w, &["sib1"]);
    assert!(apply(&mut ledger, plan, &mut acl, &mut around(&w)).is_err());

    assert_eq!(owed(&ledger), [(matter_one(), child, Stuck::ChildShut)]);
}

#[test]
fn another_refusal_records_nothing() {
    let w = world("stuck-other", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.fail_add = Some(w.canonical("matter"));

    let plan = &mut launch_plan(&w, &[]);
    assert!(apply(&mut ledger, plan, &mut acl, &mut around(&w)).is_err());

    assert_eq!(owed(&ledger), []);
}
