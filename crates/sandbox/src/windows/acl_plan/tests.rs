use std::fmt::Write as _;
use std::path::Path;

use super::super::admit::{Admitted, PathKind, admit_with};
use super::super::test_world::{FakeGroups, MATTER_SID, USER_SID, World, sids};
use super::*;
use crate::windows::FolderSupport;
use crate::{Error, Policy};

/// The gate with every folder answering `Ok`.
fn admitted<'a>(
    w: &World,
    policy: &'a Policy,
    others: &[PathBuf],
    groups: &FakeGroups,
) -> Admitted<'a> {
    let data = w.at("data");
    admit_with(policy, &data, others, |_| Ok(FolderSupport::Ok), groups).unwrap()
}

/// The plan as text, one entry a line, paths relative to the temp folder.
fn render(w: &World, grants: &[Grant]) -> String {
    let mut out = String::new();
    for g in grants {
        let who = match g.sid.as_str() {
            USER_SID => "user",
            MATTER_SID => "matter",
            _ => "other",
        };
        writeln!(
            out,
            "{:?} {:?} {who} 0x{:08x} {:?} {}",
            g.kind,
            g.rights,
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
    let policy = w.full_policy();
    let groups = FakeGroups {
        open: vec![w.canonical("other/m2")],
        ..FakeGroups::default()
    };
    let others = [w.canonical("other/m2"), w.canonical("other/m3")];
    let grants = plan(&admitted(&w, &policy, &others, &groups), &sids()).unwrap();
    insta::assert_snapshot!(render(&w, &grants));
}

#[test]
fn every_allow_comes_in_a_pair() {
    let w = World::new("plan-pairs");
    let policy = w.full_policy();
    let grants = plan(&admitted(&w, &policy, &[], &FakeGroups::default()), &sids()).unwrap();
    let allows: Vec<&Grant> = grants.iter().filter(|g| !g.rights.is_deny()).collect();
    assert!(allows.len() >= 20, "{allows:#?}");
    let (user, matter) = (Sid::new(USER_SID).unwrap(), Sid::new(MATTER_SID).unwrap());
    for g in &allows {
        let partner = if g.sid == user { &matter } else { &user };
        assert!(g.sid == user || g.sid == matter, "{g:?}");
        let paired = allows.iter().any(|p| {
            &p.sid == partner && p.path == g.path && p.rights == g.rights && p.inherit == g.inherit
        });
        assert!(paired, "no pair for {g:?}");
    }
    let users = allows.iter().filter(|g| g.sid == user).count();
    assert_eq!(users * 2, allows.len());
}

#[test]
fn every_deny_names_the_matter_sid_alone() {
    // A deny for the user would block every matter's agent, this one's included.
    let w = World::new("plan-denies");
    let policy = w.full_policy();
    let groups = FakeGroups {
        open: vec![w.canonical("other/m2")],
        ..FakeGroups::default()
    };
    let admitted = admitted(&w, &policy, &[w.canonical("other/m2")], &groups);
    let grants = plan(&admitted, &sids()).unwrap();
    let denies: Vec<&Grant> = grants.iter().filter(|g| g.rights.is_deny()).collect();
    assert_ne!(denies.len(), 0);
    for g in denies {
        assert_eq!(g.sid.as_str(), MATTER_SID, "{g:?}");
    }
}

#[test]
fn read_only_names_get_deny_write() {
    let w = World::new("plan-read-only");
    let policy = w.full_policy();
    let grants = plan(&admitted(&w, &policy, &[], &FakeGroups::default()), &sids()).unwrap();
    let denied = |path: &Path| {
        grants.iter().find(|g| {
            g.path == path && g.rights == Rights::DenyWrite && g.sid.as_str() == MATTER_SID
        })
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
    let policy = w.full_policy();
    let grants = plan(&admitted(&w, &policy, &[], &FakeGroups::default()), &sids()).unwrap();
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
    let error = plan(&admitted(&w, &policy, &[], &FakeGroups::default()), &sids()).unwrap_err();
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
    let error = plan(&admitted(&w, &policy, &[], &FakeGroups::default()), &sids()).unwrap_err();
    assert!(error.to_string().contains("a read path"), "{error}");
}

#[test]
fn the_claude_folder_matches_in_any_letter_case() {
    let folder = Path::new("/Users/lawyer/.claude");
    assert!(in_folder(Path::new("/Users/lawyer/.CLAUDE/skills"), folder));
    assert!(in_folder(Path::new("/Users/lawyer/.Claude"), folder));
    assert!(!in_folder(Path::new("/Users/lawyer/.claude-home"), folder));
    assert!(!in_folder(Path::new("/Users/lawyer"), folder));
    // Windows paths split into parts only on Windows.
    #[cfg(windows)]
    let windows = Path::new(r"C:\Users\lawyer\.claude");
    #[cfg(windows)]
    assert!(in_folder(
        Path::new(r"c:\users\LAWYER\.Claude\agents"),
        windows
    ));
}

#[test]
fn another_matter_a_group_can_reach_is_denied_to_this_matter() {
    // D8: `Users` stays a restricting SID, so this matter's token would pass both checks on m2.
    let w = World::new("plan-d8");
    let policy = w.full_policy();
    let (m2, m3) = (w.canonical("other/m2"), w.canonical("other/m3"));
    let groups = FakeGroups {
        open: vec![m2.clone()],
        ..FakeGroups::default()
    };
    let grants = plan(
        &admitted(&w, &policy, &[m2.clone(), m3.clone()], &groups),
        &sids(),
    )
    .unwrap();
    let on = |path: &Path| grants.iter().filter(|g| g.path == path).collect::<Vec<_>>();
    let m2_grants = on(&m2);
    assert_eq!(m2_grants.len(), 1, "{m2_grants:#?}");
    let deny = m2_grants[0];
    assert_eq!(deny.sid.as_str(), MATTER_SID);
    assert_eq!(deny.rights, Rights::DenyReadWrite);
    assert_eq!(deny.inherit, Inherit::Tree);
    assert_eq!(deny.kind, PathKind::OtherMatter);
    assert_eq!(deny.rights.mask() & 0x1, 0x1, "read data is denied");
    assert_eq!(deny.rights.mask() & 0x2, 0x2, "write data is denied");
    assert!(on(&m3).is_empty(), "m3's DACL grants no group");
}

#[test]
fn the_same_sid_twice_is_refused() {
    let w = World::new("plan-same-sid");
    let policy = w.builder().build().unwrap();
    let same = Sids {
        user: Sid::new(MATTER_SID).unwrap(),
        matter: Sid::new(MATTER_SID).unwrap(),
    };
    let error = plan(&admitted(&w, &policy, &[], &FakeGroups::default()), &same).unwrap_err();
    assert!(matches!(error, Error::Policy(_)), "{error:?}");
}

#[test]
fn a_sid_is_checked_for_shape() {
    assert!(Sid::new(USER_SID).is_some());
    assert!(Sid::new("S-1-5-12").is_some());
    for bad in [
        "",
        "S-1",
        "S-1-",
        "S-2-5",
        "s-1-5",
        "S-1-5-x",
        "S-1-5--1",
        "Users",
        "S-1-99999999999",
    ] {
        assert!(Sid::new(bad).is_none(), "{bad}");
    }
    let json = serde_json::to_string(&Sid::new("S-1-5-12").unwrap()).unwrap();
    assert_eq!(json, "\"S-1-5-12\"");
    assert!(serde_json::from_str::<Sid>("\"not a sid\"").is_err());
}
