//! Restoring one file of a version (Versions' per-file Restore): a changed file's content or a
//! deleted file comes back alone, every other file stays as it is, the copy it replaces is kept
//! in the history first so the restore can be taken back, and a file whose name is now a folder
//! (or whose folder is now a file) is refused before anything is written.

use clauseo_matters::{CheckpointOutcome, Error, Matter, RestoreFiles};
use clauseo_protocol::{CheckpointKind, ErrorCode, MatterPath};

use crate::fixture::{Fixture, assert_none, cid, meta, path, read_at, restore_request, tools};
use crate::support;

/// Checkpoint 1 holds `a.txt` "one", `b.txt` "bee" and `sub/deep/c.txt` "cee"; checkpoint 3,
/// after a turn, holds `a.txt` "two" and `b.txt` "bee, edited" (`sub/deep/c.txt` unchanged).
fn versioned(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("a.txt", "one");
    fx.write("b.txt", "bee");
    fx.write("sub/deep/c.txt", "cee");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    fx.write("b.txt", "bee, edited");
    matter
        .after_turn(&meta(3), &tools(&["a.txt", "b.txt"]))
        .unwrap();
    (fx, matter)
}

fn only(text: &str) -> Vec<MatterPath> {
    vec![path(text)]
}

fn ids(matter: &Matter) -> Vec<clauseo_protocol::CheckpointId> {
    matter.timeline().iter().map(|entry| entry.id).collect()
}

#[test]
fn one_changed_file_comes_back_alone_and_can_be_taken_back() {
    let (fx, mut matter) = versioned("restore-one-changed");
    // The lawyer's own edit, not yet in any version.
    fx.write("a.txt", "three, by hand");
    let chosen = only("a.txt");

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 4, 5))
        .unwrap();
    assert_eq!(restored.written, chosen);
    assert_none(&restored.removed);
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(
        fx.read("b.txt"),
        b"bee, edited",
        "another changed file stays"
    );
    assert_eq!(fx.read("sub/deep/c.txt"), b"cee");
    assert!(matches!(
        restored.checkpoint.outcome,
        CheckpointOutcome::Created(id) if id == cid(5)
    ));

    // The copy it replaced is in the history, as the outside-edits checkpoint, first.
    assert_eq!(ids(&matter), [cid(1), cid(3), cid(4), cid(5)]);
    assert_eq!(matter.timeline()[2].kind, CheckpointKind::OutsideEdits);
    assert_eq!(read_at(&matter, 4, "a.txt"), b"three, by hand");
    assert_eq!(read_at(&matter, 5, "b.txt"), b"bee, edited");

    // Versions' Undo of the restore: the version before it, the restore named as undone.
    let mut back = restore_request(4, RestoreFiles::All, 6, 7);
    back.undoes = Some(cid(5));
    let undone = matter.restore(&back).unwrap();
    assert_eq!(undone.written, chosen);
    assert_eq!(fx.read("a.txt"), b"three, by hand");
    assert_eq!(fx.read("b.txt"), b"bee, edited");
}

#[test]
fn one_deleted_file_comes_back_with_its_folders() {
    let (fx, mut matter) = versioned("restore-one-deleted");
    support::remove_dir_all(fx.file("sub"));
    support::remove_file(fx.file("b.txt"));
    let chosen = only("sub/deep/c.txt");

    let plan = matter
        .preview_restore(cid(1), RestoreFiles::Only(&chosen), None)
        .unwrap();
    let planned: Vec<&str> = plan.writes.iter().map(|w| w.path().as_str()).collect();
    assert_eq!(planned, ["sub/deep/c.txt"]);

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 4, 5))
        .unwrap();
    assert_eq!(restored.written, chosen);
    assert_eq!(fx.read("sub/deep/c.txt"), b"cee");
    assert!(!fx.exists("b.txt"), "another deleted file stays deleted");
    assert_eq!(fx.read("a.txt"), b"two");
}

/// Refused with `matter.file_folder_clash`, by the preview and the restore alike, with nothing
/// written and no checkpoint taken.
fn assert_refused(fx: &Fixture, matter: &mut Matter, chosen: &[MatterPath]) {
    let before = ids(matter);
    let err = matter
        .preview_restore(cid(1), RestoreFiles::Only(chosen), None)
        .unwrap_err();
    assert!(matches!(&err, Error::FileFolderClash { path } if path == &chosen[0]));
    assert_eq!(err.code(), ErrorCode::MatterFileFolderClash);
    let err = matter
        .restore(&restore_request(1, RestoreFiles::Only(chosen), 4, 5))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::MatterFileFolderClash);
    assert_eq!(ids(matter), before, "nothing is recorded");
    assert_eq!(fx.read("b.txt"), b"bee, edited");
}

#[test]
fn a_file_whose_name_is_now_a_folder_is_refused() {
    let (fx, mut matter) = versioned("restore-one-now-folder");
    support::remove_file(fx.file("a.txt"));
    fx.write("a.txt/inside.txt", "in a folder now");

    assert_refused(&fx, &mut matter, &only("a.txt"));
    assert_eq!(fx.read("a.txt/inside.txt"), b"in a folder now");

    // The whole matter keeps it as it is instead, as before.
    let plan = matter
        .preview_restore(cid(1), RestoreFiles::All, None)
        .unwrap();
    assert!(plan.kept.contains(&path("a.txt")));
}

#[test]
fn a_file_whose_folder_is_now_a_file_is_refused() {
    let (fx, mut matter) = versioned("restore-one-folder-now-file");
    support::remove_dir_all(fx.file("sub"));
    fx.write("sub", "a file now");

    assert_refused(&fx, &mut matter, &only("sub/deep/c.txt"));
    assert_eq!(fx.read("sub"), b"a file now");
}
