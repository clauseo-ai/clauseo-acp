//! The folders Clauseo owns for a launch (S15 owned roots): each gets its protected DACL after
//! the roots' hand-back and before anything is recorded, and a refusal records nothing.

use super::fake::LAWYER;
use super::*;
use crate::windows::acl_plan::OwnedKind;

/// The session's run folder in `w`, relative to it.
const RUN: &str = "data/run/s1";

/// Matter one's plan with notes and the session's temp folder `data/run/s1/tmp`.
fn session_plan(w: &World) -> AdmittedPlan {
    w.dir.dir(&format!("{RUN}/tmp"));
    let policy = w
        .builder()
        .notes(w.at(NOTES))
        .tmpdir(w.at(&format!("{RUN}/tmp")))
        .build()
        .unwrap();
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let none = FakeFolders::default();
    let admitted = admit_with(&policy, &w.at("data"), &Neighbours::default(), ok, &none).unwrap();
    plan(&admitted, &sids(), matter_one()).unwrap()
}

#[test]
fn every_owned_root_is_protected_keeping_every_pool_sids_denies() {
    let w = world("owned-each", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = session_plan(&w);
    let roots: Vec<PathBuf> = plan
        .owned_roots()
        .iter()
        .map(|r| r.path().to_owned())
        .collect();
    assert_eq!(roots.len(), 8, "{roots:?}");
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    let protected: Vec<PathBuf> = acl.protected.iter().map(|(p, _)| p.clone()).collect();
    assert_eq!(protected, roots);
    // No sibling here, so no group: the plan's own pool SIDs.
    let mut want: Vec<Sid> = vec![sid(USER_SID)];
    want.extend(OTHER_POOL_SIDS.iter().map(|s| sid(s)));
    want.sort();
    for (path, pool) in &acl.protected {
        assert_eq!(pool, &want, "{path:?}");
    }
    // Nothing protected is walked when nothing changed (`AlreadyThere`) but the own trees.
    let run = w.canonical(RUN);
    assert!(acl.walks.iter().all(|(root, _, _)| *root != run));
}

#[test]
fn a_root_that_cant_be_protected_refuses_the_launch_before_anything_is_recorded() {
    let w = world("owned-refused", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = session_plan(&w);
    let run = w.canonical(RUN);
    acl.fail_protect = Some(run.clone());
    let error = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap_err();
    let Error::AclApply {
        path_kind: PathKind::TempFolder,
        failure: ApplyFailure::Io(io::ErrorKind::PermissionDenied),
        at: Some(at),
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(at, &run);
    assert_eq!(ledger.entries().count(), 0);
    assert!(acl.aces.is_empty(), "{:?}", acl.aces);
}

#[test]
fn a_protected_root_has_its_tree_walked_and_the_shared_folders_dont() {
    let w = world("owned-walked", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.protect_gives = Some(Added::Written);
    let mut plan = session_plan(&w);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    for root in plan.owned_roots() {
        let walked: Vec<bool> = acl
            .walks
            .iter()
            .filter(|(r, reach, _)| r == root.path() && *reach == Reach::Tree)
            .map(|(_, _, strict)| *strict)
            .collect();
        match root.kind() {
            OwnedKind::DataRoot | OwnedKind::DataFolder => assert_eq!(walked, [false; 0]),
            OwnedKind::State | OwnedKind::Notes => assert_eq!(walked, [true], "{root:?}"),
            OwnedKind::MatterFolder | OwnedKind::RunFolder => {
                assert_eq!(walked, [false], "{root:?}");
            }
            // The run folder's walk takes it in.
            OwnedKind::TempFolder => assert_eq!(walked, [false; 0], "{root:?}"),
        }
    }
}

#[test]
fn a_run_folder_an_agent_made_goes_to_the_lawyer_before_it_is_protected() {
    let w = world("owned-pool-made", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = session_plan(&w);
    let run = w.canonical(RUN);
    acl.owners
        .insert(run.clone(), OTHER_POOL_SIDS[0].to_owned());
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    assert_eq!(acl.owners.get(&run).map(String::as_str), Some(LAWYER));
    assert!(acl.protected.iter().any(|(p, _)| *p == run));
}
