//! A read of another matter's whole folder lists it (R18): the fence's deny on the folder alone
//! is lifted for the session's pool user and put back at the end, on a forget, on a narrowing
//! and after a crash; the deny for what is below stays throughout.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::reads::access::{APPEND, POOL_TOKEN, READ, USERS, WRITE, granted, volume_default};
use super::*;

/// A fake with the volume's default entry at the top.
pub(super) fn volume(w: &World) -> FakeAcl {
    let mut acl = FakeAcl::default();
    let top = dunce::canonicalize(w.dir.path()).unwrap();
    acl.aces.insert(volume_default(&top));
    acl
}

/// Matter one's launch reading each of `reads`, matter two's fence planned.
pub(super) fn launch(w: &World, ledger: &mut Ledger, acl: &mut FakeAcl, reads: &[(&str, bool)]) {
    let policy = w.reading(reads);
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let (data, others) = (w.at("data"), w.neighbour_m2());
    let admitted = admit_with(&policy, &data, &others, ok, &FakeFolders::default()).unwrap();
    let mut plan = plan(&admitted, &sids(), matter_one()).unwrap();
    apply(ledger, &mut plan, acl, &mut around(w)).unwrap();
}

pub(super) fn reads(w: &World, acl: &FakeAcl, rel: &str) -> bool {
    granted(&acl.aces, &w.canonical(rel), &POOL_TOKEN, READ)
}

/// Matter two's deny of matter one's pool user on its folder alone.
pub(super) fn root_deny(w: &World) -> Key {
    let root = w.canonical("other/m2");
    (
        root,
        USER_SID.to_owned(),
        Rights::DenyReadWrite,
        Inherit::ThisOnly,
    )
}

/// What stands in for it while it is lifted: the same deny of writes alone.
pub(super) fn stand_in(w: &World) -> Key {
    let (root, sid, _, inherit) = root_deny(w);
    (root, sid, Rights::DenyWrite, inherit)
}

/// Whether matter one's pool user can make a file or a folder in matter two's folder.
pub(super) fn root_writable(w: &World, acl: &FakeAcl) -> bool {
    let root = w.canonical("other/m2");
    granted(&acl.aces, &root, &POOL_TOKEN, WRITE) || granted(&acl.aces, &root, &POOL_TOKEN, APPEND)
}

/// Whether the ledger marks that deny lifted by matter one.
pub(super) fn marked(w: &World, ledger: &Ledger) -> bool {
    let root = w.canonical("other/m2");
    ledger
        .entries()
        .any(|e| e.path == root && e.inherit == Inherit::ThisOnly && e.lifted == Some(matter_one()))
}

pub(super) fn pool() -> Vec<Sid> {
    vec![sid(USER_SID)]
}

/// The state a read of the whole folder runs in: listed, never written, the deny off, its
/// stand-in on, and marked.
pub(super) fn assert_lifted(w: &World, ledger: &Ledger, acl: &FakeAcl) {
    assert!(reads(w, acl, "other/m2"), "the folder lists");
    assert!(!root_writable(w, acl), "the folder takes no write");
    assert!(!acl.aces.contains(&root_deny(w)));
    assert!(acl.aces.contains(&stand_in(w)));
    assert!(marked(w, ledger));
}

/// The state after: the deny back, its stand-in gone, unmarked, the folder closed.
pub(super) fn assert_restored(w: &World, ledger: &Ledger, acl: &FakeAcl) {
    assert!(!reads(w, acl, "other/m2"), "the folder is closed");
    assert!(!root_writable(w, acl), "the folder takes no write");
    assert!(acl.aces.contains(&root_deny(w)));
    assert!(!acl.aces.contains(&stand_in(w)));
    assert!(!marked(w, ledger));
}

