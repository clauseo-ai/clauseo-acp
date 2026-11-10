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
            OwnedKind::Agents,
            format!("<root>/data/{}", targets::AGENTS_DIR),
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
    // N2: the sign-ins' folder is owned whatever the data folder is.
    let at_volume = kinds(&elsewhere, volume);
    assert!(!at_volume.contains(&OwnedKind::DataRoot), "{at_volume:?}");
    assert!(at_volume.contains(&OwnedKind::Agents), "{at_volume:?}");
    // The profile inside the data folder, or the data folder the profile itself.
    let data = w.canonical("data");
    for home in [data.join("home"), data.clone()] {
        let policy = Policy::builder(w.at("matter"), w.at(STATE))
            .home(home.clone())
            .build()
            .unwrap();
        let got = kinds(&policy, &data);
        assert!(!got.contains(&OwnedKind::DataRoot), "{home:?}: {got:?}");
        assert!(got.contains(&OwnedKind::Agents), "{home:?}: {got:?}");
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
    assert_eq!(kinds, [OwnedKind::DataRoot, OwnedKind::Agents]);
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

/// How the owned roots cover an allow of some kind inside the data folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Covered {
    /// By its chain of owned folders: the data folder, the shared folder named so in
    /// `targets.rs`, the matter's or session's folder (`middle`), and its own (`own`).
    Chain {
        shared: &'static str,
        middle: OwnedKind,
        own: OwnedKind,
    },
    /// By `<data>\agents`, an owned root of its own (`OwnedKind::Agents`).
    Agents,
}

/// How an allow of `kind` the plan grants inside the data folder is covered. Exhaustive, with no
/// catch-all: a new kind of path is a compile error here until someone says how it is covered.
fn coverage(kind: PathKind) -> Covered {
    let matter = |own| Covered::Chain {
        shared: targets::AGENT_STATE_DIR,
        middle: OwnedKind::MatterFolder,
        own,
    };
    match kind {
        PathKind::State | PathKind::CredentialLink => matter(OwnedKind::State),
        PathKind::Notes => matter(OwnedKind::Notes),
        PathKind::TempFolder => Covered::Chain {
            shared: targets::RUN_DIR,
            middle: OwnedKind::RunFolder,
            own: OwnedKind::TempFolder,
        },
        // Clauseo's Claude sign-in (`<data>\agents\claude-acp\home`) and its files, shared by
        // every matter: no per-matter chain, but inside `agents`, owned with the data folder's
        // entries even when the data folder is skipped, and walked strictly once. A read or run
        // path the plan may grant in the data folder is only ever inside one agent's install
        // under `agents` (`check_target`), so the same root covers it.
        PathKind::ClaudeStore
        | PathKind::ClaudeStoreFile
        | PathKind::Runtime
        | PathKind::Read
        | PathKind::AllowedFolder => Covered::Agents,
        PathKind::Matter => panic!("the matter folder is never inside the data folder"),
        PathKind::ReadOnlyInMatter
        | PathKind::ReadOnlyInState
        | PathKind::OtherMatter
        | PathKind::Sibling
        | PathKind::ParentFiles
        | PathKind::SharedWrite => panic!("{kind:?} is a deny, never an allow"),
    }
}

/// Cross-check against the layout (host #823 moves it): for every allow the plan grants inside
/// the data folder in the test worlds, `owned_roots` covers it as [`coverage`] says: its chain
/// of owned folders with the right kinds and `targets.rs`'s shared folder names, or the
/// `agents` root. A layout change in either place without the other fails here.
#[test]
fn owned_roots_cover_every_allow_the_plan_grants_in_the_data_folder() {
    let w = World::new("plan-owned-chain");
    w.dir.dir("data/run/s1/tmp");
    w.dir.dir("data/agents/claude-acp/bin");
    let session = w
        .builder()
        .notes(w.at(NOTES))
        .tmpdir(w.at("data/run/s1/tmp"))
        .runtime(w.at("data/agents/claude-acp/bin"))
        .build()
        .unwrap();
    let data = w.canonical("data");
    let agents = data.join(targets::AGENTS_DIR);
    let mut checked: Vec<PathKind> = Vec::new();
    for policy in [w.full_policy(), session] {
        let admitted = admitted(&w, &policy, &[], &FakeFolders::default());
        let plan = plan(&admitted, &sids(), matter_one()).unwrap();
        let kind_at = |folder: &Path| {
            plan.owned_roots()
                .iter()
                .find(|r| r.path() == folder)
                .map(OwnedRoot::kind)
        };
        let in_data = |g: &&Grant| g.rights.is_allow() && in_folder(&g.path, &data);
        for grant in plan.grants().iter().filter(in_data) {
            let at = w.relative(&grant.path);
            match coverage(grant.kind) {
                Covered::Chain {
                    shared,
                    middle,
                    own,
                } => {
                    let below: Vec<&Path> = grant
                        .path
                        .ancestors()
                        .take_while(|a| in_folder(a, &data))
                        .collect();
                    let chain = [OwnedKind::DataRoot, OwnedKind::DataFolder, middle, own];
                    for (depth, folder) in below.iter().rev().enumerate() {
                        let want = chain.get(depth).copied();
                        assert_eq!(kind_at(folder), want, "{:?} {at}", grant.kind);
                    }
                    let name = below.iter().rev().nth(1).and_then(|f| f.file_name());
                    assert_eq!(name, Some(std::ffi::OsStr::new(shared)), "{at}");
                }
                Covered::Agents => {
                    assert!(in_folder(&grant.path, &agents), "{:?} {at}", grant.kind);
                    assert_eq!(kind_at(&agents), Some(OwnedKind::Agents), "{at}");
                    assert_eq!(kind_at(&data), Some(OwnedKind::DataRoot), "{at}");
                }
            }
            checked.push(grant.kind);
        }
    }
    let want = [
        PathKind::State,
        PathKind::Notes,
        PathKind::CredentialLink,
        PathKind::TempFolder,
        PathKind::ClaudeStore,
        PathKind::ClaudeStoreFile,
        PathKind::Runtime,
    ];
    for kind in want {
        assert!(checked.contains(&kind), "no {kind:?} allow was checked");
    }
}
