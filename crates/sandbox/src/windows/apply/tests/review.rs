//! The S15 PR3 review's defects in the apply layer, each as a test that failed before its fix.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;
use crate::windows::ledger::Entry;

/// Matter two's plan with its matter folder at `rel` (relative to `w`).
fn matter_two_at(w: &World, rel: &str) -> AdmittedPlan {
    let modify = Grant {
        path: w.canonical(rel),
        sid: sid(OTHER_POOL_SIDS[0]),
        rights: Rights::Modify,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    };
    AdmittedPlan::from_grants(matter_two(), vec![modify])
}

fn sibling_deny_on(w: &World, rel: &str) -> bool {
    let path = w.canonical(rel);
    let listed = |l: &Ledger| {
        l.entries()
            .any(|e| e.path == path && e.kind == PathKind::Sibling)
    };
    let mut ledger = ledger(w);
    ledger.pending().unwrap();
    listed(&ledger)
}

/// H3: a refused first launch of a folder that was a sibling leaves its deny in place.
#[test]
fn a_refused_first_launch_leaves_the_sibling_deny() {
    let w = world("review-h3", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut one = launch_plan(&w, &["sib1"]);
    apply(&mut ledger, &mut one, &mut acl, &mut around(&w)).unwrap();
    acl.fail_walk = Some((w.canonical("sib1"), ApplyFailure::ProtectedChild));
    let mut two = matter_two_at(&w, "sib1");
    let error = apply(&mut ledger, &mut two, &mut acl, &mut around(&w)).unwrap_err();
    assert!(matches!(error, Error::AclApply { .. }), "{error:?}");
    assert!(has(&acl, &w.canonical("sib1"), GROUP_SID, Rights::DenyRead));
    assert!(sibling_deny_on(&w, "sib1"));
}

/// M1: a matter nested inside a sibling another matter denies doesn't lift that whole deny: the
/// deny is replaced, for the matter that wrote it, by the walk-down set.
#[test]
fn a_matter_nested_in_a_denied_sibling_gets_the_walk_down_set_in_its_place() {
    let w = world("review-m1", &[]);
    for rel in ["sib1/x/sub", "sib1/x/z", "sib1/y"] {
        w.dir.dir(rel);
    }
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let mut two = matter_two_at(&w, "sib1/x/sub");
    let applied = apply(&mut ledger, &mut two, &mut acl, &mut around(&w)).unwrap();
    assert!(applied.lifted >= 1, "{applied:?}");
    let tree_deny = |rel: &str| {
        acl.aces.contains(&(
            w.canonical(rel),
            GROUP_SID.to_owned(),
            Rights::DenyRead,
            Inherit::Tree,
        ))
    };
    assert!(!tree_deny("sib1"), "the whole sibling's deny is gone");
    for denied in ["sib1/y", "sib1/x/z"] {
        assert!(tree_deny(denied), "{denied}");
    }
    assert!(!tree_deny("sib1/x/sub") && !tree_deny("sib1/x"));
    for level in ["sib1", "sib1/x"] {
        let path = w.canonical(level);
        let files = acl.aces.iter().any(|(p, s, r, i)| {
            *p == path && s == GROUP_SID && *r == Rights::DenyRead && *i == Inherit::FilesInFolder
        });
        assert!(files, "files-only deny on {level}");
    }
    let replaced: BTreeSet<MatterId> = ledger
        .entries()
        .filter(|e| e.path.starts_with(w.canonical("sib1")) && e.rights == Rights::DenyRead)
        .map(|e| e.matter)
        .collect();
    assert_eq!(replaced, [matter_one()].into_iter().collect());
}

/// M2: a crash partway through a sibling's walk leaves its entry pending, and reconcile walks
/// that tree again. The sibling's deny stays pending (round 2's N4): its matter's next launch
/// walks it strictly (`round2.rs`).
#[test]
fn a_crash_during_a_sibling_walk_is_walked_again_by_reconcile() {
    let w = world("review-m2", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.panic_walk = Some(w.canonical("sib1"));
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        let mut plan = launch_plan(&w, &["sib1"]);
        apply(&mut ledger, &mut plan, &mut acl, &mut around(&w))
    }));
    assert!(crashed.is_err());
    acl.panic_walk = None;
    let mut ledger = super::ledger(&w);
    let sib1 = w.canonical("sib1");
    let state = |l: &Ledger| l.entries().find(|e| e.path == sib1).map(|e| e.state);
    assert_eq!(state(&ledger), Some(EntryState::Pending));
    acl.walks.clear();
    let done = reconcile(&mut ledger, &mut acl).unwrap();
    assert!(done.applied >= 1, "{done:?}");
    assert!(acl.walks.iter().any(|(r, _, _)| *r == sib1));
    assert_eq!(state(&ledger), Some(EntryState::Pending));
}

