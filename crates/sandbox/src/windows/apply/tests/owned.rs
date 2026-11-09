//! The folders Clauseo owns for a launch (S15 owned roots): each gets its protected DACL after
//! the roots' hand-back and before anything is recorded, and a refusal records nothing.

use super::fake::LAWYER;
use super::*;
use crate::windows::acl_plan::{OwnedKind, OwnedRoot, Subjects};
use crate::windows::writer::Strict;

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
    assert_eq!(roots.len(), 9, "{roots:?}");
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
            // `agents`: the data walk's, strictly.
            OwnedKind::Agents | OwnedKind::State | OwnedKind::Notes => {
                assert_eq!(walked, [true], "{root:?}");
            }
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

/// The review's L1: each owned folder protected just now gets an owner pass, so what a pool user
/// made there first goes to the lawyer: every pool user's item in the session's run folder (the
/// temp folder inside it is covered), another matter's pool user's item in the matter's folder
/// (the state and notes folders inside it are covered), never the matter's own agent's.
#[test]
fn every_owned_folder_protected_now_gets_an_owner_pass() {
    let w = world("owned-passes", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    acl.protect_gives = Some(Added::Written);
    let mut plan = session_plan(&w);
    let (run, state) = (w.canonical(RUN), w.canonical(STATE));
    let matter_folder = state.parent().unwrap().to_owned();
    let planted = run.join("work").join("planted.json");
    let theirs = state.join("AppData").join("theirs.json");
    let mine = state.join("AppData").join("mine.json");
    acl.owners.insert(planted.clone(), USER_SID.to_owned());
    acl.owners
        .insert(theirs.clone(), OTHER_POOL_SIDS[0].to_owned());
    acl.owners.insert(mine.clone(), USER_SID.to_owned());
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    for root in [&matter_folder, &run] {
        assert!(
            acl.handed_trees.contains(root),
            "{root:?}: {:?}",
            acl.handed_trees
        );
    }
    for inner in [run.join("tmp"), state] {
        assert!(!acl.handed_trees.contains(&inner), "{inner:?} is covered");
    }
    let owner = |p: &PathBuf| acl.owners.get(p).cloned().unwrap();
    assert_eq!(owner(&planted), LAWYER, "the run folder's: anyone's");
    assert_eq!(owner(&theirs), LAWYER, "another matter's agent's");
    assert_eq!(owner(&mine), USER_SID, "the matter's own agent's stays");
}

/// The data walks of `acl` for `data`: of the data folder itself or a folder directly in it.
fn data_walks(acl: &FakeAcl, data: &Path) -> Vec<(PathBuf, Reach, bool)> {
    let below = |p: &Path| p.parent() == Some(data) || p == data;
    acl.walks
        .iter()
        .filter(|(p, _, _)| below(p))
        .cloned()
        .collect()
}

/// The review's L2: the data folder's contents are walked once (its items worked out from it,
/// the host's folders plainly, `agents` strictly), never the matters' and sessions' folders or
/// the sandbox records; a host folder's walk that fails is logged, the launch goes on (N1), and
/// the next launch walks again until one passes.
#[test]
fn the_data_folders_contents_are_walked_until_one_walk_passes() {
    let w = world("owned-data-walk", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let data = w.canonical("data");
    let updates = data.join("updates");
    acl.fail_plain = Some(updates.clone());
    apply(
        &mut ledger,
        &mut session_plan(&w),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let want = vec![
        (data.join("agents"), Reach::Tree, true),
        (data.clone(), Reach::Children, false),
        (updates, Reach::Tree, false),
    ];
    assert_eq!(data_walks(&acl, &data), want);
    let mark = data.join("sandbox").join("owned-walked");
    assert!(!mark.exists(), "a failed walk leaves no record");
    acl.walks.clear();
    acl.fail_plain = None;
    apply(
        &mut ledger,
        &mut session_plan(&w),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_eq!(data_walks(&acl, &data), want, "walked again");
    assert!(mark.exists());
    acl.walks.clear();
    apply(
        &mut ledger,
        &mut session_plan(&w),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert_eq!(data_walks(&acl, &data), [], "once passed, not again");
}

/// N1: a failed strict walk of `agents` (the sign-ins) refuses the launch with the repair
/// message before any grant is written or any entry recorded, pending or not, and no record of
/// a data walk is written, so the next launch walks again.
#[test]
fn a_failed_walk_of_the_sign_ins_refuses_the_launch() {
    let w = world("owned-sign-in", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let agents = w.canonical("data/agents");
    acl.fail_walk = Some((agents.clone(), ApplyFailure::ChildShut));
    acl.fail_once = true;
    let error = apply(
        &mut ledger,
        &mut session_plan(&w),
        &mut acl,
        &mut around(&w),
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            Error::SignInUnsecured {
                failure: ApplyFailure::ChildShut
            }
        ),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("Restart the computer"),
        "{error}"
    );
    assert_eq!(
        ledger.entries().count(),
        0,
        "nothing recorded, not even pending"
    );
    assert!(acl.aces.is_empty(), "no grant written: {:?}", acl.aces);
    let reopened = Ledger::open(ledger_path(&w.at("data")), &trusted(), &reserved()).unwrap();
    assert_eq!(reopened.entries().count(), 0, "nothing on disk either");
    assert!(!w.at("data/sandbox/owned-walked").exists());
    acl.walks.clear();
    apply(
        &mut ledger,
        &mut session_plan(&w),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    assert!(
        acl.walks.contains(&(agents, Reach::Tree, true)),
        "walked again"
    );
}

/// N2: with the data folder skipped (it holds the profile), `agents` is still protected and
/// walked strictly; the data folder's own items and the host's folders are left as they are.
#[test]
fn the_sign_ins_are_walked_even_when_the_data_folder_is_skipped() {
    let w = world("owned-skipped", &[]);
    w.dir.dir("data/home");
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let policy = w.builder().home(w.at("data/home")).build().unwrap();
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let none = FakeFolders::default();
    let admitted = admit_with(&policy, &w.at("data"), &Neighbours::default(), ok, &none).unwrap();
    let mut plan = plan(&admitted, &sids(), matter_one()).unwrap();
    let kinds: Vec<OwnedKind> = plan.owned_roots().iter().map(OwnedRoot::kind).collect();
    assert!(!kinds.contains(&OwnedKind::DataRoot), "{kinds:?}");
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    let data = w.canonical("data");
    let agents = data.join("agents");
    assert_eq!(
        data_walks(&acl, &data),
        [(agents.clone(), Reach::Tree, true)]
    );
    assert!(acl.protected.iter().any(|(p, _)| *p == agents));
}

/// N6: a run folder a pool user made with the protected DACL it would get anyway: the hand-back
/// takes it, protect finds nothing to write, and its contents still get their owner pass.
#[test]
fn a_premade_folder_whose_dacl_matches_still_gets_its_owner_pass() {
    let w = world("owned-premade", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let mut plan = session_plan(&w);
    let run = w.canonical(RUN);
    let planted = run.join("planted.json");
    acl.owners
        .insert(run.clone(), OTHER_POOL_SIDS[0].to_owned());
    acl.owners
        .insert(planted.clone(), OTHER_POOL_SIDS[0].to_owned());
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    assert!(acl.handed_trees.contains(&run), "{:?}", acl.handed_trees);
    assert_eq!(acl.owners.get(&planted).map(String::as_str), Some(LAWYER));
}

/// N7: the strict walk of `agents` watches no SID and hands nothing back by matter, so it
/// strips only broad allows from what a pool user owns: another live session's recorded allow
/// in the Claude store stays.
#[test]
fn the_sign_ins_walk_watches_no_pool_sid() {
    let w = world("owned-sign-in-strict", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut session_plan(&w),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let agents = w.canonical("data/agents");
    let strict: Vec<&Strict> = acl
        .stricts
        .iter()
        .filter(|(p, _)| *p == agents)
        .map(|(_, s)| s)
        .collect();
    assert_eq!(strict.len(), 1);
    assert_eq!(strict[0].watched, Vec::<Sid>::new());
    assert_eq!(strict[0].assigned, Vec::<Sid>::new());
}

/// The review's L3: a protect keeps every allow the ledger records on the folder for a live
/// launch, with its recorded mask, as well as the plan's own: here a later plan whose pool user
/// is another one still keeps the first one's recorded `Modify` on the state folder.
#[test]
fn a_protect_keeps_the_allows_the_ledger_records_there() {
    let w = world("owned-kept", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    apply(
        &mut ledger,
        &mut session_plan(&w),
        &mut acl,
        &mut around(&w),
    )
    .unwrap();
    let state = w.canonical(STATE);
    let next = Subjects {
        pool_user: sid("S-1-5-21-1111111111-2222222222-3333333333-1004"),
        ..sids()
    };
    let policy = w.builder().notes(w.at(NOTES)).build().unwrap();
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let none = FakeFolders::default();
    let admitted = admit_with(&policy, &w.at("data"), &Neighbours::default(), ok, &none).unwrap();
    let mut later = plan(&admitted, &next, matter_one()).unwrap();
    acl.kept.clear();
    apply(&mut ledger, &mut later, &mut acl, &mut around(&w)).unwrap();
    let on_state: Vec<(String, Rights, u32)> = acl
        .kept
        .iter()
        .filter(|(p, _)| *p == state)
        .flat_map(|(_, keep)| keep.iter())
        .map(|g| (g.sid().as_str().to_owned(), g.rights(), g.rights().mask()))
        .collect();
    for who in [USER_SID, "S-1-5-21-1111111111-2222222222-3333333333-1004"] {
        let want = (who.to_owned(), Rights::Modify, 0x0013_01bf);
        assert!(on_state.contains(&want), "{who}: {on_state:?}");
    }
    assert!(
        on_state.iter().all(|(_, r, _)| r.is_allow()),
        "{on_state:?}"
    );
}