#[test]
fn a_read_of_the_whole_folder_lists_it_and_keeps_every_refused_name_closed() {
    let w = World::new("lift-launch");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    assert_lifted(&w, &ledger, &acl);
    for rel in [
        "other/m2/a.txt",
        "other/m2/docs/deep/c.txt",
        "other/m2/sub/e.txt",
    ] {
        assert!(reads(&w, &acl, rel), "{rel}");
    }
    for rel in [
        "other/m2/.git",
        "other/m2/.git/config",
        "other/m2/.Claude/settings.json",
        "other/m2/sub/.git/HEAD",
        "other/m2/wt/.git",
    ] {
        assert!(!reads(&w, &acl, rel), "{rel}");
    }
    // What is made directly in the folder during the run inherits the deny for what is below.
    w.dir.file("other/m2/new.txt", "n");
    assert!(!reads(&w, &acl, "other/m2/new.txt"));
    // The other pool user still can't list it: its own deny is untouched.
    let other = [OTHER_POOL_SIDS[1], GROUP_SID, USERS];
    assert!(!granted(&acl.aces, &w.canonical("other/m2"), &other, READ));
    // Matter two's deny on its folder is still listed as applied: nothing replans it.
    let root = w.canonical("other/m2");
    let listed = ledger.entries().any(|e| {
        e.matter == matter_two()
            && e.path == root
            && e.inherit == Inherit::ThisOnly
            && e.state == EntryState::Applied
    });
    assert!(listed);
}

#[test]
fn the_runs_end_puts_the_deny_back_and_a_second_end_does_nothing() {
    let w = World::new("lift-end");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    let ended = end_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_eq!(ended.restored, vec![w.canonical("other/m2")]);
    assert_restored(&w, &ledger, &acl);
    assert!(!reads(&w, &acl, "other/m2/a.txt"));
    let again = end_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_eq!(again, Narrowed::default());
}

#[test]
fn forgetting_the_whole_folder_puts_the_deny_back_and_keeps_a_read_inside_it() {
    let w = World::new("lift-narrow");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    // Narrowed with the whole folder still read: nothing changes.
    let same = w.reading(&[("other/m2", false)]);
    let kept = narrow_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &same,
        &mut acl,
        &mut around(&w),
    );
    assert_eq!(kept.unwrap(), Narrowed::default());
    assert_lifted(&w, &ledger, &acl);
    // Narrowed to a folder inside: the deny goes back, that folder stays readable.
    let inside = w.reading(&[("other/m2/docs", false)]);
    let narrowed = narrow_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &inside,
        &mut acl,
        &mut around(&w),
    );
    assert_eq!(narrowed.unwrap().restored, vec![w.canonical("other/m2")]);
    assert_restored(&w, &ledger, &acl);
    assert!(reads(&w, &acl, "other/m2/docs/b.txt"));
}

#[test]
fn a_crashed_runs_lift_goes_back_at_the_next_launch_at_start_or_on_a_forget() {
    for way in ["launch", "start", "forget"] {
        let w = World::new(&format!("lift-crash-{way}"));
        w.other_tree();
        let (mut ledger, mut acl) = (ledger(&w), volume(&w));
        launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
        // The run crashes: no end. What it leaves is the folder's names, no child.
        assert!(!reads(&w, &acl, "other/m2/.git/config"));
        let mut ledger = super::ledger(&w);
        match way {
            "launch" => launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]),
            "start" => drop(reconcile(&mut ledger, &mut acl).unwrap()),
            _ => drop(forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap()),
        }
        assert_restored(&w, &ledger, &acl);
    }
}

#[test]
fn a_crash_before_the_lifts_removal_leaves_the_deny_and_start_clears_the_mark() {
    let w = World::new("lift-crash-mark");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    // Every launch removal here is the lift's: no take-back runs on a first launch.
    acl.panic_remove = true;
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    }));
    assert!(crashed.is_err());
    acl.panic_remove = false;
    let mut ledger = super::ledger(&w);
    assert!(marked(&w, &ledger), "marked before the deny was touched");
    // The stand-in was written before the deny's removal: both are there, nothing opens.
    assert!(acl.aces.contains(&root_deny(&w)));
    assert!(acl.aces.contains(&stand_in(&w)));
    assert!(!reads(&w, &acl, "other/m2") && !root_writable(&w, &acl));
    reconcile(&mut ledger, &mut acl).unwrap();
    assert_restored(&w, &ledger, &acl);
}

#[test]
fn a_fence_written_again_mid_session_closes_the_folder_and_the_end_still_restores() {
    let w = World::new("lift-rewritten");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2", false)]);
    // Matter two's own launch writes its fence again: the session fails closed.
    acl.aces.insert(root_deny(&w));
    assert!(!reads(&w, &acl, "other/m2"));
    let ended = end_reads(
        &mut ledger,
        matter_one(),
        &pool(),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_eq!(ended.restored, vec![w.canonical("other/m2")]);
    assert_restored(&w, &ledger, &acl);
}