/// M2, the next launch: an entry an earlier launch left pending gets its walk then.
#[test]
fn the_next_launch_walks_a_tree_whose_entry_is_still_pending() {
    let w = world("review-m2-launch", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let deny = sibling_deny(&w);
    acl.aces.insert(key(&deny));
    let mut earlier = AdmittedPlan::from_grants(matter_one(), vec![deny]);
    ledger.record_pending(&mut earlier).unwrap();
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let sib1 = w.canonical("sib1");
    assert!(acl.walks.iter().any(|(r, _, s)| *r == sib1 && *s));
    assert!(ledger.entries().all(|e| e.state == EntryState::Applied));
}

/// M3: rollback decides what is shared from the file as read under its lock: another process
/// that listed the same sibling deny meanwhile keeps its permission entry.
#[test]
fn a_rollback_reads_who_shares_a_deny_under_its_lock() {
    let w = world("review-m3", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let path = ledger_path(&w.at("data"));
    let deny = sibling_deny(&w);
    let notes = w.canonical(NOTES);
    acl.on_add = Some(Box::new(move |grant: &Grant| {
        if grant.path() == notes {
            let mut other = Ledger::open(&path, &trusted(), &reserved()).unwrap();
            let mut two = AdmittedPlan::from_grants(matter_two(), vec![deny.clone()]);
            other.record_pending(&mut two).unwrap();
            let ids = two.entries().to_vec();
            other.mark_applied(&two, &ids).unwrap();
        }
    }));
    acl.fail_add = Some(w.canonical(NOTES));
    let mut plan = launch_plan(&w, &["sib1"]);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap_err();
    assert!(
        acl.aces.contains(&key(&sibling_deny(&w))),
        "matter two's deny kept"
    );
    let left: Vec<MatterId> = super::ledger(&w).entries().map(|e| e.matter).collect();
    assert_eq!(left, vec![matter_two()]);
}

/// M4: a crash during a rollback's removal, or during its walk, leaves no `applied` entry
/// without its permission entry; reconcile then settles them.
#[test]
fn a_crash_at_each_step_of_a_rollback_leaves_no_applied_entry_without_its_ace() {
    for step in ["remove", "walk"] {
        let w = world(&format!("review-m4-{step}"), &[]);
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        acl.fail_add = Some(w.canonical(NOTES));
        match step {
            "remove" => acl.panic_remove = true,
            _ => acl.panic_walk = Some(w.canonical("sib1")),
        }
        let mut plan = launch_plan(&w, &["sib1"]);
        let crashed = catch_unwind(AssertUnwindSafe(|| {
            drop(apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)));
        }));
        acl.panic_remove = false;
        acl.panic_walk = None;
        let mut ledger = super::ledger(&w);
        let lacking: Vec<&Entry> = ledger
            .entries()
            .filter(|e| e.state == EntryState::Applied && !acl.aces.contains(&key(&e.grant())))
            .collect();
        assert!(lacking.is_empty(), "{step}: {lacking:?} ({crashed:?})");
        reconcile(&mut ledger, &mut acl).unwrap();
        for e in ledger.entries() {
            assert!(acl.aces.contains(&key(&e.grant())), "{step}: {e:?}");
        }
    }
}

