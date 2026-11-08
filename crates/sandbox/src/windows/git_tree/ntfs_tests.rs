//! The Git trees on NTFS (Windows only, elevated: SYSTEM as owner needs the restore privilege):
//! `--adopt`'s lock-down of the admin folder leaves a tree's files as they were. Its DACL
//! propagates to what inherits from the admin folder, and `git\`, protected, stops it there.

use super::{TreeFs, WindowsTrees, account_sddl, git_dir_sddl, source_sddl};
use crate::test_support::TempDir;
use crate::windows::admin_copy::GIT;
use crate::windows::admin_copy::lock_tree_os::lock_tree;
use crate::windows::sids::Sid;
use crate::windows::test_acl::{elevated_tests_enabled, entries};

/// Users.
const USERS: &str = "S-1-5-32-545";
/// `FILE_GENERIC_READ | FILE_GENERIC_EXECUTE`.
const READ_EXECUTE: u32 = 0x12_00a9;

#[test]
fn elevated_adopts_lock_down_of_the_admin_folder_leaves_a_trees_files_alone() {
    if !elevated_tests_enabled() {
        return;
    }
    let dir = TempDir::new("git-tree-lock-down");
    let install = dir.dir("Git");
    dir.file(r"Git\usr\bin\msys-2.0.dll", "MZ stand-in");
    let admin = dir.dir(r"pf\Clauseo\Sandbox");
    let (group, account) = (
        Sid::parse("S-1-5-32-547").unwrap(),
        Sid::parse("S-1-5-32-546").unwrap(),
    );
    let mut fs = WindowsTrees;
    let git = admin.join(GIT);
    let copy = git.join("_source").join("id");
    let tree = git.join("ClauseoSb01").join("id");
    fs.level(&git, &git_dir_sddl(&group)).unwrap();
    for level in [git.join("_source"), copy.clone()] {
        fs.level(&level, &source_sddl(&group)).unwrap();
    }
    let listing = fs.walk(&install).unwrap();
    fs.copy_tree(&install, &listing, &copy).unwrap();
    for level in [git.join("ClauseoSb01"), tree.clone()] {
        fs.level(&level, &account_sddl(&account)).unwrap();
    }
    let listing = fs.walk(&copy).unwrap();
    fs.link_tree(&copy, &listing, &tree).unwrap();
    let file = tree.join(r"usr\bin\msys-2.0.dll");
    let before = (entries(&git), entries(&file));
    let users_read = |found: &[(u8, u8, u32, Sid)]| {
        found
            .iter()
            .any(|(_, _, mask, sid)| sid.as_str() == USERS && *mask == READ_EXECUTE)
    };
    assert!(users_read(&before.1), "{:?}", before.1);

    lock_tree(&admin, &[GIT.to_owned()]).unwrap();

    assert_eq!(
        (entries(&git), entries(&file)),
        before,
        "git\\ stops the propagation"
    );
}
