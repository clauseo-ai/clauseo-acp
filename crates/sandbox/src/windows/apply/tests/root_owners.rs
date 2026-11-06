//! S15 `PR4h`: a root the plan denies on that a pool user owns goes to the lawyer before
//! anything is recorded (main's hand-back, S15 `PR4a2`); only one whose hand-back fails refuses
//! the launch, as `PoolOwned { owned: Root }`.

use super::fake::LAWYER;
use super::*;
use crate::windows::test_world::registered;
use crate::windows::writer::Owned;

/// The ledger file's bytes now (`None` when there is none).
fn ledger_bytes(w: &World) -> Option<Vec<u8>> {
    std::fs::read(ledger_path(&w.at("data"))).ok()
}

/// Matter one's plan with siblings `sib1`, `sib2`, and matter two registered at `other/m2`
/// (its state folder at `other/m2.state`) and not fenced yet, so the plan fences it.
fn plan_with_neighbour(w: &World) -> AdmittedPlan {
    w.dir.dir("other/m2.state");
    let policy = w.full_policy();
    let folders = FakeFolders {
        listing: ["sib1", "sib2"].iter().map(|s| w.canonical(s)).collect(),
        ..FakeFolders::default()
    };
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let mut neighbours = registered(&[w.at("other/m2")]);
    for matter in &mut neighbours.registered {
        matter.fenced = false;
    }
    let admitted = admit_with(&policy, &w.at("data"), &neighbours, ok, &folders).unwrap();
    plan(&admitted, &sids(), matter_one()).unwrap()
}

/// `plan`'s grants and a files-only read deny for the group on `elsewhere`, as the gate plans
/// on the matter's parent when nothing it must leave alone is there.
fn with_parent_files(w: &World, plan: &AdmittedPlan) -> AdmittedPlan {
    let mut grants = plan.grants().to_vec();
    grants.push(Grant {
        path: w.canonical("elsewhere"),
        sid: sid(GROUP_SID),
        rights: Rights::DenyRead,
        inherit: Inherit::FilesInFolder,
        kind: PathKind::ParentFiles,
    });
    AdmittedPlan::from_grants(matter_one(), grants)
}

/// The first path in `plan` with a grant of `kind` (and ending in `name`, when given).
fn path_of(plan: &AdmittedPlan, kind: PathKind, name: Option<&str>) -> PathBuf {
    plan.grants()
        .iter()
        .find(|g| g.kind() == kind && name.is_none_or(|n| g.path().ends_with(n)))
        .map_or_else(|| panic!("no {kind} grant"), |g| g.path().to_owned())
}

/// The refusal's kind, failure and path.
fn refusal(result: Result<Applied, Error>) -> (PathKind, ApplyFailure, Option<PathBuf>) {
    match result {
        Err(Error::AclApply {
            path_kind,
            failure,
            at,
        }) => (path_kind, failure, at),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

const ROOT: ApplyFailure = ApplyFailure::PoolOwned { owned: Owned::Root };

/// Each kind of root, the name its path ends in (when the kind has several), and the pool SID
/// that owns it in the test.
const CASES: [(PathKind, Option<&str>, &str); 6] = [
    (PathKind::Matter, None, USER_SID),
    (PathKind::State, None, OTHER_POOL_SIDS[0]),
    (PathKind::Notes, None, GROUP_SID),
    // A registered matter's fence is planned on its folders under their own kinds.
    (PathKind::Matter, Some("m2"), OTHER_POOL_SIDS[1]),
    (PathKind::Sibling, Some("sib1"), USER_SID),
    (PathKind::ParentFiles, None, USER_SID),
];

/// `w`'s plan with the parent's grant when `kind` is the parent's, and `kind`'s path in it.
fn case_plan(w: &World, kind: PathKind, name: Option<&str>) -> (AdmittedPlan, PathBuf) {
    let mut plan = plan_with_neighbour(w);
    if kind == PathKind::ParentFiles {
        plan = with_parent_files(w, &plan);
    }
    let path = path_of(&plan, kind, name);
    (plan, path)
}

#[test]
fn each_root_a_pool_user_owns_is_handed_to_the_lawyer_and_the_launch_goes_on() {
    for (i, (kind, name, owner)) in CASES.into_iter().enumerate() {
        let w = world(&format!("root-owners-handed-{i}"), &[]);
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        let (mut plan, path) = case_plan(&w, kind, name);
        acl.owners.insert(path.clone(), owner.to_owned());

        apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();

        assert_eq!(acl.owners[&path], LAWYER, "{kind}");
        assert!(ledger.entries().all(|e| e.state == EntryState::Applied));
        assert!(acl.aces.iter().any(|(p, ..)| *p == path), "{kind}: written");
    }
}

#[test]
fn each_root_whose_hand_back_fails_refuses_before_anything_is_recorded() {
    for (i, (kind, name, owner)) in CASES.into_iter().enumerate() {
        let w = world(&format!("root-owners-kept-{i}"), &[]);
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        let (mut plan, path) = case_plan(&w, kind, name);
        acl.owners.insert(path.clone(), owner.to_owned());
        acl.keeps_owner.insert(path.clone());
        let before = ledger_bytes(&w);

        let result = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w));

        assert_eq!(refusal(result), (kind, ROOT, Some(path.clone())), "{kind}");
        assert_eq!(acl.owners[&path], owner, "{kind}");
        assert_eq!(ledger_bytes(&w), before, "{kind}: nothing recorded");
        assert_eq!(ledger.entries().count(), 0, "{kind}");
        assert!(acl.aces.is_empty(), "{kind}: nothing written");
    }
}

