//! The S15 `PR3b` round 6 fixes in the apply layer: a child the lawyer can't open in another
//! matter's fenced tree is set aside into that matter's set-aside, and a `Changed` carried
//! through an `io::Error` stays `Changed`.

use super::*;
use crate::windows::fence::RegisteredMatter;

/// Matter two, at `other/m2` with its state folder beside it, assigned the first other pool
/// user; `fenced` as given.
fn matter_two_at(w: &World, fenced: bool) -> RegisteredMatter {
    w.dir.dir("other/m2.state");
    RegisteredMatter {
        id: matter_two(),
        folders: MatterFolders {
            root: w.at("other/m2"),
            state: w.at("other/m2.state"),
            notes: None,
        },
        pool_user: sid(OTHER_POOL_SIDS[0]),
        fenced,
    }
}

/// Round 6, issue 1: matter two's agent leaves a file only it can open in its own matter. When
/// the pool grows, `fence_registered` walked matter two's tree strictly and refused on it
/// (`ChildShut` at `other/m2/planted`), so no matter got the new user's denies. It is now set
/// aside into matter two's set-aside (and one in matter one's tree into matter one's), and
/// every fence is applied.
#[test]
fn growing_the_pool_sets_a_planted_child_aside_into_the_matter_whose_tree_holds_it() {
    let w = world("apply-grow-planted", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let one = RegisteredMatter {
        id: matter_one(),
        folders: MatterFolders {
            root: w.at("matter"),
            state: w.at(STATE),
            notes: Some(w.at(NOTES)),
        },
        pool_user: sid(USER_SID),
        fenced: true,
    };
    let registered = [one, matter_two_at(&w, true)];
    let small = [sid(USER_SID), sid(OTHER_POOL_SIDS[0])];
    fence_registered(&mut ledger, &registered, &small, &mut acl, &mut around(&w)).unwrap();

    let planted = w.canonical("other/m2").join("planted");
    let in_one = w.canonical("matter").join("shut.txt");
    acl.shut = vec![planted.clone(), in_one.clone()];
    let mut around = around(&w);
    let grown = [small[0].clone(), small[1].clone(), sid(OTHER_POOL_SIDS[1])];
    let next = fence_registered(&mut ledger, &registered, &grown, &mut acl, &mut around).unwrap();

    assert_eq!(next.written, 3 + 2);
    let mut moved: Vec<(PathBuf, MatterId)> =
        around.set_aside.into_iter().zip(around.owners).collect();
    moved.sort();
    let mut expected = vec![(planted, matter_two()), (in_one, matter_one())];
    expected.sort();
    assert_eq!(moved, expected);
    for folder in ["matter", "other/m2", "other/m2.state"] {
        let path = w.canonical(folder);
        assert!(has(&acl, &path, OTHER_POOL_SIDS[1], Rights::DenyReadWrite));
    }
}

/// Round 6, issue 1: matter one's launch plans matter two's pending fence and walks matter
/// two's tree strictly. A child there that only matter two's agent can open refused matter
/// one's launch. It is now set aside into matter two's set-aside, not matter one's; a child in
/// matter one's own tree still goes to matter one's.
#[test]
fn a_launch_sets_a_child_in_another_matters_pending_fence_aside_into_that_matter() {
    let w = world("apply-fence-planted", &[]);
    let neighbours = Neighbours {
        registered: vec![matter_two_at(&w, false)],
        reach: Vec::new(),
    };
    let policy = w.full_policy();
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let folders = FakeFolders::default();
    let admitted = admit_with(&policy, &w.at("data"), &neighbours, ok, &folders).unwrap();
    let mut plan = plan(&admitted, &sids(), matter_one()).unwrap();
    assert!(
        plan.owners().contains(&matter_two()),
        "matter two's fence is planned"
    );

    let planted = w.canonical("other/m2").join("planted");
    let own = w.canonical("matter").join("shut.txt");
    let mut acl = FakeAcl {
        shut: vec![planted.clone(), own.clone()],
        ..FakeAcl::default()
    };
    let mut around = around(&w);
    let mut ledger = ledger(&w);
    apply(&mut ledger, &mut plan, &mut acl, &mut around).unwrap();

    let mut moved: Vec<(PathBuf, MatterId)> =
        around.set_aside.into_iter().zip(around.owners).collect();
    moved.sort();
    let mut expected = vec![(planted, matter_two()), (own, matter_one())];
    expected.sort();
    assert_eq!(moved, expected);
    let fenced = w.canonical("other/m2");
    assert!(has(&acl, &fenced, USER_SID, Rights::DenyReadWrite));
}

/// Round 6: a child outside every matter's trees (here a sibling's) still refuses the launch
/// and is not set aside, though the launch now sets aside children in other matters' trees
/// (round 2, N9).
#[test]
fn a_child_in_a_sibling_still_refuses_a_launch_that_applies_another_matters_fence() {
    let w = world("apply-fence-sibling", &[]);
    let neighbours = Neighbours {
        registered: vec![matter_two_at(&w, false)],
        reach: Vec::new(),
    };
    let policy = w.full_policy();
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let folders = FakeFolders {
        listing: vec![w.canonical("sib1")],
        ..FakeFolders::default()
    };
    let admitted = admit_with(&policy, &w.at("data"), &neighbours, ok, &folders).unwrap();
    let mut plan = plan(&admitted, &sids(), matter_one()).unwrap();
    let shut = w.canonical("sib1").join("x");
    let mut acl = FakeAcl {
        shut: vec![shut.clone()],
        ..FakeAcl::default()
    };
    let mut around = around(&w);
    let refused = apply(&mut ledger(&w), &mut plan, &mut acl, &mut around).unwrap_err();
    assert!(
        matches!(
            &refused,
            Error::AclApply { failure: ApplyFailure::ChildShut, at: Some(at), .. } if *at == shut
        ),
        "{refused:?}"
    );
    assert_eq!(around.set_aside, Vec::<PathBuf>::new());
}

/// Round 6, issue 2: a removal by file ID that met `Changed` (a volume or file ID that doesn't
/// match) reported it as `Io(Other)`, because the failure went through an `io::Error` as text.
/// It now rides inside the error and comes back as itself; a plain error is still `Io`.
#[test]
fn a_failure_carried_through_an_io_error_comes_back_as_itself() {
    let carried = io::Error::from(ApplyFailure::Changed);
    assert_eq!(ApplyFailure::of(&carried), ApplyFailure::Changed);
    assert_eq!(carried.to_string(), ApplyFailure::Changed.to_string());
    let plain = io::Error::from(io::ErrorKind::PermissionDenied);
    assert_eq!(
        ApplyFailure::of(&plain),
        ApplyFailure::Io(io::ErrorKind::PermissionDenied)
    );
}
