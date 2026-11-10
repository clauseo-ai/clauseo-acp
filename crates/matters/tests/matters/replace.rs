//! Replacing one file with content made outside the folder (tracked changes accepted or
//! rejected on a private copy): the write and its two checkpoints, the restore of the checkpoint
//! before that undoes it, and the refusals that write nothing.

use std::path::PathBuf;

use clauseo_matters::{CheckpointOutcome, Error, Limits, Matter, ReplaceFile, RestoreFiles};
use clauseo_protocol::{CheckpointKind, ErrorCode, MatterPath};

use crate::fixture::{
    Fixture, assert_no_temp_files, at, cid, created, modified, path, read_at, restore_request,
};
use crate::support;

/// `Lease.docx` "old" imported as checkpoint 1, and the private copies beside the folder: the
/// bytes read (`expected`) and the new content.
fn replacing(name: &str, expected: &[u8]) -> (Fixture, Matter, PathBuf, PathBuf) {
    let fx = Fixture::new(name);
    fx.write("Lease.docx", "old");
    let matter = fx.import();
    let work = fx.dir.path().join("work");
    support::write(work.join("read.docx"), expected);
    support::write(work.join("out.docx"), "new");
    (fx, matter, work.join("read.docx"), work.join("out.docx"))
}

fn request<'a>(
    file: &'a MatterPath,
    expected: &'a std::path::Path,
    new: &'a std::path::Path,
) -> ReplaceFile<'a> {
    ReplaceFile {
        path: file,
        expected,
        new,
        outside_edits: cid(2),
        checkpoint: cid(3),
        at: at(3),
    }
}

#[test]
fn replace_writes_the_file_between_checkpoints_and_a_restore_undoes_it() {
    let (fx, mut matter, expected, new) = replacing("replace-writes", b"old");
    let file = path("Lease.docx");
    let replaced = matter
        .replace_file(&request(&file, &expected, &new))
        .unwrap();
    // The folder had not changed: no outside-edits checkpoint; the import holds the old bytes.
    assert_eq!(replaced.outside_edits, None);
    assert_eq!(replaced.before, cid(1));
    assert_eq!(
        created(replaced.checkpoint.unwrap(), 3),
        [modified("Lease.docx", b"old", b"new")]
    );
    assert_eq!(fx.read("Lease.docx"), b"new");
    let last = *matter.timeline().last().unwrap();
    assert_eq!(last.kind, CheckpointKind::Revisions);
    assert_eq!((last.thread, last.turn), (None, None));
    assert_eq!(read_at(&matter, 1, "Lease.docx"), b"old");
    assert_no_temp_files(&fx.folder);

    let paths = [file.clone()];
    let restored = matter
        .restore(&restore_request(1, RestoreFiles::Only(&paths), 4, 5))
        .unwrap();
    assert_eq!(restored.written, paths);
    assert_eq!(fx.read("Lease.docx"), b"old");
}

/// A failure after the rename (the `revisions` checkpoint's walk meeting a file another program
/// holds, on Windows) does not fail the replace: the file is written, `before` holds the old
/// bytes and a restore of it puts them back; there is no `revisions` checkpoint, and the next
/// checkpoint records the new content.
#[test]
fn a_failure_after_the_rename_still_answers_written_with_its_before() {
    let (fx, mut matter, expected, new) = replacing("replace-after-rename", b"old");
    let file = path("Lease.docx");
    clauseo_matters::test_support::fail_after_replace_write(fx.file("Lease.docx"));
    let replaced = matter
        .replace_file(&request(&file, &expected, &new))
        .unwrap();
    assert_eq!(replaced.before, cid(1));
    assert_eq!(replaced.checkpoint, None);
    assert_eq!(fx.read("Lease.docx"), b"new");
    assert_eq!(matter.timeline().len(), 1);
    assert_no_temp_files(&fx.folder);

    let paths = [file.clone()];
    let restored = matter
        .restore(&restore_request(1, RestoreFiles::Only(&paths), 4, 5))
        .unwrap();
    assert_eq!(restored.written, paths);
    assert_eq!(fx.read("Lease.docx"), b"old");
}

#[test]
fn an_edit_outside_clauseo_is_checkpointed_first_and_is_the_before() {
    let (fx, mut matter, expected, new) = replacing("replace-outside", b"edited");
    fx.write("Lease.docx", "edited");
    support::settle(fx.file("Lease.docx"));
    let file = path("Lease.docx");
    let replaced = matter
        .replace_file(&request(&file, &expected, &new))
        .unwrap();
    assert_eq!(
        replaced.outside_edits.map(|checkpoint| checkpoint.outcome),
        Some(CheckpointOutcome::Created(cid(2)))
    );
    assert_eq!(replaced.before, cid(2));
    assert_eq!(read_at(&matter, 2, "Lease.docx"), b"edited");
    assert_eq!(fx.read("Lease.docx"), b"new");
}

