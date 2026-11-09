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
            format!("D:PAI{fence}(A;OICI;FA;;;S-1-5-18)(A;OICI;FA;;;lawyer){own}"),
        ),
        (
            OwnedKind::Notes,
            format!("{m}/notes"),
            format!("D:PAI{fence}(A;OICI;FA;;;S-1-5-18)(A;OICI;FA;;;lawyer){own}"),
        ),
        (
            OwnedKind::TempFolder,
            "<root>/data/run/s1/tmp".to_owned(),
            format!("{bare}{own}"),
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
    for root in plan.owned_roots() {
        let own = matches!(
            root.kind(),
            OwnedKind::State | OwnedKind::Notes | OwnedKind::TempFolder
        );
        let want: Vec<&str> = if own {
            vec![USER_SID, MATTER_SID]
        } else {
            Vec::new()
        };
        let got: Vec<&str> = root.allow().iter().map(Sid::as_str).collect();
        assert_eq!(got, want, "{:?}", root.kind());
    }
    let tmp = plan
        .owned_roots()
        .iter()
        .find(|r| r.kind() == OwnedKind::TempFolder)
        .unwrap();
    let sddl = owned(&w, &plan)
        .into_iter()
        .find(|(kind, _, _)| *kind == OwnedKind::TempFolder)
        .unwrap()
        .2;
    assert!(
        sddl.contains("(A;OICI;0x1301bf;;;matter)"),
        "{sddl} {tmp:?}"
    );
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