/// M4 for forget: a crash during a removal leaves the matter's entries marked removing (round 4),
/// not applied.
#[test]
fn a_crash_during_forget_leaves_its_entries_removing() {
    let w = world("review-m4-forget", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
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
    let ledger = super::ledger(&w);
    assert!(ledger.entries().count() > 0);
    assert!(ledger.entries().all(|e| e.state == EntryState::Removing));
}

/// L4: the lawyer's own explicit label is recorded before ours and put back on rollback.
#[test]
fn the_lawyers_label_is_recorded_and_put_back_on_rollback() {
    let w = world("review-l4", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let lawyers = Label {
        level: 4096,
        flags: 0,
        mask: 0x3,
    };
    acl.labels.insert(w.canonical("matter"), lawyers);
    acl.fail_add = Some(w.canonical(NOTES));
    let mut plan = launch_plan(&w, &["sib1"]);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap_err();
    assert!(
        acl.restored.contains(&(w.canonical("matter"), lawyers)),
        "{:?}",
        acl.restored
    );
}

/// L4 for forget: the recorded label goes back when the matter is forgotten.
#[test]
fn the_lawyers_label_is_put_back_when_the_matter_is_forgotten() {
    let w = world("review-l4-forget", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let lawyers = Label {
        level: 4096,
        flags: 0,
        mask: 0x3,
    };
    acl.labels.insert(w.canonical("matter"), lawyers);
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let matter = w.canonical("matter");
    let recorded = ledger
        .entries()
        .find(|e| e.path == matter && e.rights == Rights::LowLabel)
        .and_then(|e| e.prior_label);
    assert_eq!(recorded, Some(lawyers));
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert!(acl.restored.contains(&(matter, lawyers)));
}

/// L5: a credential link that is no longer the expected credentials file refuses the launch
/// before anything is recorded; so does a launch that names no credentials file.
#[test]
fn a_credential_link_to_another_file_refuses_the_launch() {
    let w = world("review-l5", &[]);
    for credentials in [Some(w.at("elsewhere/other.json")), None] {
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        let mut around = Around {
            credentials,
            ..Around::default()
        };
        let error = apply(
            &mut ledger,
            &mut launch_plan(&w, &[]),
            &mut acl,
            &mut around,
        );
        assert!(
            matches!(
                error,
                Err(Error::AclApply {
                    path_kind: PathKind::CredentialLink,
                    failure: ApplyFailure::Changed,
                    ..
                })
            ),
            "{error:?}"
        );
        assert!(acl.aces.is_empty());
        assert_eq!(ledger.entries().count(), 0);
    }
}

/// M5: forgetting a matter hands what its pool user made back, in its matter, state and notes
/// folders.
#[test]
fn forgetting_a_matter_reclaims_its_own_folders_for_the_desktop_user() {
    let w = world("review-m5-forget", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    let roots: BTreeSet<PathBuf> = acl.reclaimed.iter().map(|(r, _)| r.clone()).collect();
    for own in ["matter", STATE, NOTES] {
        assert!(roots.contains(&w.canonical(own)), "{own}");
    }
    assert!(
        acl.reclaimed
            .iter()
            .all(|(_, pool)| pool.contains(&sid(USER_SID)))
    );
}

/// M5: a child the lawyer can't open is set aside and the walk run again; the launch goes on.
#[test]
fn a_child_the_lawyer_cant_open_is_set_aside_and_the_walk_run_again() {
    let w = world("review-m5-shut", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.fail_walk = Some((w.canonical("matter"), ApplyFailure::ChildShut));
    acl.fail_once = true;
    let mut around = around(&w);
    let mut plan = launch_plan(&w, &["sib1"]);
    apply(&mut ledger, &mut plan, &mut acl, &mut around).unwrap();
    assert_eq!(around.set_aside, vec![w.canonical("matter").join("child")]);
}

/// M5: a strict refusal names the child for the lawyer.
#[test]
fn a_refused_walk_names_the_child() {
    let w = world("review-at", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.fail_walk = Some((w.canonical("matter"), ApplyFailure::BroadAllowChild));
    let error = apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    );
    let Err(Error::AclApply { at, .. }) = error else {
        panic!("{error:?}");
    };
    assert_eq!(at, Some(w.canonical("matter").join("child")));
}

/// M5: when the agent exits, its own trees are walked strictly again, so what it made open is
/// repaired then (here the fake records the strict walks).
#[test]
fn the_agents_own_trees_are_walked_strictly_after_its_session() {
    let w = world("review-m5-after", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut launch_plan(&w, &["sib1"]),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    acl.walks.clear();
    repair_after_session(&ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    for own in ["matter", STATE, NOTES] {
        let root = w.canonical(own);
        assert!(acl.walks.iter().any(|(r, _, s)| *r == root && *s), "{own}");
    }
}
