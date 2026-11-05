//! Windows only (CI's Windows job runs them; boxbox cannot): directory and file symbolic links
//! beside the junctions the shared tests make, the read-only attribute, a locked file, and what a
//! pinned folder holds still.

use std::path::Path;

use super::{DATA, World, exists, make};
use crate::delete_app_data::walk::{check, plain};
use crate::delete_app_data::{DELETED, FAILED, KEPT_AND_FAILED, ops};

#[test]
fn a_directory_symbolic_link_goes_as_a_link_and_its_target_survives() {
    let world = World::new("dad-symlink-dir");
    world.data_dir("EBWebView");
    let link = world.data("EBWebView/linked");
    if !make::made_or_unprivileged(make::symlink_dir(&world.canary(), &link)) {
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
    // The junction needs no privilege and must be made; the symbolic links skip only without it.
    let dir_link = make::symlink_dir(&world.canary(), &world.data("a/dir-link"));
    let file_link = make::symlink_file(&world.canary().join("secret.txt"), &world.data("f"));
    if !make::made_or_unprivileged(dir_link) || !make::made_or_unprivileged(file_link) {
        return;
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
    assert!(!exists(&world.roaming_root()));
}

#[test]
fn a_locked_file_beside_a_kept_sandbox_has_its_own_code() {
    let world = World::new("dad-locked-kept");
    world.data_dir("agents");
    let held = world.data_file("agents/held.log");
    world.data_dir("sandbox");
    world.data_file("sandbox/acl-ledger.json");
    let lock = make::lock(&held);
    let (code, stderr) = world.delete(false);
    drop(lock);
    assert_eq!(code, KEPT_AND_FAILED);
    assert!(stderr.contains("agents: could not delete it"), "{stderr}");
    assert!(exists(&world.data("sandbox/acl-ledger.json")));
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
    let root = world.local_root();
    assert!(make::rename(&root, &world.at("Local/moved")).is_err());
    drop(pinned);
    make::rename(&sandbox, &world.data("moved")).unwrap();
}

#[test]
fn verbatim_prefixes_are_dropped_before_the_real_paths_are_compared() {
    let pairs = [
        (r"\\?\C:\Users\a\AppData\Local", r"C:\Users\a\AppData\Local"),
        (r"\\?\UNC\srv\share\Local", r"\\srv\share\Local"),
        (r"C:\Users\a", r"C:\Users\a"),
        (r"\\srv\share\Local", r"\\srv\share\Local"),
    ];
    for (path, expected) in pairs {
        assert_eq!(plain(Path::new(path)), Path::new(expected), "{path}");
    }
}

#[test]
fn a_data_folder_past_260_characters_is_still_its_known_folders() {
    // The known folder just under 260 characters, so `dunce` shortens it; the data folder past
    // it, so `dunce` keeps its `\\?\` prefix.
    let world = World::new("dad-long");
    let base = world.at("Local").as_os_str().len();
    let name = "k".repeat(250 - base - 1);
    world.scratch.dir(&format!("Local/{name}/{DATA}"));
    let known = world.at("Local").join(&name);
    let root = known.join(DATA);
    assert!(known.as_os_str().len() < 260 && root.as_os_str().len() > 260);
    assert!(
        dunce::canonicalize(&root).unwrap().as_os_str().len()
            != dunce::canonicalize(&known)
                .unwrap()
                .join(DATA)
                .as_os_str()
                .len(),
        "dunce no longer treats the two differently"
    );
    assert_eq!(check(&known, &root), Ok(true));
}
