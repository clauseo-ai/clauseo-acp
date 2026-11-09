//! The folders Clauseo owns for a launch and the protected descriptor each gets (S15 owned
//! roots): the SDDL is pinned, so a change to who may reach a session's state shows here.

use super::*;
use crate::windows::acl_plan::{OwnedKind, owned_sddl};

/// The desktop user in these tests (the reserved stand-in for the lawyer's account).
const LAWYER: &str = "S-1-5-21-7-8-9-1001";

/// The SIDs no owned root may name, inherited or not: `Users`, `Authenticated Users`,
/// `Everyone`, `CREATOR OWNER`, and their SDDL aliases.
const BROAD: [&str; 8] = [
    "S-1-5-32-545",
    "S-1-5-11",
    "S-1-1-0",
    "S-1-3-0",
    ";BU)",
    ";AU)",
    ";WD)",
    ";CO)",
];

/// Matter one's plan with notes and the session's temp folder `data/run/s1/tmp`.
fn session_plan(w: &World, subjects: &Subjects) -> AdmittedPlan {
    w.dir.dir("data/run/s1/tmp");
    let policy = w
        .builder()
        .notes(w.at(NOTES))
        .tmpdir(w.at("data/run/s1/tmp"))
        .build()
        .unwrap();
    let admitted = admitted(w, &policy, &[], &FakeFolders::default());
    plan(&admitted, subjects, matter_one()).unwrap()
}

/// Each owned root, as (kind, path relative to the temp folder, SDDL with short SID names).
fn owned(w: &World, plan: &AdmittedPlan) -> Vec<(OwnedKind, String, String)> {
    let lawyer = Sid::parse(LAWYER).unwrap();
    plan.owned_roots()
        .iter()
        .map(|root| {
            let sddl = owned_sddl(plan, root, &lawyer)
                .replace(USER_SID, "pool")
                .replace(OTHER_POOL_SIDS[0], "other1")
                .replace(OTHER_POOL_SIDS[1], "other2")
                .replace(MATTER_SID, "matter")
                .replace(LAWYER, "lawyer");
            (root.kind(), w.relative(root.path()), sddl)
        })
        .collect()
}

#[test]
fn a_sessions_owned_roots_get_protected_dacls_naming_no_broad_group() {
    let w = World::new("plan-owned");
    let plan = session_plan(&w, &sids());
    let m = "<root>/data/agent-state/01890a5d-ac96-774b-bcce-b302099a8057";
    let admins = "D:PAI(A;OICI;FA;;;S-1-5-18)(A;OICI;FA;;;S-1-5-32-544)(A;OICI;FA;;;lawyer)";
    let bare = "D:PAI(A;OICI;FA;;;S-1-5-18)(A;OICI;FA;;;lawyer)";
    let fence = "(D;OICI;0xf01ff;;;other1)(D;OICI;0xf01ff;;;other2)";
    let ow = "(A;OICIIO;0x1301bf;;;S-1-3-4)";
    let own = "(A;OICI;0x1301bf;;;pool)S:(ML;OICI;NW;;;LW)";
    let want = [
        (
            OwnedKind::DataRoot,
            "<root>/data".to_owned(),
            admins.to_owned(),
        ),
        (
            OwnedKind::DataFolder,
            "<root>/data/agent-state".to_owned(),
            admins.to_owned(),
        ),
        (
            OwnedKind::DataFolder,
            "<root>/data/run".to_owned(),
            admins.to_owned(),
        ),
        (OwnedKind::MatterFolder, m.to_owned(), bare.to_owned()),
        (
            OwnedKind::RunFolder,
            "<root>/data/run/s1".to_owned(),
            bare.to_owned(),
        ),
        (
            OwnedKind::State,
            format!("{m}/claude"),
            format!("D:PAI{fence}(A;OICI;FA;;;S-1-5-18)(A;OICI;FA;;;lawyer){ow}{own}"),
        ),
        (
            OwnedKind::Notes,
            format!("{m}/notes"),
            format!("D:PAI{fence}(A;OICI;FA;;;S-1-5-18)(A;OICI;FA;;;lawyer){ow}{own}"),
        ),
        (
            OwnedKind::TempFolder,
            "<root>/data/run/s1/tmp".to_owned(),
            format!("{bare}{ow}{own}"),
        ),
    ];
    let got = owned(&w, &plan);
    assert_eq!(got, want);
    for (_, path, sddl) in &got {
        assert!(sddl.starts_with("D:PAI"), "{path}: protected");
        for broad in BROAD {
            assert!(!sddl.contains(broad), "{path}: {broad} in {sddl}");
        }
    }
}

