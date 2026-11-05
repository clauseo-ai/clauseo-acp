//! Windows only (CI's Windows job runs them; boxbox cannot): directory and file symbolic links
//! beside the junctions the shared tests make, the read-only attribute, a locked file, and what a
//! pinned folder holds still.

use super::{DATA, World, exists, make};
use crate::delete_app_data::{DELETED, FAILED, ops};

#[test]
fn a_directory_symbolic_link_goes_as_a_link_and_its_target_survives() {
    let world = World::new("dad-symlink-dir");
    world.data_dir("EBWebView");
    let link = world.data("EBWebView/linked");
    if let Err(error) = make::symlink_dir(&world.canary(), &link) {
        eprintln!("skipped: no privilege to make a directory symbolic link ({error})");
        return;
    }
    let (code, stderr) = world.delete(false);
    assert_eq!((code, stderr.as_str()), (DELETED, ""));
    world.assert_both_gone();
    world.assert_canary_whole();
}

#[test]
fn a_junction_to_a_file_link_and_a_folder_all_at_once() {
    // Junction, directory link and file link side by side, one of each kind, two levels down.
    let world = World::new("dad-every-link");
    world.data_dir("a/b");
    make::folder_link(&world.canary(), &world.data("a/b/junction"));
    let symlinks = make::symlink_dir(&world.canary(), &world.data("a/dir-link"))
        .and_then(|()| make::symlink_file(&world.canary().join("secret.txt"), &world.data("f")));
    if let Err(error) = symlinks {
        eprintln!("symbolic links not made ({error}); the junction alone is checked");
    }
    let (code, stderr) = world.delete(false);
    assert_eq!((code, stderr.as_str()), (DELETED, ""));
    world.assert_both_gone();
    world.assert_canary_whole();
}

#[test]
fn read_only_files_and_folders_are_deleted() {
    let world = World::new("dad-read-only");
    let folder = world.data_dir("blobs");
    let file = world.data_file("blobs/blob");
    make::set_read_only(&file);
    make::set_read_only(&folder);
    let (code, stderr) = world.delete(false);
    assert_eq!((code, stderr.as_str()), (DELETED, ""));
    world.assert_both_gone();
}

#[test]
fn a_locked_file_is_reported_and_the_rest_still_goes() {
    let world = World::new("dad-locked");
    world.data_dir("agents/claude");
    let held = world.data_file("agents/claude/held.log");
    world.data_file("agents/claude/other.json");
    world.data_file("store.sqlite");
    world.data_dir("logs");
    world.data_file("logs/host.log");
    let lock = make::lock(&held);
    let (code, stderr) = world.delete(false);
    drop(lock);
    assert_eq!(code, FAILED);
    // `agents` went to std's `remove_dir_all`, which stopped there: reported, not walked.
    assert!(stderr.contains("agents: could not delete it"), "{stderr}");
    assert!(exists(&held));
    assert!(!exists(&world.data("store.sqlite")));
    assert!(!exists(&world.data("logs")));
    assert!(!exists(&world.at(&format!("Roaming/{DATA}"))));
}

#[test]
fn a_pinned_folder_cannot_be_renamed_nor_can_the_folder_above_it() {
    // What `ops::pin_folder` relies on: while the handle is held, neither the folder nor its
    // parent can be moved away and replaced by a junction.
    let world = World::new("dad-pinned");
    let sandbox = world.data_dir("sandbox");
    world.data_file("sandbox/acl-ledger.json");
    let pinned = ops::pin_folder(&sandbox).unwrap();
    assert!(make::rename(&sandbox, &world.data("moved")).is_err());
    let root = world.at(&format!("Local/{DATA}"));
    assert!(make::rename(&root, &world.at("Local/moved")).is_err());
    drop(pinned);
    make::rename(&sandbox, &world.data("moved")).unwrap();
}
