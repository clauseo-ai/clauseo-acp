//! Restoring one file of a version (Versions' per-file Restore): a changed file's content or a
//! deleted file comes back alone, every other file stays as it is, the copy it replaces is kept
//! in the history first so the restore can be taken back, and a file whose name is now a folder
//! (or whose folder is now a file) is refused before anything is written.

use clauseo_matters::{CheckpointOutcome, Error, Matter, RestoreFiles};
use clauseo_protocol::{CheckpointKind, ErrorCode, MatterPath};

use crate::fixture::{
    Fixture, assert_no_temp_files, assert_none, cid, meta, path, read_at, restore_request, tools,
};
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

/// Refused with `matter.file_folder_clash`, by the preview and the restore alike, naming the
/// entry in the way (`in_way_of`), with nothing written and no checkpoint taken.
fn assert_refused(fx: &Fixture, matter: &mut Matter, chosen: &[MatterPath], in_way_of: &str) {
    let before = ids(matter);
    let err = matter
        .preview_restore(cid(1), RestoreFiles::Only(chosen), None)
        .unwrap_err();
    assert!(
        matches!(&err, Error::FileFolderClash { path, in_way }
            if path == &chosen[0] && in_way.as_str() == in_way_of),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::MatterFileFolderClash);
    let err = matter
        .restore(&restore_request(1, RestoreFiles::Only(chosen), 4, 5))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::MatterFileFolderClash);
    assert_eq!(ids(matter), before, "nothing is recorded");
    assert_eq!(fx.read("b.txt"), b"bee, edited");
    assert_no_temp_files(&fx.folder);
}

#[test]
fn a_file_whose_name_is_now_a_folder_is_refused() {
    let (fx, mut matter) = versioned("restore-one-now-folder");
    support::remove_file(fx.file("a.txt"));
    fx.write("a.txt/inside.txt", "in a folder now");

    assert_refused(&fx, &mut matter, &only("a.txt"), "a.txt");
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

    assert_refused(&fx, &mut matter, &only("sub/deep/c.txt"), "sub");
    assert_eq!(fx.read("sub"), b"a file now");
}

/// A socket where a folder of the chosen file goes is a clash too, not a failure to read.
#[cfg(unix)]
#[test]
fn a_file_whose_folder_is_now_a_socket_is_refused() {
    let (fx, mut matter) = versioned("restore-one-folder-now-socket");
    support::remove_dir_all(fx.file("sub"));
    let _socket = support::socket(fx.file("sub"));

    assert_refused(&fx, &mut matter, &only("sub/deep/c.txt"), "sub");
}

/// Undo of a one-file restore, scoped to that file as Versions asks it: the file goes back to
/// the copy it replaced, and another file edited since the restore keeps its edit.
#[test]
fn undoing_a_one_file_restore_leaves_other_edits_alone() {
    let (fx, mut matter) = versioned("restore-one-undo-scoped");
    fx.write("a.txt", "three, by hand");
    let chosen = only("a.txt");
    matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 4, 5))
        .unwrap();
    fx.write("b.txt", "bee, edited after the restore");

    let mut back = restore_request(4, RestoreFiles::Only(&chosen), 6, 7);
    back.undoes = Some(cid(5));
    let plan = matter
        .preview_restore(cid(4), RestoreFiles::Only(&chosen), Some(cid(5)))
        .unwrap();
    let planned: Vec<&str> = plan.writes.iter().map(|w| w.path().as_str()).collect();
    assert_eq!(planned, ["a.txt"]);
    let undone = matter.restore(&back).unwrap();
    assert_eq!(undone.written, chosen);
    assert_eq!(fx.read("a.txt"), b"three, by hand");
    assert_eq!(fx.read("b.txt"), b"bee, edited after the restore");
}

