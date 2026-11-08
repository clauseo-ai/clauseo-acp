//! The Git trees' engine against the fake: discovery, the tree id, the build, the cross-volume
//! copy and the rename into place. Retention, refresh, removal and failures are in
//! `tests_more.rs`.

use std::path::{Path, PathBuf};

use super::fake::{ADMIN, FakeTrees, GIT_FILES, GIT_ROOT, Node, key};
use super::{
    Account, FileId, Recorded, Unavailable, account_sddl, discover, ensure, git_dir_sddl,
    source_sddl, tree_id,
};
use crate::windows::sids::Sid;

/// A pool group's SID.
pub(super) fn group() -> Sid {
    Sid::parse("S-1-5-21-1-2-3-1000").unwrap()
}

/// Pool account `n`.
pub(super) fn account(n: u32) -> Account {
    Account {
        name: format!("ClauseoSb{n:02}"),
        sid: Sid::parse(&format!("S-1-5-21-1-2-3-{}", 1000 + n)).unwrap(),
    }
}

/// The source's tree id, as discovery names it.
pub(super) fn id_of(fake: &mut FakeTrees) -> String {
    discover::discover(fake).unwrap().id
}

/// Account `account`'s tree with id `id`.
pub(super) fn tree(account: &Account, id: &str) -> PathBuf {
    Path::new(ADMIN).join("git").join(&account.name).join(id)
}

/// What `ensure` recorded for the only account.
fn only(fake: &mut FakeTrees) -> Recorded {
    let mut out = ensure(fake, &[account(1)], &group());
    out.remove(0).1
}

#[test]
fn the_registered_install_is_the_source() {
    let mut fake = FakeTrees::new();
    fake.install(r"D:\Tools\Git");
    fake.registry = Some(r"D:\Tools\Git".to_owned());
    let source = discover::discover(&mut fake).unwrap();
    assert_eq!(key(&source.root), r"d:\tools\git");
}

#[test]
fn with_no_registry_value_program_files_git_is_the_source() {
    let mut fake = FakeTrees::new();
    fake.registry = None;
    let source = discover::discover(&mut fake).unwrap();
    assert_eq!(key(&source.root), key(Path::new(GIT_ROOT)));
    fake.registry = Some("  ".to_owned());
    assert!(discover::discover(&mut fake).is_ok());
}

#[test]
fn no_git_anywhere_is_no_git() {
    let mut fake = FakeTrees::new();
    fake.registry = None;
    fake.nodes
        .retain(|path, _| !path.starts_with(r"c:\program files\git"));
    assert_eq!(discover::discover(&mut fake), Err(Unavailable::NoGit));
    assert_eq!(only(&mut fake), Recorded::Unavailable(Unavailable::NoGit));
}

#[test]
fn an_install_missing_msys_or_git_exe_is_no_git() {
    for file in [r"usr\bin\msys-2.0.dll", r"cmd\git.exe"] {
        let mut fake = FakeTrees::new();
        fake.nodes.remove(&format!(r"c:\program files\git\{file}"));
        assert_eq!(
            discover::discover(&mut fake),
            Err(Unavailable::NoGit),
            "{file}"
        );
    }
}

#[test]
fn a_per_user_install_is_refused_and_recorded_with_a_plain_reason() {
    let mut fake = FakeTrees::new();
    let root = r"C:\Users\lawyer\AppData\Local\Programs\Git";
    fake.install(root);
    fake.registry = Some(root.to_owned());
    assert_eq!(
        only(&mut fake),
        Recorded::Unavailable(Unavailable::PerUserGit)
    );
    assert_eq!(fake.count("hard_link"), 0);
    let (error, line) = fake.logs.last().unwrap();
    assert!(*error);
    assert!(
        line.ends_with("(Git Bash unavailable: per-user-git)"),
        "{line}"
    );
    let record = fake.record.clone().unwrap();
    assert_eq!(record[0].1, "Git Bash unavailable: per-user-git");
}

#[test]
fn an_install_others_can_change_is_refused_as_per_user() {
    let mut fake = FakeTrees::new();
    fake.admin_only = false;
    assert_eq!(discover::discover(&mut fake), Err(Unavailable::PerUserGit));
}

