use std::fmt::Write as _;
use std::path::Path;

use super::super::admit::{Admitted, FolderRefusal, PathKind, admit_with};
use super::super::test_world::{
    FakeFolders, GROUP_SID, MATTER_SID, NOTES, OTHER_POOL_SIDS, STATE, STORE, USER_SID, World,
    matter_one, registered, sids,
};
use super::*;
use crate::windows::FolderSupport;
use crate::windows::fence::{MatterFolders, Neighbours, RegisteredMatter};
use crate::{Error, Policy};

/// The gate with every folder answering `Ok`, `registered` the other matters and `folders` the
/// listing of the matter's parent.
fn admitted<'a>(
    w: &World,
    policy: &'a Policy,
    roots: &[PathBuf],
    folders: &FakeFolders,
) -> Admitted<'a> {
    admitted_with(w, policy, &registered(roots), folders)
}

/// The gate with every folder answering `Ok` and `neighbours` as given.
fn admitted_with<'a>(
    w: &World,
    policy: &'a Policy,
    neighbours: &Neighbours,
    folders: &FakeFolders,
) -> Admitted<'a> {
    let data = w.at("data");
    admit_with(
        policy,
        &data,
        neighbours,
        |_| Ok(FolderSupport::Ok),
        folders,
    )
    .unwrap()
}

/// The plan's grants for `policy`, with no registered matter and no sibling.
fn grants_for(w: &World, policy: &Policy, subjects: &Subjects) -> Vec<Grant> {
    let admitted = admitted(w, policy, &[], &FakeFolders::default());
    plan(&admitted, subjects, matter_one())
        .unwrap()
        .grants()
        .to_vec()
}

/// A short name for a test SID.
fn who(sid: &Sid) -> &'static str {
    match sid.as_str() {
        USER_SID => "pool",
        MATTER_SID => "matter",
        GROUP_SID => "group",
        LOW_LABEL_SID => "low",
        s if s == OTHER_POOL_SIDS[0] => "other-pool-1",
        s if s == OTHER_POOL_SIDS[1] => "other-pool-2",
        _ => "?",
    }
}

/// The plan as text, one entry a line, paths relative to the temp folder.
fn render(w: &World, grants: &[Grant]) -> String {
    let mut out = String::new();
    for g in grants {
        writeln!(
            out,
            "{:?} {:?} {} 0x{:08x} {:?} {}",
            g.kind,
            g.rights,
            who(&g.sid),
            g.rights.mask(),
            g.inherit,
            w.relative(&g.path)
        )
        .unwrap();
    }
    out
}

#[test]
fn the_full_plan() {
    let w = World::new("plan-full");
    w.dir.dir("Drafts");
    let policy = w.full_policy();
    let folders = FakeFolders {
        listing: vec![w.canonical("Drafts"), w.canonical("matter")],
        ..FakeFolders::default()
    };
    let admitted = admitted(&w, &policy, &[w.canonical("other/m2")], &folders);
    let plan = plan(&admitted, &sids(), matter_one()).unwrap();
    insta::assert_snapshot!(render(&w, plan.grants()));
}

#[test]
fn every_allow_names_the_pool_user_and_no_one_else_by_default() {
    let w = World::new("plan-pool-user");
    let grants = grants_for(&w, &w.full_policy(), &sids());
    let allows: Vec<&Grant> = grants.iter().filter(|g| g.rights.is_allow()).collect();
    assert!(allows.len() >= 10, "{allows:#?}");
    for g in &allows {
        assert_eq!(g.sid.as_str(), USER_SID, "{g:?}");
    }
    assert!(grants.iter().all(|g| g.sid.as_str() != MATTER_SID));
}

