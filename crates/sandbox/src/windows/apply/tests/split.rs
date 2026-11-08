//! The fence's deny on a matter folder, split in two (R18): a ledger that lists the one
//! whole-tree entry from before is moved to the pair at the next launch, the folder denied
//! throughout.

use super::reads::access::{READ, USERS, granted};
use super::*;

/// Matter two's fence as a launch before R18 wrote it: one whole-tree deny on its folder for
/// each other pool user.
fn whole_fence(w: &World) -> AdmittedPlan {
    let root = w.canonical("other/m2");
    let grants = [USER_SID, OTHER_POOL_SIDS[1]]
        .iter()
        .map(|s| Grant {
            path: root.clone(),
            sid: sid(s),
            rights: Rights::DenyReadWrite,
            inherit: Inherit::Tree,
            kind: PathKind::Matter,
        })
        .collect();
    AdmittedPlan::from_grants(matter_two(), grants)
}

#[test]
fn a_whole_tree_fence_is_split_at_the_next_launch_and_the_old_entry_goes() {
    let w = World::new("split-migrate");
    w.other_tree();
    // Matter two's state folder exists, so its fence is whole once applied.
    w.dir
        .dir("data/agent-state/01890a5d-ac96-774b-8cce-b302099a8058/c");
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let top = dunce::canonicalize(w.dir.path()).unwrap();
    acl.aces.insert(reads::access::volume_default(&top));
    apply(&mut ledger, &mut whole_fence(&w), &mut acl, &mut around(&w)).unwrap();
    let m2 = w.neighbour_m2().registered.remove(0);
    let pool = [
        sid(USER_SID),
        sid(OTHER_POOL_SIDS[0]),
        sid(OTHER_POOL_SIDS[1]),
    ];
    let applied = |l: &Ledger| l.fence_applied(m2.id, &m2.folders, &m2.pool_user, &pool);
    // The old shape isn't the fence as planned now: the launcher passes it as unfenced.
    assert!(!applied(&ledger));

    // During the launch, before the take-back, the old entry and the new pair are both there.
    let root = w.canonical("other/m2");
    let whole = |acl: &FakeAcl| {
        acl.aces
            .iter()
            .any(|k| k.0 == root && k.2 == Rights::DenyReadWrite && k.3 == Inherit::Tree)
    };
    let policy = w.reading(&[]);
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let (data, others) = (w.at("data"), w.neighbour_m2());
    let admitted = admit_with(&policy, &data, &others, ok, &FakeFolders::default()).unwrap();
    let mut plan = plan(&admitted, &sids(), matter_one()).unwrap();
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();

    assert!(applied(&ledger));
    assert!(!whole(&acl));
    let whole_entries = ledger
        .entries()
        .filter(|e| e.path == root && e.kind == PathKind::Matter && e.inherit == Inherit::Tree);
    assert_eq!(whole_entries.count(), 0);
    for inherit in [Inherit::ThisOnly, Inherit::Below] {
        let key = (
            root.clone(),
            USER_SID.to_owned(),
            Rights::DenyReadWrite,
            inherit,
        );
        assert!(acl.aces.contains(&key), "{inherit:?}");
    }
    // The folder and its children stay closed to the other pool users.
    let token = [USER_SID, GROUP_SID, USERS];
    for rel in ["other/m2", "other/m2/a.txt", "other/m2/docs/deep/c.txt"] {
        assert!(
            !granted(&acl.aces, &w.canonical(rel), &token, READ),
            "{rel}"
        );
    }
    // The old entry's tree is walked again, so its inherited copies go.
    assert!(acl.walks.iter().filter(|(r, _, _)| *r == root).count() >= 2);
}
