//! A matter on another volume than the data folder: what is set aside goes to that volume's own
//! set-aside folder, made at launch, and comes back from there; a launch whose folder can't be
//! made is refused, and a restore never trusts a folder that isn't the item's volume's. Unix
//! only, through the stand-in's other volume (`Posix::other_volume`).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::posix::Posix;
use super::{exit, exit_as, launch, launch_as, list, none, volume, world};
use crate::windows::set_aside::files::Place;
use crate::windows::set_aside::run;
use crate::windows::set_aside::time::timestamp;
use crate::windows::set_aside::{ITEMS_ROOT, SetAsideFailure};
use crate::windows::test_world::{World, matter_one};

/// The stand-in with the matter `root` on another volume, whose set-aside folder is
/// `<drive-d>\.clauseo-set-aside`, which it returns.
fn on_drive_d(w: &World, root: &Path) -> (Posix, PathBuf) {
    let other = w.dir.dir("drive-d").join(ITEMS_ROOT);
    let volume = volume();
    volume.other_volume(root, &other);
    (volume, other)
}

/// The pass folder for `now` below the set-aside folder `root`.
fn pass(root: &Path, now: SystemTime) -> PathBuf {
    root.join("set-aside")
        .join(matter_one().to_string())
        .join(timestamp(now))
}

#[test]
fn a_new_git_in_a_matter_on_another_drive_goes_to_that_drives_folder() {
    let (w, root, data) = world("volumes-git");
    let (volume, other) = on_drive_d(&w, &root);
    launch_as(&volume, &root, &data, "s1", &[], launch()).unwrap();
    let matter_folder = other.join("set-aside").join(matter_one().to_string());
    assert!(
        matter_folder.is_dir(),
        "the launch makes the drive's folder"
    );

    w.dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
    let moved = exit_as(&volume, &root, &data, "s1", exit()).unwrap();

    let to = pass(&other, exit()).join(".git");
    let found: Vec<_> = moved
        .iter()
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect();
    assert_eq!(found, [(root.join(".git"), to.clone())]);
    assert!(to.join("HEAD").is_file());
    assert!(!root.join(".git").exists());
    assert!(
        !pass(&data.join("sandbox"), exit()).exists(),
        "nothing goes under the data folder"
    );
    assert_eq!(list(&data), moved, "the manifest stays in the data folder");
}

#[test]
fn a_restore_from_another_drive_puts_the_item_back_and_removes_the_emptied_folder() {
    let (w, root, data) = world("volumes-restore");
    let (volume, other) = on_drive_d(&w, &root);
    launch_as(&volume, &root, &data, "s1", &[], launch()).unwrap();
    w.dir.file("matter/CLAUDE.md", "agent's\n");
    let moved = exit_as(&volume, &root, &data, "s1", exit()).unwrap();
    let [entry] = moved.as_slice() else {
        panic!("one item set aside: {moved:?}");
    };
    assert!(entry.to.starts_with(&other));

    run::restore(&volume, &data, entry).unwrap();

    assert_eq!(
        std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
        "agent's\n"
    );
    assert!(
        !pass(&other, exit()).exists(),
        "the emptied pass folder goes"
    );
    assert!(
        other
            .join("set-aside")
            .join(matter_one().to_string())
            .is_dir()
    );
    assert_eq!(list(&data), none());
}

#[test]
fn a_launch_whose_drive_folder_cant_be_made_is_refused_naming_it() {
    for shape in ["file", "link"] {
        let (w, root, data) = world(&format!("volumes-refused-{shape}"));
        let (volume, other) = on_drive_d(&w, &root);
        if shape == "file" {
            w.dir.file("drive-d/.clauseo-set-aside", "not a folder");
        } else {
            let elsewhere = w.dir.dir("elsewhere-folder");
            w.dir.symlink("drive-d/.clauseo-set-aside", &elsewhere);
        }

        let refused = run::before_launch(&volume, &root, &data, &matter_one(), "s1", &[], launch())
            .unwrap_err();

        assert_eq!(refused.failure, SetAsideFailure::DriveFolder, "{shape}");
        assert_eq!(refused.at, Some(other.clone()), "{shape}");
        let place = Place::new(&data, &matter_one());
        assert_eq!(
            place.read_record().unwrap(),
            None,
            "{shape}: nothing recorded"
        );
    }
}

#[test]
fn the_words_for_a_drive_folder_that_cant_be_made_are_plain() {
    assert_eq!(
        SetAsideFailure::DriveFolder.to_string(),
        "Clauseo can't make a private folder on the matter's drive for what it moves out of the \
         matter"
    );
}

#[test]
fn a_restore_entry_under_a_foreign_folder_is_refused() {
    let (w, root, data) = world("volumes-foreign");
    let (volume, other) = on_drive_d(&w, &root);
    launch_as(&volume, &root, &data, "s1", &[], launch()).unwrap();
    w.dir.file("matter/CLAUDE.md", "agent's\n");
    exit_as(&volume, &root, &data, "s1", exit()).unwrap();

    // The manifest's own entry rewritten to name another drive's folder, the rest kept.
    let place = Place::new(&data, &matter_one());
    let mut manifest = place.read_manifest().unwrap();
    let entry = manifest.entries.first_mut().unwrap();
    let below = entry.to.strip_prefix(&other).unwrap().to_owned();
    entry.to = w.dir.dir("drive-e").join(ITEMS_ROOT).join(below);
    let entry = entry.clone();
    place.write_manifest(&manifest).unwrap();

    let refused = run::restore(&volume, &data, &entry).unwrap_err();

    assert_eq!(refused.failure, SetAsideFailure::Record);
    assert!(!root.join("CLAUDE.md").exists());
    assert_eq!(list(&data), [entry], "still listed");
}