#[test]
fn the_matter_sid_is_a_second_subject_of_every_allow_when_asked() {
    let w = World::new("plan-matter-sid");
    let mut subjects = sids();
    subjects.matter = Some(Sid::parse(MATTER_SID).unwrap());
    let grants = grants_for(&w, &w.full_policy(), &subjects);
    let allows: Vec<&Grant> = grants.iter().filter(|g| g.rights.is_allow()).collect();
    let pool = allows.iter().filter(|g| g.sid.as_str() == USER_SID).count();
    let matter: Vec<&&Grant> = allows
        .iter()
        .filter(|g| g.sid.as_str() == MATTER_SID)
        .collect();
    assert_eq!(pool, matter.len());
    for m in matter {
        let twin = allows.iter().any(|g| {
            g.sid.as_str() == USER_SID
                && g.path == m.path
                && g.rights == m.rights
                && g.inherit == m.inherit
        });
        assert!(twin, "no pool-user twin for {m:?}");
    }
    // The matter SID never takes a deny or the label.
    let others = grants.iter().filter(|g| !g.rights.is_allow());
    assert!(others.into_iter().all(|g| g.sid.as_str() != MATTER_SID));
}

#[test]
fn the_matter_state_and_notes_folders_get_the_low_label() {
    let w = World::new("plan-low-label");
    let policy = w.full_policy();
    let grants = grants_for(&w, &policy, &sids());
    let own = [PathKind::Matter, PathKind::State, PathKind::Notes];
    let labels: Vec<&Grant> = grants
        .iter()
        .filter(|g| g.rights.is_label() && own.contains(&g.kind))
        .collect();
    let want = [
        (policy.matter(), PathKind::Matter),
        (policy.state(), PathKind::State),
        (policy.notes().unwrap(), PathKind::Notes),
    ];
    assert_eq!(labels.len(), want.len(), "{labels:#?}");
    for (label, (path, kind)) in labels.iter().zip(want) {
        assert_eq!((label.path.as_path(), label.kind), (path, kind));
        assert_eq!(label.sid.as_str(), LOW_LABEL_SID);
        assert_eq!(label.inherit, Inherit::Tree, "OICI");
        assert_eq!(label.rights.mask(), 0x1, "no-write-up");
        assert!(!label.rights.is_deny() && !label.rights.is_allow());
    }
}

#[test]
fn every_other_pool_user_is_denied_this_matters_own_folders_and_no_one_else_is() {
    let w = World::new("plan-other-pool");
    let policy = w.full_policy();
    let grants = grants_for(&w, &policy, &sids());
    let denies: Vec<&Grant> = grants
        .iter()
        .filter(|g| g.rights == Rights::DenyReadWrite)
        .collect();
    let own = [policy.matter(), policy.state(), policy.notes().unwrap()];
    assert_eq!(
        denies.len(),
        OTHER_POOL_SIDS.len() * own.len(),
        "{denies:#?}"
    );
    for other in OTHER_POOL_SIDS {
        for folder in own {
            let found = denies
                .iter()
                .any(|g| g.sid.as_str() == other && g.path == folder && g.inherit == Inherit::Tree);
            assert!(found, "{other} on {folder:?}");
        }
    }
    for g in &denies {
        assert!(OTHER_POOL_SIDS.contains(&g.sid.as_str()), "{g:?}");
    }
    assert_eq!(
        Rights::DenyReadWrite.mask() & 0x3,
        0x3,
        "read and write data"
    );
    // The pool user itself is never another pool user.
    let mut colliding = sids();
    colliding
        .other_pool_users
        .push(Sid::parse(USER_SID).unwrap());
    let admitted = admitted(&w, &policy, &[], &FakeFolders::default());
    assert!(matches!(
        plan(&admitted, &colliding, matter_one()),
        Err(Error::Policy(_))
    ));
}

