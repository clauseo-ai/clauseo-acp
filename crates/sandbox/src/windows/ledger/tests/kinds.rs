//! Design B's items in the ledger: the Low label, the pool users' denies, a sibling deny two
//! matters share, and a registered matter's fence.

use super::*;

#[test]
fn labels_and_the_pool_denies_are_recorded_and_forget_matter_returns_them() {
    // Each new kind of item: the Low label, another pool user's deny, the group's sibling deny.
    let temp = TempDir::new("ledger-new-kinds");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let matter = abs("m1");
    let item = |sid: &str, rights, kind| Grant {
        path: matter.clone(),
        sid: Sid::parse(sid).unwrap(),
        rights,
        inherit: Inherit::Tree,
        kind,
    };
    let grants = vec![
        item(LOW_LABEL_SID, Rights::LowLabel, PathKind::Matter),
        item(OTHER_POOL_SIDS[0], Rights::DenyReadWrite, PathKind::Matter),
        Grant {
            path: abs("Drafts"),
            ..item(GROUP_SID, Rights::DenyRead, PathKind::Sibling)
        },
    ];
    let mut plan = AdmittedPlan::from_grants(matter_one(), grants.clone());
    ledger.record_pending(&mut plan).unwrap();
    let reopened = open(&path);
    let read: Vec<Grant> = reopened.entries().map(Entry::grant).collect();
    assert_eq!(read, grants, "every kind is read back and trusted");
    let mut handed = Vec::new();
    let done = ledger
        .forget_matter(matter_one(), |entry| {
            handed.push(entry.grant());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        done,
        Forgotten {
            removed: 3,
            kept: 0,
            shared: 0
        }
    );
    assert_eq!(handed, grants);
}

#[test]
fn a_shared_sibling_deny_is_removed_only_with_the_last_matter_that_lists_it() {
    let temp = TempDir::new("ledger-shared-ace");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let deny = Grant {
        path: abs("Drafts"),
        sid: Sid::parse(GROUP_SID).unwrap(),
        rights: Rights::DenyRead,
        inherit: Inherit::Tree,
        kind: PathKind::Sibling,
    };
    for matter in [matter_one(), matter_two()] {
        let mut plan = AdmittedPlan::from_grants(matter, vec![deny.clone()]);
        ledger.record_pending(&mut plan).unwrap();
        let ids = plan.entries().to_vec();
        ledger.mark_applied(&plan, &ids).unwrap();
    }
    assert_eq!(
        open(&path).entries().count(),
        2,
        "each matter owns its entry"
    );
    let first = ledger.entries().next().unwrap().clone();
    assert!(ledger.is_shared(&first));
    let done = ledger
        .forget_matter(matter_one(), |_| panic!("matter two still needs the ACE"))
        .unwrap();
    assert_eq!(
        done,
        Forgotten {
            removed: 0,
            kept: 0,
            shared: 1
        }
    );
    let mut removed = Vec::new();
    let done = ledger
        .forget_matter(matter_two(), |entry| {
            removed.push(entry.grant());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        done,
        Forgotten {
            removed: 1,
            kept: 0,
            shared: 0
        }
    );
    assert_eq!(removed, [deny]);
    assert_eq!(open(&path).entries().count(), 0);
}

#[test]
fn a_fence_counts_as_applied_only_when_every_entry_is() {
    let temp = TempDir::new("ledger-fence-applied");
    let mut ledger = open(&ledger_path(temp.path()));
    let folders = crate::windows::fence::MatterFolders {
        root: abs("m2"),
        state: abs("m2-state"),
        notes: None,
    };
    let assigned = Sid::parse(OTHER_POOL_SIDS[0]).unwrap();
    let pool = trusted();
    let fence = crate::windows::fence::plan_fence(&folders, &assigned, &pool);
    assert!(!ledger.fence_applied(matter_two(), &folders, &assigned, &pool));
    let mut plan = AdmittedPlan::from_grants(matter_two(), fence);
    ledger.record_pending(&mut plan).unwrap();
    assert!(
        !ledger.fence_applied(matter_two(), &folders, &assigned, &pool),
        "pending"
    );
    let ids = plan.entries().to_vec();
    ledger.mark_applied(&plan, &ids).unwrap();
    assert!(ledger.fence_applied(matter_two(), &folders, &assigned, &pool));
}

#[test]
fn a_label_naming_anything_but_the_low_level_is_set_aside() {
    let temp = TempDir::new("ledger-label-trust");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let label = |sid: &str| Grant {
        path: abs("m1"),
        sid: Sid::parse(sid).unwrap(),
        rights: Rights::LowLabel,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    };
    let mut plan = AdmittedPlan::from_grants(matter_one(), vec![label(USER_SID)]);
    ledger.record_pending(&mut plan).unwrap();
    let reopened = open(&path);
    assert_eq!(reopened.entries().count(), 0);
    assert_eq!(reopened.set_aside_entries(), 1);
}

#[test]
fn entries_differing_in_inheritance_are_not_one_ace() {
    let temp = TempDir::new("ledger-shared-inherit");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let deny = |inherit| Grant {
        path: abs("Clients"),
        sid: Sid::parse(GROUP_SID).unwrap(),
        rights: Rights::DenyRead,
        inherit,
        kind: PathKind::Sibling,
    };
    let pairs = [
        (matter_one(), Inherit::Tree),
        (matter_two(), Inherit::FilesInFolder),
    ];
    for (matter, inherit) in pairs {
        let mut plan = AdmittedPlan::from_grants(matter, vec![deny(inherit)]);
        ledger.record_pending(&mut plan).unwrap();
    }
    assert!(!ledger.is_shared(ledger.entries().next().unwrap()));
    let mut removed = 0;
    let done = ledger
        .forget_matter(matter_one(), |_| {
            removed += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!((done.removed, done.shared, removed), (1, 0, 1));
}
