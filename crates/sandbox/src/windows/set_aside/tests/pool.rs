//! S15 `PR4a2` review: the set-aside's hand-over goes only to items a pool SID the machine's
//! record names owns. A colleague's file, or one `Administrators` owns, that the agent
//! hard-linked into an item to be set aside keeps its owner and permissions; with the record
//! unreadable or empty nothing is handed over and the item stays listed.

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use super::super::run::Volume as _;
use super::posix::{POOL_USER, Posix};
use super::{before, exit, exit_as, launch, list, world};
use crate::windows::test_world::World;

/// A colleague's SID, and `Administrators`'.
const COLLEAGUE: &str = "S-1-5-21-1004336348-1177238915-682003330-1105";
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// A lawyer's folder the agent renames to `sub/.git` after the launch, holding the hook it
/// wrote (pool-owned) and hard links to a colleague's and an Administrators-owned file in the
/// matter. Returns the folder's new path, the hook and the two files in the matter.
fn renamed_git(w: &World, root: &Path, volume: &Posix) -> (PathBuf, PathBuf, [PathBuf; 2]) {
    let hook = w
        .dir
        .file("matter/Precedents/hooks/post-checkout", "#!/bin/sh\n");
    volume.foreign(&hook);
    let colleague = w
        .dir
        .file("matter/Colleague/brief.docx", "the colleague's\n");
    let admin = w.dir.file("matter/Firm/policy.docx", "the firm's\n");
    volume.foreign_as(&colleague, COLLEAGUE);
    volume.foreign_as(&admin, ADMINISTRATORS);
    crate::test_support::set_mode(&colleague, 0o640);
    w.dir.hard_link("matter/Precedents/brief.docx", &colleague);
    w.dir.hard_link("matter/Precedents/policy.docx", &admin);
    let git = root.join("sub/.git");
    crate::windows::write::rename_new(&root.join("Precedents"), &git).unwrap();
    (git, hook, [colleague, admin])
}

fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path).unwrap().mode()
}

#[test]
fn a_colleagues_file_hard_linked_into_a_set_aside_item_keeps_its_owner() {
    let (w, root, data) = world("aside-pool-linked");
    w.dir.file("matter/Precedents/memo.md", "# the lawyer's\n");
    w.dir.dir("matter/sub");
    before(&root, &data, launch()).unwrap();
    let volume = Posix::new(true);
    let (git, _hook, [colleague, admin]) = renamed_git(&w, &root, &volume);
    let modes = [mode(&colleague), mode(&admin)];

    let moved = exit_as(&volume, &root, &data, "s1", exit()).unwrap();
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].from, git);
    // The pool user's hook is still handed over; the two linked files are not.
    let hook = moved[0].to.join("hooks/post-checkout");
    assert_eq!(*volume.owned.borrow(), [hook]);
    for (path, owner) in [(&colleague, COLLEAGUE), (&admin, ADMINISTRATORS)] {
        assert_eq!(volume.owner(path).unwrap(), owner, "{}", path.display());
    }
    assert_eq!(
        [mode(&colleague), mode(&admin)],
        modes,
        "permissions unchanged"
    );
    assert_eq!(list(&data), moved);
}

#[test]
fn an_unreadable_or_empty_pool_record_hands_nothing_over() {
    for (test, pool) in [
        ("aside-pool-unreadable", None),
        ("aside-pool-empty", Some(Vec::new())),
    ] {
        let (w, root, data) = world(test);
        w.dir.file("matter/Precedents/memo.md", "# the lawyer's\n");
        w.dir.dir("matter/sub");
        before(&root, &data, launch()).unwrap();
        let volume = Posix::new(true);
        let (git, hook, _files) = renamed_git(&w, &root, &volume);
        *volume.pool.borrow_mut() = pool;

        let moved = exit_as(&volume, &root, &data, "s1", exit()).unwrap();
        assert_eq!(moved.len(), 1, "{test}: {moved:?}");
        assert_eq!(moved[0].from, git);
        assert!(volume.owned.borrow().is_empty(), "{test}");
        let inside = moved[0].to.join("hooks/post-checkout");
        assert_eq!(volume.owner(&inside).unwrap(), POOL_USER, "{test}");
        assert!(!hook.exists());
        assert_eq!(list(&data), moved, "{test}: the item stays listed");
    }
}
