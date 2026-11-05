//! The S15 `PR3b` round 2 review's defects in the apply layer, each as a test that failed before
//! its fix.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;
use crate::windows::acl_plan::LOW_LABEL_SID;

/// A third matter, for the cross-matter cases.
fn matter_three() -> MatterId {
    "01890a5d-ac96-774b-8cce-b302099a8059".parse().unwrap()
}

/// `matter`'s plan with its matter folder at `rel` for `pool`.
fn matter_at(w: &World, matter: MatterId, rel: &str, pool: &str) -> AdmittedPlan {
    let modify = Grant {
        path: w.canonical(rel),
        sid: sid(pool),
        rights: Rights::Modify,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    };
    AdmittedPlan::from_grants(matter, vec![modify])
}

fn launch(w: &World, ledger: &mut Ledger, acl: &mut FakeAcl) {
    let mut plan = launch_plan(w, &["sib1"]);
    apply(ledger, &mut plan, acl, &mut around(w)).unwrap();
}

/// A launch's refusal: the failure and the child it names.
fn refusal(result: Result<Applied, Error>) -> (ApplyFailure, Option<PathBuf>) {
    match result {
        Err(Error::AclApply { failure, at, .. }) => (failure, at),
        other => panic!("{other:?}"),
    }
}

/// N4: reconcile walks a crashed sibling's tree plainly but leaves its deny pending, so the
/// matter's next launch walks it strictly and only then marks it applied.
#[test]
fn a_crashed_siblings_deny_stays_pending_until_the_next_launch_walks_it_strictly() {
    let w = world("r2-n4", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let sib1 = w.canonical("sib1");
    acl.panic_walk = Some(sib1.clone());
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        let mut plan = launch_plan(&w, &["sib1"]);
        drop(apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)));
    }));
    assert!(crashed.is_err());
    acl.panic_walk = None;
    let mut ledger = super::ledger(&w);
    let state = |l: &Ledger| l.entries().find(|e| e.path == sib1).map(|e| e.state);
    let done = reconcile(&mut ledger, &mut acl).unwrap();
    assert!(done.deferred >= 1, "{done:?}");
    assert!(acl.walks.iter().any(|(r, _, s)| *r == sib1 && !s));
    assert_eq!(state(&ledger), Some(EntryState::Pending));

    acl.walks.clear();
    launch(&w, &mut ledger, &mut acl);
    assert!(acl.walks.iter().any(|(r, _, s)| *r == sib1 && *s));
    assert_eq!(state(&ledger), Some(EntryState::Applied));
}

