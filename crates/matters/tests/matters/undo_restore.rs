//! Undoing a restore: a restore that takes back an earlier one removes the files that restore
//! added, each only while it holds what the restore wrote, and refuses a checkpoint that is not
//! a restore.

use clauseo_matters::{Error, Matter, Restore, RestoreFiles};
use clauseo_protocol::MatterPath;

use crate::fixture::{
    Fixture, assert_no_temp_files, assert_none, at, cid, created, meta, path, tools,
};
use crate::support;

/// Checkpoint 1 holds `a.txt` "one"; a turn (2, 3) makes it "two" and adds `notes.md`; the
/// file is then removed outside, and restoring 3 takes the outside edits as 4 and brings
/// `notes.md` and nothing else back as the restore 5.
fn restored(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    fx.write("notes.md", "notes");
    created(
        matter
            .after_turn(&meta(3), &tools(&["a.txt", "notes.md"]))
            .unwrap(),
        3,
    );
    support::remove_file(fx.file("notes.md"));
    let done = matter.restore(&request(3, None, 4, 5)).unwrap();
    assert_eq!(done.written, [path("notes.md")]);
    assert_eq!(fx.read("notes.md"), b"notes");
    (fx, matter)
}

fn request(from: u64, undoes: Option<u64>, outside: u64, checkpoint: u64) -> Restore<'static> {
    Restore {
        from: cid(from),
        files: RestoreFiles::All,
        outside_edits: cid(outside),
        checkpoint: cid(checkpoint),
        at: at(checkpoint),
        undoes: undoes.map(cid),
    }
}

fn paths(texts: &[&str]) -> Vec<MatterPath> {
    texts.iter().map(|text| path(text)).collect()
}

#[test]
fn undoing_a_restore_removes_the_file_it_brought_back() {
    let (fx, mut matter) = restored("undo-restore-removes");
    // restoring the version before it, as a plain restore, keeps the file: a restore never deletes
    let plain = matter
        .preview_restore(cid(4), RestoreFiles::All, None)
        .unwrap();
    assert_none(&plain.writes);
    assert_none(&plain.removes);
    assert_eq!(plain.added_since, paths(&["notes.md"]));
    // taking the restore back removes it, and the preview says so first
    let preview = matter
        .preview_restore(cid(4), RestoreFiles::All, Some(cid(5)))
        .unwrap();
    assert_none(&preview.writes);
    assert_eq!(preview.removes, paths(&["notes.md"]));
    assert_none(&preview.added_since);

    let done = matter.restore(&request(4, Some(5), 6, 7)).unwrap();
    assert_eq!(done.removed, paths(&["notes.md"]));
    assert_none(&done.written);
    assert!(!fx.file("notes.md").exists());
    assert_eq!(fx.read("a.txt"), b"two");
    assert_eq!(done.checkpoint.outcome.id(), cid(7));
    assert_eq!(matter.timeline().last().map(|entry| entry.id), Some(cid(7)));
    assert_no_temp_files(&fx.folder);
}

#[test]
fn undoing_a_restore_keeps_a_file_edited_since_and_one_added_since() {
    let (fx, mut matter) = restored("undo-restore-keeps");
    fx.write("notes.md", "the lawyer's notes");
    fx.write("mine.txt", "mine");
    let done = matter.restore(&request(4, Some(5), 6, 7)).unwrap();
    assert_none(&done.removed);
    assert_eq!(done.added_since, paths(&["mine.txt", "notes.md"]));
    assert_eq!(fx.read("notes.md"), b"the lawyer's notes");
    assert_eq!(fx.read("mine.txt"), b"mine");
}

#[test]
fn undoing_a_checkpoint_that_is_not_a_restore_is_refused() {
    let (fx, mut matter) = restored("undo-restore-refused");
    let preview = matter.preview_restore(cid(1), RestoreFiles::All, Some(cid(3)));
    assert!(matches!(preview, Err(Error::UnknownCheckpoint { id }) if id == cid(3)));
    let done = matter.restore(&request(1, Some(3), 6, 7));
    assert!(matches!(done, Err(Error::UnknownCheckpoint { id }) if id == cid(3)));
    assert_eq!(fx.read("notes.md"), b"notes");
}
