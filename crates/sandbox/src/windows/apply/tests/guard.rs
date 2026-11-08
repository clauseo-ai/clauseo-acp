//! The guard on what the pool writes (`Policy::native_code_deny`) through the ledger: applied
//! with the plan's other entries, its trees walked, and gone with them when the matter is.

use super::*;
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