#[test]
fn siblings_get_the_groups_deny_read_and_the_matter_and_registered_ones_none() {
    let w = World::new("plan-siblings");
    for rel in ["Drafts", "Precedents", "registered"] {
        w.dir.dir(rel);
    }
    let policy = w.full_policy();
    let folders = FakeFolders {
        listing: ["Drafts", "Precedents", "registered", "matter"]
            .iter()
            .map(|rel| w.canonical(rel))
            .collect(),
        ..FakeFolders::default()
    };
    let admitted = admitted(&w, &policy, &[w.canonical("registered")], &folders);
    let grants = plan(&admitted, &sids(), matter_one()).unwrap();
    let group: Vec<&Grant> = grants
        .grants()
        .iter()
        .filter(|g| g.sid.as_str() == GROUP_SID)
        .collect();
    let paths: Vec<&Path> = group.iter().map(|g| g.path.as_path()).collect();
    assert_eq!(paths, [w.canonical("Drafts"), w.canonical("Precedents")]);
    for g in &group {
        assert_eq!(
            (g.rights, g.inherit, g.kind),
            (Rights::DenyRead, Inherit::Tree, PathKind::Sibling)
        );
    }
    assert_eq!(Rights::DenyRead.mask() & 0x2, 0, "reads only");
    // Nothing for the group on the matter, a registered matter or the parent.
    for path in [
        w.canonical("matter"),
        w.canonical("registered"),
        w.canonical(""),
    ] {
        assert!(!group.iter().any(|g| g.path == path), "{path:?}");
    }
}

#[test]
fn read_only_names_get_deny_write_for_the_pool_user() {
    let w = World::new("plan-read-only");
    let policy = w.full_policy();
    let grants = grants_for(&w, &policy, &sids());
    let denied = |path: &Path| {
        grants
            .iter()
            .find(|g| g.path == path && g.rights == Rights::DenyWrite && g.sid.as_str() == USER_SID)
    };
    // Every rule the policy names, nothing hard-coded here.
    assert!(policy.read_only_in_matter().len() > 8);
    for rule in policy.read_only_in_matter() {
        let grant = denied(rule.path()).unwrap_or_else(|| panic!("no deny on {rule:?}"));
        let want = match rule {
            PathRule::Literal(_) => Inherit::ThisOnly,
            PathRule::Subpath(_) => Inherit::Tree,
        };
        assert_eq!(grant.inherit, want, "{rule:?}");
        assert_eq!(grant.kind, PathKind::ReadOnlyInMatter);
    }
    let matter = w.canonical("matter");
    for name in [".git", "CLAUDE.md", ".codex"] {
        assert!(denied(&matter.join(name)).is_some(), "{name}");
    }
    // `.codex\config.toml` is under the `.codex` deny, which reaches down the tree.
    assert_eq!(
        denied(&matter.join(".codex")).unwrap().inherit,
        Inherit::Tree
    );
    assert_eq!(
        denied(&matter.join("big.pdf")).unwrap().inherit,
        Inherit::ThisOnly
    );
    for path in policy.read_only_in_state() {
        assert!(denied(path).is_some(), "{path:?}");
    }
    // The deny takes writes, deletes and permission changes, and leaves reads.
    let mask = Rights::DenyWrite.mask();
    for bit in [0x2, 0x4, 0x10, 0x40, 0x100, 0x1_0000, 0x4_0000, 0x8_0000] {
        assert_ne!(mask & bit, 0, "{bit:#x}");
    }
    assert_eq!(mask & 0x1, 0);
}

#[test]
fn modify_never_gives_delete_child_or_permission_changes() {
    // Without these, the agent can't strip a deny, nor delete a protected name through its
    // parent folder.
    for rights in [Rights::Modify, Rights::ReadAndExecute, Rights::CreateFiles] {
        assert_eq!(
            rights.mask() & (0x40 | 0x4_0000 | 0x8_0000),
            0,
            "{rights:?}"
        );
    }
}