#[test]
fn the_matter_sid_keeps_its_allow_on_the_launch_folders_only() {
    let w = World::new("plan-owned-matter-sid");
    let subjects = Subjects {
        matter: Some(Sid::parse(MATTER_SID).unwrap()),
        ..sids()
    };
    let plan = session_plan(&w, &subjects);
    for (kind, path, sddl) in owned(&w, &plan) {
        let own = matches!(
            kind,
            OwnedKind::State | OwnedKind::Notes | OwnedKind::TempFolder
        );
        let has = sddl.contains("(A;OICI;0x1301bf;;;matter)");
        assert_eq!(has, own, "{path}: {sddl}");
        assert_eq!(sddl.contains(";;;S-1-3-4)"), own, "{path}: OWNER RIGHTS");
    }
}

/// The data folder itself is owned only when it has a name of its own and doesn't hold the
/// profile: a volume's root and a profile folder keep the DACLs Windows and the lawyer gave
/// them, on purpose (the review's L2).
#[test]
fn a_data_folder_that_is_a_volumes_root_or_holds_the_profile_is_skipped_on_purpose() {
    let w = World::new("plan-owned-skip");
    let volume = if cfg!(windows) {
        Path::new(r"C:\")
    } else {
        Path::new("/")
    };
    let elsewhere = Policy::builder(w.at("matter"), w.at(STATE))
        .home(w.at("home"))
        .build()
        .unwrap();
    let kinds = |policy: &Policy, data: &Path| -> Vec<OwnedKind> {
        crate::windows::acl_plan::owned::owned_roots(policy, data)
            .iter()
            .map(OwnedRoot::kind)
            .collect()
    };
    assert!(!kinds(&elsewhere, volume).contains(&OwnedKind::DataRoot));
    // The profile inside the data folder, or the data folder the profile itself.
    let data = w.canonical("data");
    for home in [data.join("home"), data.clone()] {
        let policy = Policy::builder(w.at("matter"), w.at(STATE))
            .home(home.clone())
            .build()
            .unwrap();
        let got = kinds(&policy, &data);
        assert!(!got.contains(&OwnedKind::DataRoot), "{home:?}: {got:?}");
        assert!(
            got.contains(&OwnedKind::State),
            "{home:?}: the state folder still is"
        );
    }
    assert!(kinds(&elsewhere, &data).contains(&OwnedKind::DataRoot));
}

#[test]
fn a_state_folder_outside_the_data_folder_is_not_owned() {
    // Only folders Clauseo makes in its data folder are owned: the lawyer's are a separate
    // decision, and a state folder elsewhere is left as it is.
    let w = World::new("plan-owned-outside");
    w.dir.dir("elsewhere/state");
    let policy = Policy::builder(w.at("matter"), w.at("elsewhere/state"))
        .home(w.at("home"))
        .hidden(w.at("data"))
        .build()
        .unwrap();
    let admitted = admitted(&w, &policy, &[], &FakeFolders::default());
    let plan = plan(&admitted, &sids(), matter_one()).unwrap();
    let kinds: Vec<OwnedKind> = plan.owned_roots().iter().map(OwnedRoot::kind).collect();
    assert_eq!(kinds, [OwnedKind::DataRoot]);
    let matter = w.canonical("matter");
    assert!(plan.owned_roots().iter().all(|r| r.path() != matter));
}

#[test]
fn a_fence_only_plan_owns_nothing() {
    let w = World::new("plan-owned-fence");
    let grants = grants_for(&w, &w.builder().build().unwrap(), &sids());
    let plan = AdmittedPlan::from_grants(matter_one(), grants);
    assert_eq!(plan.owned_roots(), []);
}

/// Each kind of path on an owned chain, and the owned kind of the folder it is or is in.
const ON_CHAIN: [(PathKind, OwnedKind); 4] = [
    (PathKind::State, OwnedKind::State),
    (PathKind::Notes, OwnedKind::Notes),
    (PathKind::CredentialLink, OwnedKind::State),
    (PathKind::TempFolder, OwnedKind::TempFolder),
];

/// The owned kind a folder `depth` levels below the data folder must have, on the chain down to
/// a path of `kind`: the layout `targets.rs` accepts (`<data>\<AGENT_STATE_DIR>\<matter>\<x>`,
/// `<data>\<RUN_DIR>\<session>\tmp`). `None` below the owned folder (the credential link).
fn chain_kind(kind: PathKind, depth: usize) -> Option<OwnedKind> {
    let (_, own) = ON_CHAIN.into_iter().find(|(k, _)| *k == kind)?;
    let middle = if own == OwnedKind::TempFolder {
        OwnedKind::RunFolder
    } else {
        OwnedKind::MatterFolder
    };
    [OwnedKind::DataRoot, OwnedKind::DataFolder, middle, own]
        .get(depth)
        .copied()
}

/// Cross-check against the layout (host #823 moves it): for each state, notes, credential link
/// and temp path `check_target` accepted in the test worlds, `owned_roots` covers its chain in
/// the data folder with the right kinds, the shared folder named as `targets.rs` names it. A
/// layout change in either place without the other fails here.
#[test]
fn owned_roots_cover_every_accepted_state_notes_and_temp_paths_chain() {
    let w = World::new("plan-owned-chain");
    w.dir.dir("data/run/s1/tmp");
    let session = w
        .builder()
        .notes(w.at(NOTES))
        .tmpdir(w.at("data/run/s1/tmp"))
        .build()
        .unwrap();
    let data = w.canonical("data");
    let kinds = ON_CHAIN.map(|(kind, _)| kind);
    let mut checked: Vec<PathKind> = Vec::new();
    for policy in [w.full_policy(), session] {
        let admitted = admitted(&w, &policy, &[], &FakeFolders::default());
        let plan = plan(&admitted, &sids(), matter_one()).unwrap();
        for grant in plan.grants().iter().filter(|g| kinds.contains(&g.kind)) {
            let below: Vec<&Path> = grant
                .path
                .ancestors()
                .take_while(|a| in_folder(a, &data))
                .collect();
            for (depth, folder) in below.iter().rev().enumerate() {
                let found = plan
                    .owned_roots()
                    .iter()
                    .find(|r| r.path() == *folder)
                    .map(OwnedRoot::kind);
                let want = chain_kind(grant.kind, depth);
                assert_eq!(found, want, "{:?} at {}", grant.kind, w.relative(folder));
            }
            let shared = below.iter().rev().nth(1).and_then(|f| f.file_name());
            let named = if grant.kind == PathKind::TempFolder {
                targets::RUN_DIR
            } else {
                targets::AGENT_STATE_DIR
            };
            assert_eq!(
                shared,
                Some(std::ffi::OsStr::new(named)),
                "{:?}",
                grant.kind
            );
            checked.push(grant.kind);
        }
    }
    for kind in kinds {
        assert!(
            checked.contains(&kind),
            "no accepted {kind:?} path was checked"
        );
    }
}
