//! The S15 `PR3b` round 2 review's defects in the ledger, each as a test that failed before its
//! fix.

use super::*;

/// A Low label grant on `path`, reaching `inherit`.
fn label_grant(path: &Path, inherit: Inherit) -> Grant {
    Grant {
        path: path.to_owned(),
        sid: Sid::parse(LOW_LABEL_SID).unwrap(),
        rights: Rights::LowLabel,
        inherit,
        kind: PathKind::CredentialLink,
    }
}

/// N4: reconcile leaves a deny entry whose permission entry is there `pending`, for its
/// matter's next launch to walk strictly; an allow is marked `applied` as before.
#[test]
fn reconcile_leaves_a_present_deny_pending() {
    let temp = TempDir::new("ledger-r2-n4");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    let done = ledger.reconcile(|_| Ok(true)).unwrap();
    assert_eq!((done.applied, done.deferred), (2, 1));
    let after = [
        (1, EntryState::Applied),
        (2, EntryState::Applied),
        (3, EntryState::Pending),
    ];
    assert_eq!(states(&open(&path)), after);
}

/// N5: a forget records the hand-back owed before any removal; while it is owed, reconcile
/// keeps the matter's entries whose permission entry is gone.
#[test]
fn a_matter_whose_handback_is_owed_keeps_its_entries_through_reconcile() {
    let temp = TempDir::new("ledger-r2-n5");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    let pick = |e: &Entry| (e.matter == matter_one()).then_some(Take::Remove);
    let seen = |_: &Entry| -> io::Result<()> {
        // Under the lock, before this removal, the hand-back is on disk.
        assert!(open(&path).handback_owed(&Sid::parse(USER_SID).unwrap()));
        Err(io::Error::other("stop here"))
    };
    let taken = ledger
        .take_back_with(Some(matter_one()), pick, &mut held::ByPath, seen)
        .unwrap();
    assert_eq!(taken.kept, 3);
    let done = ledger.reconcile(|_| Ok(false)).unwrap();
    assert_eq!((done.dropped, done.deferred), (0, 3));
    assert_eq!(ids(&open(&path)).len(), 3);
    ledger.clear_handbacks(matter_one()).unwrap();
    let done = ledger.reconcile(|_| Ok(false)).unwrap();
    assert_eq!(done.dropped, 3);
}

/// N7: our own Low label, found where a label grant goes, is never recorded as the lawyer's;
/// another label is.
#[test]
fn our_own_label_is_never_recorded_as_the_prior_one() {
    let temp = TempDir::new("ledger-r2-n7");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let folder = label_grant(&abs("m1"), Inherit::Tree);
    let link = label_grant(&abs("m1.state/auth.json"), Inherit::ThisOnly);
    let mut plan = AdmittedPlan::from_grants(matter_one(), vec![folder, link]);
    ledger.record_pending(&mut plan).unwrap();
    let (folder_id, link_id) = (plan.entries()[0], plan.entries()[1]);
    let low = |flags, mask| Label {
        level: 4096,
        flags,
        mask,
    };
    // Ours on a folder (object and container inherit), and ours on a file.
    ledger
        .set_prior_label(&plan, folder_id, low(0x3, 0x1))
        .unwrap();
    ledger
        .set_prior_label(&plan, link_id, low(0x10, 0x1))
        .unwrap();
    assert!(ledger.entries().all(|e| e.prior_label.is_none()));
    // The lawyer's own: no-read-up too.
    ledger.set_prior_label(&plan, link_id, low(0, 0x3)).unwrap();
    let recorded: Vec<Option<Label>> = open(&path).entries().map(|e| e.prior_label).collect();
    assert_eq!(recorded, vec![None, Some(low(0, 0x3))]);
}