#[test]
fn nothing_lands_in_the_profiles_claude_folder() {
    let w = World::new("plan-no-claude");
    let grants = grants_for(&w, &w.full_policy(), &sids());
    let claude = w.canonical("home/.claude");
    let home = w.canonical("home");
    for g in &grants {
        assert!(!g.path.starts_with(&claude), "{g:?}");
        assert!(
            !home.starts_with(&g.path),
            "a grant on the profile or above: {g:?}"
        );
    }
}

#[test]
fn a_claude_store_in_the_profiles_claude_folder_is_refused() {
    let w = World::new("plan-claude-store-refused");
    let policy = w
        .builder()
        .claude_store(w.at("home/.claude"))
        .build()
        .unwrap();
    let admitted = admitted(&w, &policy, &[], &FakeFolders::default());
    let error = plan(&admitted, &sids(), matter_one()).unwrap_err();
    let Error::Policy(message) = &error else {
        panic!("{error:?}");
    };
    assert!(message.contains("the Claude store folder"), "{message}");
    let root = dunce::canonicalize(w.dir.path()).unwrap();
    assert!(
        !message.contains(&*root.to_string_lossy()),
        "a path in {message}"
    );
}

#[test]
fn a_read_path_in_the_profiles_claude_folder_is_refused() {
    let w = World::new("plan-claude-read-refused");
    w.dir.dir("home/.claude/skills");
    let policy = w
        .builder()
        .read(w.at("home/.claude/skills"))
        .build()
        .unwrap();
    let admitted = admitted(&w, &policy, &[], &FakeFolders::default());
    let error = plan(&admitted, &sids(), matter_one()).unwrap_err();
    assert!(error.to_string().contains("a read path"), "{error}");
}

/// The profile's `.claude` folder is found in any letter case. On Windows the paths are drive
/// paths, as production passes there; a `/`-rooted path isn't one (round 9).
#[test]
fn the_claude_folder_matches_in_any_letter_case() {
    #[cfg(windows)]
    let [folder, upper, title, longer, home] = [
        r"C:\Users\lawyer\.claude",
        r"C:\Users\lawyer\.CLAUDE\skills",
        r"C:\Users\lawyer\.Claude",
        r"C:\Users\lawyer\.claude-home",
        r"C:\Users\lawyer",
    ];
    #[cfg(not(windows))]
    let [folder, upper, title, longer, home] = [
        "/Users/lawyer/.claude",
        "/Users/lawyer/.CLAUDE/skills",
        "/Users/lawyer/.Claude",
        "/Users/lawyer/.claude-home",
        "/Users/lawyer",
    ];
    let folder = Path::new(folder);
    assert!(in_folder(Path::new(upper), folder));
    assert!(in_folder(Path::new(title), folder));
    assert!(!in_folder(Path::new(longer), folder));
    assert!(!in_folder(Path::new(home), folder));
    // Windows paths split into parts only on Windows.
    #[cfg(windows)]
    assert!(in_folder(
        Path::new(r"c:\users\LAWYER\.Claude\agents"),
        folder
    ));
}

#[test]
fn a_pool_user_that_is_the_group_or_the_matter_sid_is_refused() {
    let w = World::new("plan-same-sid");
    let policy = w.builder().build().unwrap();
    let admitted = admitted(&w, &policy, &[], &FakeFolders::default());
    let mut as_group = sids();
    as_group.pool_group = as_group.pool_user.clone();
    let mut as_matter = sids();
    as_matter.matter = Some(as_matter.pool_user.clone());
    for subjects in [as_group, as_matter] {
        let error = plan(&admitted, &subjects, matter_one()).unwrap_err();
        assert!(matches!(error, Error::Policy(_)), "{error:?}");
    }
}