/// A file moved since comes back at its old path (Versions asks by the rename's `from`), and
/// the moved copy stays; a file the lawyer made at the old path since is kept in history first.
#[test]
fn a_moved_file_comes_back_at_its_old_path() {
    let (fx, mut matter) = versioned("restore-one-moved");
    support::rename(fx.file("a.txt"), fx.file("moved.txt"));
    let chosen = only("a.txt");
    let restored = matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 4, 5))
        .unwrap();
    assert_eq!(restored.written, chosen);
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(fx.read("moved.txt"), b"two");

    fx.write("a.txt", "a new file at the old path");
    let again = matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 6, 7))
        .unwrap();
    assert_eq!(again.written, chosen);
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(read_at(&matter, 6, "a.txt"), b"a new file at the old path");
}

/// A link at the chosen file is kept as it is, never written through, and is no error.
#[cfg(unix)]
#[test]
fn a_link_at_the_chosen_file_is_kept() {
    let (fx, mut matter) = versioned("restore-one-link");
    support::remove_file(fx.file("a.txt"));
    support::symlink(fx.file("b.txt"), fx.file("a.txt"));
    let chosen = only("a.txt");
    let restored = matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 4, 5))
        .unwrap();
    assert_none(&restored.written);
    assert_eq!(
        fx.read("b.txt"),
        b"bee, edited",
        "nothing written through the link"
    );
    assert!(
        fx.file("a.txt")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_no_temp_files(&fx.folder);
}

/// Undoing a one-file restore that brought a file back removes it: into the trash, or, when the
/// trash can't take it (its folder is a file here), deleted for good since the history holds
/// it; `deleted` names it then, and only then.
#[test]
fn undoing_a_restore_that_brought_a_file_back_says_whether_it_went_for_good() {
    for blocked in [false, true] {
        let name = if blocked {
            "restore-one-undo-for-good"
        } else {
            "restore-one-undo-trashed"
        };
        let (fx, mut matter) = versioned(name);
        support::remove_file(fx.file("b.txt"));
        let chosen = only("b.txt");
        matter
            .restore(&restore_request(3, RestoreFiles::Only(&chosen), 4, 5))
            .unwrap();
        assert_eq!(fx.read("b.txt"), b"bee, edited");
        if blocked {
            support::write(
                fx.root().with_file_name("trash"),
                "a file, not a trash folder",
            );
        }

        let mut back = restore_request(4, RestoreFiles::Only(&chosen), 6, 7);
        back.undoes = Some(cid(5));
        let undone = matter.restore(&back).unwrap();
        assert_eq!(undone.removed, chosen);
        assert!(!fx.exists("b.txt"));
        let for_good: &[MatterPath] = if blocked { &chosen } else { &[] };
        assert_eq!(undone.deleted, for_good, "blocked: {blocked}");
    }
}

/// A folder made where the chosen file goes after the plan checked it refuses the restore as the
/// clash it is, at the last check before writing: after the outside-edits version was kept (so
/// not by the plan), and never as `matter.file_locked`. Nothing is written. The probe is asked
/// about the file three times: by the walk's stat, by the walk's read of the open file (the hand
/// edit is read), then by the plan's `writable`, after its clash check; the swap comes at the
/// third.
#[test]
fn a_folder_made_after_the_plan_is_refused_as_a_clash_before_writing() {
    let (fx, mut matter) = versioned("restore-one-folder-after-plan");
    fx.write("a.txt", "three, by hand");
    fx.probe.swap_for_folder(fx.file("a.txt"), 3);
    let chosen = only("a.txt");

    let err = matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 4, 5))
        .unwrap_err();
    assert!(fx.probe.swapped(), "the probe made the folder");
    assert!(
        matches!(&err, Error::FileFolderClash { path, in_way }
            if path == &chosen[0] && in_way == &chosen[0]),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::MatterFileFolderClash);
    // the outside-edits version was kept first: the plan passed, and the last check refused
    assert_eq!(ids(&matter), [cid(1), cid(3), cid(4)]);
    assert!(fx.file("a.txt").is_dir());
    assert_eq!(fx.read("b.txt"), b"bee, edited");
    assert_no_temp_files(&fx.folder);
}