#[test]
fn a_network_install_is_refused() {
    for path in [
        r"\\server\share\Git",
        r"Z:\Git",
        r"\\?\UNC\server\share\Git",
    ] {
        let mut fake = FakeTrees::new();
        fake.install(r"Z:\Git");
        fake.network.push(r"Z:\".to_owned());
        fake.registry = Some(path.to_owned());
        assert_eq!(
            discover::discover(&mut fake),
            Err(Unavailable::NetworkPath),
            "{path}"
        );
    }
}

#[test]
fn a_relative_or_dotted_registry_path_is_refused() {
    for path in [r"Git", r"C:\Program Files\..\Git", r"C:Git"] {
        let mut fake = FakeTrees::new();
        fake.registry = Some(path.to_owned());
        assert_eq!(
            discover::discover(&mut fake),
            Err(Unavailable::NetworkPath),
            "{path}"
        );
    }
}

#[test]
fn a_network_admin_folder_is_refused() {
    let mut fake = FakeTrees::new();
    fake.network.push(r"C:\Program Files\Clauseo".to_owned());
    assert_eq!(
        only(&mut fake),
        Recorded::Unavailable(Unavailable::NetworkPath)
    );
}

#[test]
fn a_reparse_point_on_the_way_or_in_the_install_is_refused() {
    let mut fake = FakeTrees::new();
    fake.nodes
        .insert(r"c:\program files\git".to_owned(), Node::Link);
    assert_eq!(
        discover::discover(&mut fake),
        Err(Unavailable::ReparsePoint)
    );

    let mut fake = FakeTrees::new();
    fake.nodes
        .insert(r"c:\program files\git\usr\share".to_owned(), Node::Link);
    assert_eq!(
        only(&mut fake),
        Recorded::Unavailable(Unavailable::ReparsePoint)
    );
    assert_eq!(
        fake.below(tree(&account(1), "x").parent().unwrap()),
        Vec::<String>::new()
    );
}

#[test]
fn the_tree_id_is_the_version_and_a_hash_of_the_files() {
    let msys = FileId {
        volume: 1,
        id: 10,
        size: 3,
        written: 4,
    };
    let git = FileId { id: 11, ..msys };
    let root = Path::new(GIT_ROOT);
    let id = tree_id(root, Some("2.47.1.1"), &msys, &git);
    let (version, hash) = id.rsplit_once('-').unwrap();
    assert_eq!(version, "2.47.1.1");
    assert_eq!(hash.len(), 12);
    assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(id, tree_id(root, Some("2.47.1.1"), &msys, &git));
    let replaced = FileId { id: 12, ..git };
    assert_ne!(id, tree_id(root, Some("2.47.1.1"), &msys, &replaced));
    let grown = FileId { size: 9, ..msys };
    assert_ne!(id, tree_id(root, Some("2.47.1.1"), &grown, &git));
    assert_ne!(
        id,
        tree_id(Path::new(r"D:\Git"), Some("2.47.1.1"), &msys, &git)
    );
    assert!(
        tree_id(root, Some(r"2.48\..\x"), &msys, &git).starts_with("2.48..x-"),
        "only letters, digits and dots stay"
    );
    assert!(tree_id(root, None, &msys, &git).starts_with("git-"));
    assert!(tree_id(root, Some("..."), &msys, &git).starts_with("git-"));
}

#[test]
fn the_tree_id_changes_when_the_installer_replaces_git() {
    let mut fake = FakeTrees::new();
    let before = id_of(&mut fake);
    assert_eq!(before, id_of(&mut fake));
    fake.new_file(r"C:\Program Files\Git\cmd\git.exe");
    let after = id_of(&mut fake);
    assert_ne!(before, after);
    fake.version = Some("2.48.0.1".to_owned());
    assert!(id_of(&mut fake).starts_with("2.48.0.1-"));
}

#[test]
fn every_file_is_linked_into_each_accounts_tree_with_its_own_dacl() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    let accounts = [account(1), account(2)];
    let out = ensure(&mut fake, &accounts, &group());
    for (at, account) in accounts.iter().enumerate() {
        let tree = tree(account, &id);
        assert_eq!(out[at], (account.sid.clone(), Recorded::Tree(tree.clone())));
        for file in GIT_FILES {
            let source = fake.node(&Path::new(GIT_ROOT).join(file)).cloned();
            let linked = fake.node(&tree.join(file)).cloned();
            assert!(matches!(source, Some(Node::File(_))));
            assert_eq!(linked, source, "{file} is a link of the install's file");
        }
        let sddl = Some(account_sddl(&account.sid));
        assert_eq!(fake.node(&tree), Some(&Node::Folder(sddl.clone())));
        assert_eq!(fake.node(tree.parent().unwrap()), Some(&Node::Folder(sddl)));
        assert_eq!(
            fake.node(&tree.join("usr")),
            Some(&Node::Folder(None)),
            "a folder below inherits the tree's DACL"
        );
    }
    assert_eq!(fake.count("hard_link"), 2 * GIT_FILES.len());
    assert_eq!(fake.count("copy_file"), 0);
    let git = Path::new(ADMIN).join("git");
    assert_eq!(
        fake.node(&git),
        Some(&Node::Folder(Some(git_dir_sddl(&group()))))
    );
    let record = fake.record.clone().unwrap();
    assert_eq!(record.len(), 2);
    assert_eq!(
        record[1],
        (account(2).sid, tree(&account(2), &id).display().to_string())
    );
}