#[test]
fn a_file_that_differs_from_the_bytes_read_is_refused_and_nothing_is_written() {
    let (fx, mut matter, expected, new) = replacing("replace-stale", b"what was read");
    let file = path("Lease.docx");
    let error = matter
        .replace_file(&request(&file, &expected, &new))
        .unwrap_err();
    assert!(
        matches!(&error, Error::ChangedSince { path } if *path == file),
        "{error}"
    );
    assert_eq!(error.code(), ErrorCode::WordRevisionsStale);
    assert_eq!(fx.read("Lease.docx"), b"old");
    assert_eq!(matter.timeline().len(), 1);
    // A missing file is refused alike.
    let gone = path("Gone.docx");
    let error = matter
        .replace_file(&request(&gone, &expected, &new))
        .unwrap_err();
    assert!(matches!(error, Error::ChangedSince { .. }), "{error}");
}

#[test]
fn a_file_open_in_word_is_refused_and_nothing_is_written() {
    let (fx, mut matter, expected, new) = replacing("replace-locked", b"old");
    fx.write("~$Lease.docx", "owner file: open in Word");
    let file = path("Lease.docx");
    let error = matter
        .replace_file(&request(&file, &expected, &new))
        .unwrap_err();
    assert!(
        matches!(&error, Error::FileLocked { paths } if *paths == [file.clone()]),
        "{error}"
    );
    assert_eq!(fx.read("Lease.docx"), b"old");
    assert_eq!(matter.timeline().len(), 1);
    assert_no_temp_files(&fx.folder);
}

#[test]
fn ids_on_the_timeline_or_equal_are_refused() {
    let (_fx, mut matter, expected, new) = replacing("replace-ids", b"old");
    let file = path("Lease.docx");
    let mut reused = request(&file, &expected, &new);
    reused.outside_edits = cid(1);
    assert!(matches!(
        matter.replace_file(&reused),
        Err(Error::DuplicateCheckpoint { .. })
    ));
    let mut same = request(&file, &expected, &new);
    same.checkpoint = cid(2);
    assert!(matches!(
        matter.replace_file(&same),
        Err(Error::DuplicateCheckpoint { .. })
    ));
}

/// A file over the size cap is never read by a walk, so no write of it could be checkpointed:
/// refused as `NotVersioned` (`word.edit_failed`), not as changed (a stale list, retried for
/// ever), and nothing is written.
#[test]
fn a_file_over_the_size_cap_is_refused_as_not_versioned() {
    let mut fx = Fixture::new("replace-over-cap");
    fx.limits = Limits {
        file_cap_bytes: 2,
        import_warn_bytes: 1_000_000,
    };
    fx.write("Lease.docx", "old");
    let mut matter = fx.import();
    let work = fx.dir.path().join("work");
    support::write(work.join("read.docx"), "old");
    support::write(work.join("out.docx"), "n");
    let file = path("Lease.docx");
    let (expected, new) = (work.join("read.docx"), work.join("out.docx"));
    let error = matter
        .replace_file(&request(&file, &expected, &new))
        .unwrap_err();
    assert!(
        matches!(&error, Error::NotVersioned { path } if *path == file),
        "{error}"
    );
    assert_eq!(error.code(), ErrorCode::WordEditFailed);
    assert_eq!(fx.read("Lease.docx"), b"old");
    assert_eq!(matter.timeline().len(), 1);
    // Read alike: refused before it is opened.
    let error = matter.read_local(&file).unwrap_err();
    assert!(matches!(error, Error::NotVersioned { .. }), "{error}");
}

/// `read_local` reads a plain file, and refuses one the probe calls cloud-only without
/// opening it.
#[test]
fn read_local_refuses_a_cloud_only_file() {
    let (fx, matter, _expected, _new) = replacing("replace-read-local", b"old");
    let file = path("Lease.docx");
    assert_eq!(matter.read_local(&file).unwrap(), b"old");
    fx.probe.mark(fx.file("Lease.docx"));
    let error = matter.read_local(&file).unwrap_err();
    assert!(
        matches!(&error, Error::NotVersioned { path } if *path == file),
        "{error}"
    );
    assert_eq!(error.code(), ErrorCode::WordEditFailed);
    fx.probe.unmark(fx.file("Lease.docx"));
    let error = matter.read_local(&path("Gone.docx")).unwrap_err();
    assert!(
        matches!(&error, Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound),
        "{error}"
    );
}
