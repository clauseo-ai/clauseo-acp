//! A matter on another volume than the data folder: what is set aside goes to the lawyer's own
//! set-aside folder on that volume, made at launch, and comes back from there; a launch whose
//! folder can't be made, or is another account's, is refused, another lawyer's never is, and a
//! restore never trusts a folder that isn't this lawyer's on the item's volume. Unix only,
//! through the stand-in's other volume (`Posix::other_volume`).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::posix::{LAWYER, Posix};
use super::{exit, exit_as, launch, launch_as, list, none, volume, world};
use crate::windows::set_aside::files::Place;
use crate::windows::set_aside::run;
use crate::windows::set_aside::run::Volume as _;
use crate::windows::set_aside::time::timestamp;
use crate::windows::set_aside::{ITEMS_ROOT, SetAsideFailure, drive_folder_name};
use crate::windows::test_world::{World, matter_one};

/// Another lawyer of the machine.
const LAWYER_B: &str = "S-1-5-21-1-2-3-1002";

/// The stand-in with the matter `root` on another volume, whose set-aside folder for
/// [`LAWYER`] is `<drive-d>\.clauseo-set-aside-<SID>`, which it returns.
fn on_drive_d(w: &World, root: &Path) -> (Posix, PathBuf) {
    let drive = w.dir.dir("drive-d");
    let volume = volume();
    volume.other_volume(root, &drive);
    (volume, drive.join(drive_folder_name(LAWYER)))
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
        let name = format!("drive-d/{}", drive_folder_name(LAWYER));
        if shape == "file" {
            w.dir.file(&name, "not a folder");
        } else {
            let elsewhere = w.dir.dir("elsewhere-folder");
            w.dir.symlink(&name, &elsewhere);
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

#[test]
fn the_drive_folder_is_named_for_the_lawyers_sid() {
    let (w, root, _data) = world("volumes-name");
    let (_volume, other) = on_drive_d(&w, &root);
    assert_eq!(
        other.file_name().unwrap(),
        ".clauseo-set-aside-S-1-5-21-1-2-3-1001"
    );
    assert_eq!(ITEMS_ROOT, ".clauseo-set-aside");
}

#[test]
fn a_drive_folder_another_account_owns_refuses_the_launch_and_stays_theirs() {
    let (w, root, data) = world("volumes-not-yours");
    let (volume, other) = on_drive_d(&w, &root);
    w.dir.file(
        &format!("drive-d/{}/planted.txt", drive_folder_name(LAWYER)),
        "a squatter's\n",
    );
    volume.foreign_as(&other, "S-1-5-21-9-9-9-500");

    let refused =
        run::before_launch(&volume, &root, &data, &matter_one(), "s1", &[], launch()).unwrap_err();

    assert_eq!(refused.failure, SetAsideFailure::DriveFolderNotYours);
    assert_eq!(refused.at, Some(other.clone()));
    assert!(volume.owned.borrow().is_empty(), "nothing taken back");
    assert_eq!(
        volume.owner(&other).unwrap(),
        "S-1-5-21-9-9-9-500",
        "the squatter still owns it"
    );
    assert!(!other.join("set-aside").exists(), "nothing made inside it");
    let place = Place::new(&data, &matter_one());
    assert_eq!(place.read_record().unwrap(), None, "nothing recorded");
}

#[test]
fn another_lawyers_drive_folder_never_refuses_this_one() {
    let (w, root, data) = world("volumes-lawyer-b");
    let (volume, mine) = on_drive_d(&w, &root);
    let theirs = w
        .dir
        .dir(&format!("drive-d/{}", drive_folder_name(LAWYER_B)));
    volume.foreign_as(&theirs, LAWYER_B);

    launch_as(&volume, &root, &data, "s1", &[], launch()).unwrap();
    w.dir.file("matter/CLAUDE.md", "agent's\n");
    let moved = exit_as(&volume, &root, &data, "s1", exit()).unwrap();

    let [entry] = moved.as_slice() else {
        panic!("one item set aside: {moved:?}");
    };
    assert!(entry.to.starts_with(&mine), "{}", entry.to.display());
    assert_eq!(
        volume.owner(&theirs).unwrap(),
        LAWYER_B,
        "theirs is untouched"
    );
    assert_eq!(
        std::fs::read_dir(&theirs).unwrap().count(),
        0,
        "nothing went into theirs"
    );
}

#[test]
fn a_restore_entry_under_another_lawyers_or_the_unnamed_folder_is_refused() {
    for other_root in [drive_folder_name(LAWYER_B), ITEMS_ROOT.to_owned()] {
        let (w, root, data) = world(&format!("volumes-root-{}", other_root.len()));
        let (volume, mine) = on_drive_d(&w, &root);
        launch_as(&volume, &root, &data, "s1", &[], launch()).unwrap();
        w.dir.file("matter/CLAUDE.md", "agent's\n");
        exit_as(&volume, &root, &data, "s1", exit()).unwrap();

        let place = Place::new(&data, &matter_one());
        let mut manifest = place.read_manifest().unwrap();
        let entry = manifest.entries.first_mut().unwrap();
        let below = entry.to.strip_prefix(&mine).unwrap().to_owned();
        entry.to = w.dir.dir("drive-d").join(&other_root).join(below);
        let entry = entry.clone();
        place.write_manifest(&manifest).unwrap();

        let refused = run::restore(&volume, &data, &entry).unwrap_err();

        assert_eq!(refused.failure, SetAsideFailure::Record, "{other_root}");
        assert!(!root.join("CLAUDE.md").exists(), "{other_root}");
    }
}

#[test]
fn the_words_for_a_drive_folder_another_account_owns_are_plain() {
    assert_eq!(
        SetAsideFailure::DriveFolderNotYours.to_string(),
        "Clauseo's private folder on the matter's drive belongs to another account, so Clauseo \
         won't use it until its system task moves it aside"
    );
}
