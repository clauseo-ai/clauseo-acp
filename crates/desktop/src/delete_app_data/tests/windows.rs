//! Windows only (CI's Windows job runs them; boxbox cannot): directory and file symbolic links
//! beside the junctions the shared tests make, the read-only attribute, a locked file, and the
//! sandbox folder's name held to S15's.

use super::{DATA, World, exists, make};
use crate::delete_app_data::{DELETED, FAILED};

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
    assert!(stderr.contains("held.log: could not delete it"), "{stderr}");
    assert!(exists(&held));
    assert!(!exists(&world.data("agents/claude/other.json")));
    assert!(!exists(&world.data("store.sqlite")));
    assert!(!exists(&world.data("logs")));
    assert!(!exists(&world.at(&format!("Roaming/{DATA}"))));
}

#[test]
fn the_sandbox_folder_is_s15s() {
    assert_eq!(
        crate::delete_app_data::walk::SANDBOX_DIR,
        clauseo_sandbox::windows::matters::SANDBOX_DIR
    );
}