#[test]
fn an_unlaunched_registered_matter_is_fenced_at_another_matters_launch() {
    let w = World::new("plan-fence-registered");
    let m2 = MatterFolders {
        root: w.canonical("other/m2"),
        state: w.canonical("other/m3"),
        notes: None,
    };
    let assigned = Sid::parse(OTHER_POOL_SIDS[0]).unwrap();
    let neighbour = |fenced| Neighbours {
        registered: vec![RegisteredMatter {
            id: super::super::test_world::matter_two(),
            folders: m2.clone(),
            pool_user: assigned.clone(),
            fenced,
        }],
        reach: Vec::new(),
    };
    let policy = w.builder().build().unwrap();
    let unfenced = neighbour(false);
    let admitted = admitted_with(&w, &policy, &unfenced, &FakeFolders::default());
    let plan = plan(&admitted, &sids(), matter_one()).unwrap();
    let theirs: Vec<(&Grant, MatterId)> = plan
        .grants()
        .iter()
        .zip(plan.owners().iter().copied())
        .filter(|(g, _)| g.path == m2.root || g.path == m2.state)
        .collect();
    // This launch's pool user and the other one, denied on m2's two folders; m2's own user not.
    assert_eq!(theirs.len(), 4, "{theirs:#?}");
    for (grant, owner) in &theirs {
        assert_eq!(
            *owner,
            super::super::test_world::matter_two(),
            "the entry is m2's own"
        );
        assert_eq!(grant.rights, Rights::DenyReadWrite);
        assert_ne!(grant.sid, assigned);
    }
    assert!(theirs.iter().any(|(g, _)| g.sid.as_str() == USER_SID));
    // Once the ledger lists it as applied, the launcher passes `fenced` and it isn't planned.
    let fenced = neighbour(true);
    let admitted = admitted_with(&w, &policy, &fenced, &FakeFolders::default());
    let again = super::plan(&admitted, &sids(), matter_one()).unwrap();
    assert!(again.grants().iter().all(|g| g.path != m2.root));
    assert!(again.owners().iter().all(|o| *o == matter_one()));
}

#[test]
fn the_claude_store_and_the_credential_link_get_the_low_label() {
    let w = World::new("plan-store-label");
    let policy = w.full_policy();
    let grants = grants_for(&w, &policy, &sids());
    let label = |path: &Path| {
        grants
            .iter()
            .find(|g| g.path == path && g.rights == Rights::LowLabel)
            .unwrap_or_else(|| panic!("no label on {path:?}"))
    };
    let store = label(policy.claude_store().unwrap().dir());
    assert_eq!(
        (store.inherit, store.kind),
        (Inherit::Tree, PathKind::ClaudeStore)
    );
    let link = label(policy.credential_link().unwrap());
    assert_eq!(
        (link.inherit, link.kind),
        (Inherit::ThisOnly, PathKind::CredentialLink)
    );
}

/// The session's temp folder, `data/run/s1/tmp`: `Modify` down the tree for the pool user and
/// the Low label, as the state folder gets; nothing on the run folder above it or its parent.
#[test]
fn the_session_temp_folder_gets_modify_and_the_low_label_and_its_parents_nothing() {
    let w = World::new("plan-tmpdir");
    w.dir.dir("data/run/s1");
    let policy = w.builder().tmpdir(w.at("data/run/s1/tmp")).build().unwrap();
    let run = w.canonical("data/run/s1");
    let tmp = run.join("tmp");
    let grants = grants_for(&w, &policy, &sids());
    let on_tmp: Vec<(Rights, Inherit, PathKind, &str)> = grants
        .iter()
        .filter(|g| g.path == tmp)
        .map(|g| (g.rights, g.inherit, g.kind, who(&g.sid)))
        .collect();
    let kind = PathKind::TempFolder;
    let want = [
        (Rights::Modify, Inherit::Tree, kind, "pool"),
        (Rights::LowLabel, Inherit::Tree, kind, "low"),
    ];
    assert_eq!(on_tmp, want);
    for above in [&run, &w.canonical("data/run"), &w.canonical("data")] {
        let on = grants.iter().find(|g| &g.path == above);
        assert!(on.is_none(), "{on:?}");
    }
}

/// The profile, the sandbox records and the data folder's allow list.
mod data;
