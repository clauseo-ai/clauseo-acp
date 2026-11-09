//! The guard on what the pool writes (`Policy::native_code_deny`) through the ledger: applied
//! with the plan's other entries, its trees walked, and gone with them when the matter is.

use super::*;
use crate::windows::apply::walk::{guard_roots, own_roots, strict_at};
use crate::windows::shared_writes::OWNER_RIGHTS_SID;

/// Matter one's plan with the switch on.
fn guarded_plan(w: &World) -> AdmittedPlan {
    let policy = w.full_builder().native_code_deny(true).build().unwrap();
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let folders = FakeFolders::default();
    let admitted =
        admit_with(&policy, &w.at("data"), &Neighbours::default(), ok, &folders).unwrap();
    plan(&admitted, &sids(), matter_one()).unwrap()
}

#[test]
fn the_guard_is_applied_walked_down_and_listed_and_forgetting_takes_it_all_off() {
    let w = world("apply-guard", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = guarded_plan(&w);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();

    let matter = w.canonical("matter");
    assert!(has(&acl, &matter, USER_SID, Rights::DenyExecute));
    assert!(has(&acl, &matter, USER_SID, Rights::DenyPermissionChange));
    assert!(has(&acl, &matter, OWNER_RIGHTS_SID, Rights::OwnerModify));
    // Every entry listed and applied, `OWNER RIGHTS`' too (the ledger trusts it by its rights).
    assert_eq!(ledger.entries().count(), plan.grants().len());
    assert!(ledger.entries().all(|e| e.state == EntryState::Applied));
    let reread = super::ledger(&w);
    let owner = |e: &&crate::windows::ledger::Entry| e.rights == Rights::OwnerModify;
    assert_eq!(
        reread.entries().filter(owner).count(),
        4,
        "matter, state, notes, store"
    );
    // What is already there gets them: each folder is walked down, strictly, and the Claude
    // store too, which only its deny's walk reaches.
    for root in ["matter", STATE, NOTES, crate::windows::test_world::STORE] {
        let root = w.canonical(root);
        let walked = acl
            .walks
            .iter()
            .any(|(r, reach, s)| *r == root && *s && *reach == Reach::Tree);
        assert!(walked, "{root:?}: {:?}", acl.walks);
    }

    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert!(acl.aces.is_empty(), "{:?}", acl.aces);
    assert_eq!(ledger.entries().count(), 0);
}

/// #808's M1: the guard's subjects reach the strict walks of the guarded trees only, and none
/// with the switch off.
#[test]
fn the_guards_subjects_go_to_the_walks_of_its_trees_alone() {
    let w = world("apply-guard-strict", &["sib1"]);
    let plan = guarded_plan(&w);
    let strict = strict_for(&plan);
    let want: Vec<Sid> = [OWNER_RIGHTS_SID, USER_SID].map(sid).into_iter().collect();
    let mut got = strict.guarded.clone();
    got.sort();
    let mut want = want;
    want.sort();
    assert_eq!(got, want);
    let (own, guards) = (own_roots(&plan), guard_roots(&plan));
    let at = |root: &Path| strict_at(&strict, &own, &guards, root, |m| plan.assigned_to(m));
    let inside = w.canonical("matter").join("sub");
    assert_eq!(at(&inside).guarded, strict.guarded);
    assert_eq!(
        at(&w.canonical(crate::windows::test_world::STORE)).guarded,
        strict.guarded
    );
    assert_eq!(at(&w.canonical("sib1")).guarded, Vec::<Sid>::new());
    let off = strict_for(&launch_plan(&w, &["sib1"]));
    assert_eq!(off.guarded, Vec::<Sid>::new());
}

/// #808's M2: an `OWNER RIGHTS` entry IT wrote before ours was listed is recorded as found and
/// never removed; once IT's is gone and the launch writes it, it is ours again.
#[test]
fn an_owner_rights_entry_already_there_is_its_and_stays() {
    let w = world("apply-guard-found", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = guarded_plan(&w);
    let matter = w.canonical("matter");
    let theirs = plan
        .grants()
        .iter()
        .find(|g| g.path() == matter && g.rights() == Rights::OwnerModify)
        .cloned()
        .unwrap();
    acl.aces.insert(key(&theirs));
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    let found = |l: &Ledger| l.entries().filter(|e| e.found).count();
    assert_eq!(found(&ledger), 1);
    assert_eq!(found(&super::ledger(&w)), 1, "kept on disk");
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    let left: Vec<Key> = acl.aces.iter().cloned().collect();
    assert_eq!(left, vec![key(&theirs)], "IT's entry stays, ours go");
    assert_eq!(ledger.entries().count(), 0);

    // IT took its own off; the next launch writes ours, which forget then removes.
    acl.aces.clear();
    acl.aces.insert(key(&theirs));
    let mut plan = guarded_plan(&w);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    acl.aces.remove(&key(&theirs));
    let mut again = guarded_plan(&w);
    apply(&mut ledger, &mut again, &mut acl, &mut around(&w)).unwrap();
    assert_eq!(found(&ledger), 0);
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert!(acl.aces.is_empty(), "{:?}", acl.aces);
}

/// #808's M2: ownership is the permission entry's, not one matter's. Matters one and two both
/// list IT's entry as found; IT takes it off and matter one writes ours: both entries are ours
/// now, and the last matter forgotten removes it.
#[test]
fn an_owner_rights_entry_rewritten_is_ours_for_every_matter_naming_it() {
    let w = world("apply-guard-found-shared", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let matter = w.canonical("matter");
    let mut one = guarded_plan(&w);
    let ow = one
        .grants()
        .iter()
        .find(|g| g.path() == matter && g.rights() == Rights::OwnerModify)
        .cloned()
        .unwrap();
    acl.aces.insert(key(&ow));
    apply(&mut ledger, &mut one, &mut acl, &mut around(&w)).unwrap();
    let mut two = AdmittedPlan::from_grants(matter_two(), vec![ow.clone()]);
    apply(&mut ledger, &mut two, &mut acl, &mut around(&w)).unwrap();
    let found = |l: &Ledger| l.entries().filter(|e| e.found).count();
    assert_eq!(found(&ledger), 2, "both list IT's");

    acl.aces.remove(&key(&ow));
    let mut again = guarded_plan(&w);
    apply(&mut ledger, &mut again, &mut acl, &mut around(&w)).unwrap();
    assert!(acl.aces.contains(&key(&ow)));
    assert_eq!(found(&ledger), 0, "ours, for both matters");

    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert!(acl.aces.contains(&key(&ow)), "matter two still names it");
    forget_matter(&mut ledger, matter_two(), &mut acl, &mut around(&w)).unwrap();
    assert!(
        !acl.aces.contains(&key(&ow)),
        "the last one forgotten removes it"
    );
    assert_eq!(ledger.entries().count(), 0);
}

/// #808's N1: a new matter's first write of an `OWNER RIGHTS` entry IT took off marks every
/// entry naming it ours, so the last matter forgotten removes it.
#[test]
fn a_new_matters_write_marks_every_found_entry_ours() {
    let w = world("apply-guard-found-new", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let matter = w.canonical("matter");
    let mut one = guarded_plan(&w);
    let ow = one
        .grants()
        .iter()
        .find(|g| g.path() == matter && g.rights() == Rights::OwnerModify)
        .cloned()
        .unwrap();
    acl.aces.insert(key(&ow));
    apply(&mut ledger, &mut one, &mut acl, &mut around(&w)).unwrap();
    let found = |l: &Ledger| l.entries().filter(|e| e.found).count();
    assert_eq!(found(&ledger), 1, "matter one lists IT's");

    acl.aces.remove(&key(&ow));
    let mut two = AdmittedPlan::from_grants(matter_two(), vec![ow.clone()]);
    apply(&mut ledger, &mut two, &mut acl, &mut around(&w)).unwrap();
    assert!(acl.aces.contains(&key(&ow)), "matter two wrote ours");
    assert_eq!(found(&ledger), 0, "and matter one's entry names ours now");

    forget_matter(&mut ledger, matter_two(), &mut acl, &mut around(&w)).unwrap();
    assert!(acl.aces.contains(&key(&ow)), "matter one still names it");
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert!(
        !acl.aces.contains(&key(&ow)),
        "the last one forgotten removes it"
    );
}

/// #808's N2: whether a new `OWNER RIGHTS` entry is ours is decided under the ledger's lock,
/// with the record: matter two's entry is ours, so matter one's is listed ours, and either
/// forget order leaves nothing behind once both are forgotten.
#[test]
fn an_entry_listed_beside_one_of_ours_is_ours() {
    let w = world("apply-guard-ours", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let matter = w.canonical("matter");
    let ow = guarded_plan(&w)
        .grants()
        .iter()
        .find(|g| g.path() == matter && g.rights() == Rights::OwnerModify)
        .cloned()
        .unwrap();
    let mut two = AdmittedPlan::from_grants(matter_two(), vec![ow.clone()]);
    apply(&mut ledger, &mut two, &mut acl, &mut around(&w)).unwrap();
    assert!(acl.aces.contains(&key(&ow)), "matter two wrote ours");
    let mut one = guarded_plan(&w);
    apply(&mut ledger, &mut one, &mut acl, &mut around(&w)).unwrap();
    assert_eq!(
        ledger.entries().filter(|e| e.found).count(),
        0,
        "ours, not IT's"
    );
    forget_matter(&mut ledger, matter_two(), &mut acl, &mut around(&w)).unwrap();
    assert!(acl.aces.contains(&key(&ow)), "matter one still names it");
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert!(
        !acl.aces.contains(&key(&ow)),
        "the last one forgotten removes it"
    );
}

/// #808's N2: the check of whether a new `OWNER RIGHTS` entry is there runs with the ledger's
/// lock held, so no forget can come between the answer and the mark: a forget then would have
/// to wait for the lock (here, a try at it fails).
#[test]
fn the_check_for_its_entry_runs_under_the_ledgers_lock() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    let w = world("apply-guard-found-lock", &["sib1"]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    // Another matter's entry, so the ledger file (and its lock) exists.
    let mut two = AdmittedPlan::from_grants(matter_two(), vec![sibling_deny(&w)]);
    ledger.record_pending(&mut two).unwrap();
    let mut one = guarded_plan(&w);
    let matter = w.canonical("matter");
    let ow = one
        .grants()
        .iter()
        .find(|g| g.path() == matter && g.rights() == Rights::OwnerModify)
        .cloned()
        .unwrap();
    acl.aces.insert(key(&ow));
    let lock = ledger_path(&w.at("data")).with_file_name(crate::windows::ledger::LEDGER_LOCK);
    let seen: Rc<RefCell<Vec<bool>>> = Rc::default();
    let record = Rc::clone(&seen);
    let at = ow.path().to_owned();
    acl.on_present = Some(Box::new(move |grant: &Grant| {
        if grant.rights() == Rights::OwnerModify && grant.path() == at {
            let free = crate::windows::write::lock_exclusive_for(&lock, Duration::ZERO).is_ok();
            record.borrow_mut().push(free);
        }
    }));
    apply(&mut ledger, &mut one, &mut acl, &mut around(&w)).unwrap();
    acl.on_present = None;
    // The record's check, under the lock, then the check before the write, which needs none.
    assert_eq!(*seen.borrow(), [false, true]);
    assert_eq!(ledger.entries().filter(|e| e.found).count(), 1, "IT's");
}
