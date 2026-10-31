//! Undo on a case- or normalisation-insensitive file system (APFS by default, NTFS for case):
//! a deletion never removes the file the undo just put back. Each test first asks the test
//! folder's file system whether it finds a name by its variant. On macOS (and, for case, on
//! Windows) it must, so these tests never pass there without the insensitive case; elsewhere a
//! sensitive file system skips them.

use std::path::Path;

use clauseo_matters::Matter;

use crate::fixture::{
    Fixture, added, assert_no_temp_files, assert_none, cid, created, deleted, meta, path, renamed,
    tools, undo,
};
use crate::support;

/// Whether the file system holding `fx` finds `variant` once `name` exists. Panics when it
/// does not and `required` says this OS must.
pub(crate) fn insensitive(fx: &Fixture, name: &str, variant: &str, required: bool) -> bool {
    let dir = fx.dir.path().join("probe");
    support::write(dir.join(name), "probe");
    let found = dir.join(variant).symlink_metadata().is_ok();
    support::remove_dir_all(&dir);
    assert!(
        found || !required,
        "{} finds no {variant:?} for {name:?}: this test needs the OS's default file system",
        dir.display()
    );
    if !found {
        eprintln!("skipped: {} tells {name:?} from {variant:?}", dir.display());
    }
    found
}

pub(crate) const CASE_REQUIRED: bool = cfg!(any(target_os = "macos", windows));

/// The names in the matter folder besides `.clauseo`, sorted.
pub(crate) fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != ".clauseo")
        .collect();
    names.sort();
    names
}

/// Imports `fx` holding `old` with `bytes`, renames it to `new` in a turn, and undoes it.
fn rename_and_undo(fx: &Fixture, old: &str, new: &str, bytes: &[u8]) -> Matter {
    fx.write(old, bytes);
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    support::rename(fx.file(old), fx.file(new));
    assert_eq!(names(&fx.folder), [new]);
    let changes = created(matter.after_turn(&meta(3), &tools(&[new])).unwrap(), 3);
    assert_eq!(changes, [renamed(old, new, bytes)]);
    assert_none(&matter.edited_since(cid(1), cid(3)).unwrap());

    let undone = undo(&mut matter, (1, 3), &[new], (4, 5)).unwrap();
    assert_eq!(undone.restored, [path(new)]);
    assert_eq!(undone.outside_edits, None);
    assert_none(&undone.edited_since);
    assert_eq!(names(&fx.folder), [old]);
    assert_eq!(fx.read(old), bytes);
    assert_eq!(created(undone.checkpoint, 5), [renamed(new, old, bytes)]);
    assert_no_temp_files(&fx.folder);
    matter
}

#[test]
fn undo_of_case_only_rename_restores_the_old_name() {
    let fx = Fixture::new("undo-case-rename");
    if !insensitive(&fx, "Brief.txt", "brief.txt", CASE_REQUIRED) {
        return;
    }
    rename_and_undo(&fx, "Brief.txt", "brief.txt", b"the brief");
}

#[test]
fn undo_of_delete_and_recreate_in_another_case_restores_the_old_file() {
    let fx = Fixture::new("undo-case-recreate");
    if !insensitive(&fx, "memo.txt", "Memo.txt", CASE_REQUIRED) {
        return;
    }
    fx.write("memo.txt", "old");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    support::remove_file(fx.file("memo.txt"));
    fx.write("Memo.txt", "new");
    let step = ["Memo.txt", "memo.txt"];
    let changes = created(matter.after_turn(&meta(3), &tools(&step)).unwrap(), 3);
    assert_eq!(
        changes,
        [added("Memo.txt", b"new"), deleted("memo.txt", b"old")]
    );
    assert_none(&matter.edited_since(cid(1), cid(3)).unwrap());

    let undone = undo(&mut matter, (1, 3), &step, (4, 5)).unwrap();
    assert_eq!(undone.restored, [path("Memo.txt"), path("memo.txt")]);
    assert_none(&undone.edited_since);
    assert_eq!(names(&fx.folder), ["memo.txt"]);
    assert_eq!(fx.read("memo.txt"), b"old");
    assert_eq!(
        created(undone.checkpoint, 5),
        [deleted("Memo.txt", b"new"), added("memo.txt", b"old")]
    );
    assert_no_temp_files(&fx.folder);
}

#[test]
fn undo_of_nfc_to_nfd_rename_restores_the_nfc_name() {
    let fx = Fixture::new("undo-nfd-rename");
    let nfc = "R\u{e9}sum\u{e9}.txt";
    let nfd = "Re\u{301}sume\u{301}.txt";
    if !insensitive(&fx, nfc, nfd, cfg!(target_os = "macos")) {
        return;
    }
    rename_and_undo(&fx, nfc, nfd, b"the CV");
}

#[test]
fn undo_keeps_a_file_spelled_otherwise_made_after_the_step() {
    let fx = Fixture::new("undo-case-unrelated");
    if !insensitive(&fx, "Memo.txt", "memo.txt", CASE_REQUIRED) {
        return;
    }
    fx.write("Memo.txt", "the step deletes this");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    support::remove_file(fx.file("Memo.txt"));
    let changes = created(
        matter.after_turn(&meta(3), &tools(&["Memo.txt"])).unwrap(),
        3,
    );
    assert_eq!(changes, [deleted("Memo.txt", b"the step deletes this")]);
    // The lawyer then makes an unrelated file whose name differs only in case.
    fx.write("memo.txt", "the lawyer's own memo");

    let undone = undo(&mut matter, (1, 3), &["Memo.txt"], (4, 5)).unwrap();
    assert_none(&undone.restored);
    assert_eq!(undone.kept, [path("Memo.txt")]);
    assert_eq!(
        created(undone.outside_edits.unwrap(), 4),
        [added("memo.txt", b"the lawyer's own memo")]
    );
    assert_eq!(
        undone.checkpoint.outcome,
        clauseo_matters::CheckpointOutcome::Reused(cid(4))
    );
    assert_eq!(names(&fx.folder), ["memo.txt"]);
    assert_eq!(fx.read("memo.txt"), b"the lawyer's own memo");
    assert_no_temp_files(&fx.folder);
}