#[test]
fn the_cause_names_no_path_and_no_sid() {
    let shown = ROOT.to_string();
    assert!(shown.contains("sandbox user owns"), "{shown}");
    assert!(!shown.contains('\\') && !shown.contains("S-1-"), "{shown}");
}

#[test]
fn roots_the_lawyer_or_another_user_owns_are_left_as_they_are() {
    let w = world("root-owners-lawyer", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = plan_with_neighbour(&w);
    let matter = path_of(&plan, PathKind::Matter, None);
    let colleague = "S-1-5-21-9-9-9-1500";
    acl.owners.insert(matter.clone(), colleague.to_owned());
    acl.keeps_owner.insert(matter.clone());

    let applied = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();

    assert!(applied.written > 0);
    assert_eq!(acl.owners[&matter], colleague, "not a pool user's: left");
}

/// Main's hand-back of a read-only path covers the state folder's settings too.
#[test]
fn a_pool_owned_settings_file_in_the_state_folder_is_handed_back_before_its_deny() {
    let w = world("root-owners-settings", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    w.dir.file(&format!("{STATE}/settings.json"), "{}");
    let mut plan = plan_with_neighbour(&w);
    let settings = path_of(&plan, PathKind::ReadOnlyInState, Some("settings.json"));
    acl.owners
        .insert(settings.clone(), OTHER_POOL_SIDS[0].to_owned());

    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();

    assert_eq!(acl.owners[&settings], LAWYER);
    assert!(has(&acl, &settings, USER_SID, Rights::DenyWrite));
}

/// Matter two at `other/m2`, assigned the first other pool user.
fn matter_two_at(w: &World) -> RegisteredMatter {
    w.dir.dir("other/m2.state");
    RegisteredMatter {
        id: matter_two(),
        folders: MatterFolders {
            root: w.at("other/m2"),
            state: w.at("other/m2.state"),
            notes: None,
        },
        pool_user: sid(OTHER_POOL_SIDS[0]),
        fenced: true,
    }
}

/// Growing the pool fences each registered matter through `apply`: a registered root its own
/// assigned user owns (a SID the fence-only plan names in no grant) is handed back too, and
/// refuses only when that fails.
#[test]
fn growing_the_pool_hands_back_a_registered_root_its_own_user_owns() {
    for keeps in [false, true] {
        let w = world(&format!("root-owners-grow-{keeps}"), &[]);
        let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
        let registered = [matter_two_at(&w)];
        let pool = [sid(USER_SID), sid(OTHER_POOL_SIDS[0])];
        let roots = [w.at("other/m2"), w.canonical("other/m2")];
        for root in &roots {
            acl.owners
                .insert(root.clone(), OTHER_POOL_SIDS[0].to_owned());
            if keeps {
                acl.keeps_owner.insert(root.clone());
            }
        }

        let result = fence_registered(&mut ledger, &registered, &pool, &mut acl, &mut around(&w));

        if keeps {
            let (_, failure, at) = refusal(result);
            assert_eq!(failure, ROOT);
            assert!(at.is_some_and(|at| at.ends_with("other/m2")));
            assert!(acl.aces.is_empty(), "nothing written");
        } else {
            assert!(result.unwrap().written > 0);
            assert!(roots.iter().any(|root| acl.owners[root] == LAWYER));
        }
    }
}