/// N5: a hand-back that fails stays owed: the pool user is held back from reuse, reconcile
/// keeps the matter's entries, and a retried forget finishes the hand-back and clears it.
#[test]
fn a_failed_handback_stays_owed_through_reconcile_and_a_retry_clears_it() {
    let w = world("r2-n5", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    launch(&w, &mut ledger, &mut acl);
    let matter = w.canonical("matter");
    acl.fail_reclaim = Some(matter.clone());
    let error = forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap_err();
    assert!(matches!(error, Error::AclApply { .. }), "{error:?}");
    assert!(super::ledger(&w).handback_owed(&sid(USER_SID)));
    assert!(!super::ledger(&w).handback_owed(&sid(OTHER_POOL_SIDS[0])));

    let mut ledger = super::ledger(&w);
    reconcile(&mut ledger, &mut acl).unwrap();
    let after = super::ledger(&w);
    assert!(after.handback_owed(&sid(USER_SID)));
    assert!(
        after
            .entries()
            .any(|e| e.matter == matter_one() && e.path == matter),
        "the forgotten matter's entries are kept"
    );

    acl.fail_reclaim = None;
    acl.reclaimed.clear();
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert!(acl.reclaimed.iter().any(|(root, _)| *root == matter));
    let after = super::ledger(&w);
    assert!(!after.handback_owed(&sid(USER_SID)));
    assert_eq!(after.handbacks_owed(), Vec::new());
    assert!(after.entries().all(|e| e.matter != matter_one()));
}

/// N5: the hand-back owed is on disk before any removal, so a crash during one leaves it.
#[test]
fn a_crash_during_forget_leaves_the_handback_owed() {
    let w = world("r2-n5-crash", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    launch(&w, &mut ledger, &mut acl);
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
    assert!(super::ledger(&w).handback_owed(&sid(USER_SID)));
}

/// N5: a child the lawyer can't open in the forgotten matter's own tree is set aside and the
/// hand-back walk run again.
#[test]
fn a_shut_child_in_a_forgotten_matter_is_set_aside_and_the_handback_run_again() {
    let w = world("r2-n5-shut", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    launch(&w, &mut ledger, &mut acl);
    let matter = w.canonical("matter");
    acl.shut = vec![matter.join("x")];
    let mut around = around(&w);
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around).unwrap();
    assert_eq!(around.set_aside, vec![matter.join("x")]);
    assert!(acl.reclaimed.iter().any(|(root, _)| *root == matter));
    assert!(!super::ledger(&w).handback_owed(&sid(USER_SID)));
}

/// N5, from the Windows run: under the `C:\m` layout the lawyer may not write the agent's files
/// nor set them aside. The forget refuses naming the child, and the hand-back stays owed, so the
/// pool user isn't reused before PR4's elevated clean-up.
#[test]
fn a_handback_the_lawyer_cant_do_names_the_child_and_stays_owed() {
    let w = world("r2-n5-cm", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    launch(&w, &mut ledger, &mut acl);
    let matter = w.canonical("matter");
    let owed = |w: &World| super::ledger(w).handback_owed(&sid(USER_SID));

    // The hand-back's write is refused on a child.
    acl.fail_reclaim = Some(matter.clone());
    let result = forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w));
    let Err(Error::AclApply { failure, at, .. }) = result else {
        panic!("{result:?}");
    };
    let denied = ApplyFailure::Io(io::ErrorKind::PermissionDenied);
    assert_eq!((failure, at), (denied, Some(matter.join("child"))));
    assert!(owed(&w));

    // A child the lawyer can't open, and its set-aside fails too.
    acl.fail_reclaim = None;
    let shut = matter.join("x");
    acl.shut = vec![shut.clone()];
    let mut around = Around {
        fail_set_aside: true,
        ..around(&w)
    };
    let result = forget_matter(&mut ledger, matter_one(), &mut acl, &mut around);
    let Err(Error::AclApply { failure, at, .. }) = result else {
        panic!("{result:?}");
    };
    assert_eq!((failure, at), (ApplyFailure::ChildShut, Some(shut)));
    assert!(owed(&w));
    assert!(
        super::ledger(&w)
            .entries()
            .any(|e| e.matter == matter_one() && e.path == matter)
    );
}

/// N7: our own Low label, already on the credential link, is never recorded as the lawyer's
/// and so never put back.
#[test]
fn our_own_label_on_the_link_is_never_recorded_as_the_lawyers() {
    let w = world("r2-n7-own", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let link = w.canonical(STATE).join("auth.json");
    let ours = Label {
        level: 4096,
        flags: 0,
        mask: 0x1,
    };
    acl.labels.insert(link.clone(), ours);
    launch(&w, &mut ledger, &mut acl);
    let recorded = ledger
        .entries()
        .find(|e| e.path == link && e.rights == Rights::LowLabel)
        .map(|e| e.prior_label);
    assert_eq!(recorded, Some(None));
    forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert!(
        acl.restored.iter().all(|(path, _)| *path != link),
        "{:?}",
        acl.restored
    );
}

/// Every `applied` entry's permission entry is there.
fn no_applied_entry_lacks_its_entry(w: &World, acl: &FakeAcl, step: &str) {
    for e in super::ledger(w).entries() {
        if e.state == EntryState::Applied {
            assert!(acl.aces.contains(&acl.key_of(&e.grant())), "{step}: {e:?}");
        }
    }
}

/// N7: two matters whose credential links are one file share its Low label. Forgetting them
/// in either order leaves no Low label behind, never an applied entry without its label, and
/// puts the lawyer's own label back once, when the last one goes.
#[test]
fn two_matters_sharing_a_credentials_file_leave_no_low_label_in_either_order() {
    for first in [matter_one(), matter_two()] {
        let w = world(&format!("r2-n7-{first}"), &[]);
        w.dir.file("other/m2s/auth.json", "{}");
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        let link = w.canonical(STATE).join("auth.json");
        let link2 = w.canonical("other/m2s/auth.json");
        acl.links.insert(link2.clone(), link.clone());
        // The lawyer's own label on the credentials file: Medium, no-write-up.
        let lawyers = Label {
            level: 8192,
            flags: 0,
            mask: 0x1,
        };
        acl.labels.insert(link.clone(), lawyers);
        launch(&w, &mut ledger, &mut acl);
        let on_link2 = |sid_text: &str, rights| Grant {
            path: link2.clone(),
            sid: sid(sid_text),
            rights,
            inherit: Inherit::ThisOnly,
            kind: PathKind::CredentialLink,
        };
        let mut two = AdmittedPlan::from_grants(
            matter_two(),
            vec![
                on_link2(OTHER_POOL_SIDS[0], Rights::ReadWriteData),
                on_link2(LOW_LABEL_SID, Rights::LowLabel),
            ],
        );
        apply(&mut ledger, &mut two, &mut acl, &mut around(&w)).unwrap();
        no_applied_entry_lacks_its_entry(&w, &acl, "launched");

        let second = if first == matter_one() {
            matter_two()
        } else {
            matter_one()
        };
        for matter in [first, second] {
            forget_matter(&mut ledger, matter, &mut acl, &mut around(&w)).unwrap();
            no_applied_entry_lacks_its_entry(&w, &acl, &format!("{first} then {matter}"));
        }
        let low = acl
            .aces
            .iter()
            .any(|(p, _, r, _)| *p == link && *r == Rights::LowLabel);
        assert!(!low, "{first}: our label is gone");
        let put_back = acl.labels.get(&link).filter(|l| l.level == 4096);
        assert_eq!(put_back, None, "{first}: no Low label put back");
        let theirs = acl.restored.iter().filter(|(_, l)| *l == lawyers).count();
        assert_eq!(theirs, 1, "{first}: the lawyer's label put back once");
    }
}

/// N8: a credentials file inside the state folder, or the credential link itself, refuses the
/// launch: the check would compare the link with itself.
#[test]
fn a_credentials_file_in_the_state_folder_or_the_link_itself_refuses_the_launch() {
    let w = world("r2-n8", &[]);
    w.dir.file(&format!("{STATE}/sub/auth.json"), "{}");
    let in_state = [
        w.at(&format!("{STATE}/auth.json")),
        w.at(&format!("{STATE}/sub/auth.json")),
    ];
    for credentials in in_state {
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        let mut around = Around {
            credentials: Some(credentials.clone()),
            ..Around::default()
        };
        let mut plan = launch_plan(&w, &[]);
        let result = apply(&mut ledger, &mut plan, &mut acl, &mut around);
        assert!(
            matches!(
                result,
                Err(Error::AclApply {
                    path_kind: PathKind::CredentialLink,
                    failure: ApplyFailure::Changed,
                    ..
                })
            ),
            "{credentials:?}: {result:?}"
        );
        assert!(acl.aces.is_empty());
        assert_eq!(ledger.entries().count(), 0);
    }
}

/// N9: a child the lawyer can't open in a sibling outside the matter refuses the launch,
/// naming it, and nothing is moved.
#[test]
fn a_shut_child_in_a_sibling_refuses_the_launch_and_nothing_is_moved() {
    let w = world("r2-n9", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let child = w.canonical("sib1").join("child");
    acl.shut = vec![child.clone()];
    let mut around = around(&w);
    let mut plan = launch_plan(&w, &["sib1"]);
    let result = apply(&mut ledger, &mut plan, &mut acl, &mut around);
    assert_eq!(refusal(result), (ApplyFailure::ChildShut, Some(child)));
    assert_eq!(around.set_aside, Vec::<PathBuf>::new());
    assert!(acl.aces.is_empty());
}

/// N11: every child the lawyer can't open in the matter is set aside, at launch and after the
/// session, before the walk passes.
#[test]
fn every_shut_child_in_the_matter_is_set_aside_before_the_walk_passes() {
    let w = world("r2-n11", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let matter = w.canonical("matter");
    let (a, b) = (matter.join("a"), matter.join("b/c"));
    acl.shut = vec![a.clone(), b.clone()];
    let mut around = around(&w);
    let mut plan = launch_plan(&w, &["sib1"]);
    apply(&mut ledger, &mut plan, &mut acl, &mut around).unwrap();
    assert_eq!(around.set_aside, vec![a.clone(), b.clone()]);

    acl.shut = vec![b.clone(), a.clone()];
    around.set_aside.clear();
    repair_after_session(&ledger, matter_one(), &mut acl, &mut around).unwrap();
    assert_eq!(around.set_aside, vec![b, a]);
}

/// N11: a set-aside that fails refuses the launch, naming the child.
#[test]
fn a_set_aside_that_fails_refuses_the_launch_naming_the_child() {
    let w = world("r2-n11-fail", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let child = w.canonical("matter").join("a");
    acl.shut = vec![child.clone()];
    let mut around = Around {
        fail_set_aside: true,
        ..around(&w)
    };
    let mut plan = launch_plan(&w, &["sib1"]);
    let result = apply(&mut ledger, &mut plan, &mut acl, &mut around);
    assert_eq!(refusal(result), (ApplyFailure::ChildShut, Some(child)));
    assert!(acl.aces.is_empty());
}

/// N11: a child that comes back after its set-aside refuses, naming it.
#[test]
fn a_shut_child_that_comes_back_after_its_set_aside_refuses() {
    let w = world("r2-n11-back", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let child = w.canonical("matter").join("a");
    acl.shut = vec![child.clone(), child.clone()];
    let mut around = around(&w);
    let mut plan = launch_plan(&w, &["sib1"]);
    let result = apply(&mut ledger, &mut plan, &mut acl, &mut around);
    assert_eq!(
        refusal(result),
        (ApplyFailure::ChildShut, Some(child.clone()))
    );
    assert_eq!(around.set_aside, vec![child]);
}

/// N12: when two matters deny a sibling that comes to hold a third matter, and one of their
/// walk-down replacements fails, that matter keeps its deny.
#[test]
fn a_matter_whose_walk_down_replacement_failed_keeps_its_deny() {
    let w = world("r2-n12", &[]);
    for rel in ["sib1/x/sub", "sib1/x/z", "sib1/y"] {
        w.dir.dir(rel);
    }
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    launch(&w, &mut ledger, &mut acl);
    let mut two = AdmittedPlan::from_grants(matter_two(), vec![sibling_deny(&w)]);
    ledger.record_pending(&mut two).unwrap();
    let ids = two.entries().to_vec();
    ledger.mark_applied(&two, &ids).unwrap();

    // Matter one's replacement comes first, and its write on `sib1/y` fails once.
    acl.fail_add = Some(w.canonical("sib1/y"));
    acl.fail_add_once = true;
    let mut three = matter_at(&w, matter_three(), "sib1/x/sub", OTHER_POOL_SIDS[1]);
    apply(&mut ledger, &mut three, &mut acl, &mut around(&w)).unwrap();
    assert!(acl.aces.contains(&key(&sibling_deny(&w))), "the deny stays");
    let sib1 = w.canonical("sib1");
    let deny_of = |matter| {
        super::ledger(&w).entries().any(|e| {
            e.matter == matter
                && e.path == sib1
                && e.kind == PathKind::Sibling
                && e.inherit == Inherit::Tree
        })
    };
    assert!(deny_of(matter_one()), "matter one still lists its deny");
    assert!(!deny_of(matter_two()), "matter two's was replaced");
}