#[test]
fn the_account_dacl_grants_that_account_read_and_execute_alone() {
    let sddl = account_sddl(&account(3).sid);
    assert_eq!(
        sddl,
        "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;S-1-5-21-1-2-3-1003)"
    );
    assert!(!sddl.contains(account(4).sid.as_str()));
    assert!(!sddl.contains(group().as_str()));
    assert_eq!(
        git_dir_sddl(&group()),
        "O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a0;;;S-1-5-21-1-2-3-1000)"
    );
}

#[test]
fn a_source_on_another_volume_is_copied_once_and_the_trees_link_the_copy() {
    let mut fake = FakeTrees::new();
    fake.install(r"D:\Git");
    fake.registry = Some(r"D:\Git".to_owned());
    let id = id_of(&mut fake);
    let accounts = [account(1), account(2), account(3)];
    let out = ensure(&mut fake, &accounts, &group());
    assert!(
        out.iter().all(|(_, r)| matches!(r, Recorded::Tree(_))),
        "{out:?}"
    );
    assert_eq!(
        fake.count("copy_file"),
        GIT_FILES.len(),
        "one copy for every account"
    );
    let copy = Path::new(ADMIN).join("git").join("_source").join(&id);
    let sddl = Some(source_sddl(&group()));
    assert_eq!(fake.node(&copy), Some(&Node::Folder(sddl.clone())));
    assert_eq!(fake.node(copy.parent().unwrap()), Some(&Node::Folder(sddl)));
    for account in &accounts {
        for file in GIT_FILES {
            let copied = fake.node(&copy.join(file)).cloned();
            let original = fake.node(&Path::new(r"D:\Git").join(file)).cloned();
            assert_ne!(copied, original, "a copy is a file of its own");
            assert_eq!(fake.node(&tree(account, &id).join(file)).cloned(), copied);
        }
    }
    // A second run copies nothing again.
    ensure(&mut fake, &accounts, &group());
    assert_eq!(fake.count("copy_file"), GIT_FILES.len());
}

#[test]
fn a_new_source_copy_replaces_the_old_one() {
    let mut fake = FakeTrees::new();
    fake.install(r"D:\Git");
    fake.registry = Some(r"D:\Git".to_owned());
    let old = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    fake.new_file(r"D:\Git\cmd\git.exe");
    let new = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    let sources = Path::new(ADMIN).join("git").join("_source");
    assert_eq!(
        fake.below(&sources)
            .iter()
            .filter(|p| !p.contains('\\'))
            .count(),
        1
    );
    assert!(fake.node(&sources.join(&new)).is_some());
    assert!(fake.node(&sources.join(&old)).is_none());
}

#[test]
fn a_tree_is_built_under_a_temporary_name_then_renamed_into_place() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    let tree = key(&tree(&account(1), &id));
    let temp = format!("{tree}.tmp");
    only(&mut fake);
    let links: Vec<&String> = fake
        .calls
        .iter()
        .filter(|line| line.starts_with("hard_link "))
        .collect();
    assert!(
        links
            .iter()
            .all(|line| line.contains(&format!("-> {temp}\\"))),
        "{links:?}"
    );
    let rename = fake
        .calls
        .iter()
        .position(|line| *line == format!("rename {temp} -> {tree}"))
        .unwrap();
    let last_link = fake
        .calls
        .iter()
        .rposition(|line| line.starts_with("hard_link "))
        .unwrap();
    assert!(rename > last_link, "the rename comes after the last link");
    assert!(fake.node(Path::new(&temp)).is_none());
}

#[test]
fn a_build_that_fails_midway_leaves_no_tree_and_no_temporary_one() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    fake.fail.push(r"\usr\bin\sh.exe".to_owned());
    assert_eq!(
        only(&mut fake),
        Recorded::Unavailable(Unavailable::BuildFailed)
    );
    let tree = tree(&account(1), &id);
    assert!(fake.node(&tree).is_none());
    assert_eq!(fake.below(tree.parent().unwrap()), Vec::<String>::new());
}

#[test]
fn a_failed_rename_leaves_no_temporary_tree() {
    let mut fake = FakeTrees::new();
    fake.fail.push("rename ".to_owned());
    assert_eq!(
        only(&mut fake),
        Recorded::Unavailable(Unavailable::BuildFailed)
    );
    assert_eq!(
        fake.below(tree(&account(1), "x").parent().unwrap()),
        Vec::<String>::new()
    );
}
