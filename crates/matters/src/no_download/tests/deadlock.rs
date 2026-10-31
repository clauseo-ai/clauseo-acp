//! A read or a listing that fails as a file or folder evicted after its stat fails under the
//! guard (`EDEADLK`, injected by `fault.rs` on every OS): "not downloaded", like a file the stat
//! found cloud-only, at every caller. Never read again, never an error of the whole call.

use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use clauseo_protocol::{FileChoice, FileDecision, SkippedFile, SkippedFileReason};

use super::{Fixture, cid, meta, path, put, thread, turn};
use crate::no_download::fault;
use crate::types::{CompareSide, Restore, RestoreFiles, UndoRequest};
use crate::{Error, PlaceholderProbe, write};

fn cloud_only(name: &str) -> SkippedFile {
    SkippedFile {
        path: path(name),
        reason: SkippedFileReason::CloudOnly,
    }
}

fn read(file: &std::path::Path) -> Vec<u8> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    write::open_read_nofollow(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

fn files_at(matter: &crate::Matter, checkpoint: clauseo_protocol::CheckpointId) -> Vec<String> {
    matter
        .files_at(checkpoint)
        .unwrap()
        .into_iter()
        .map(|file| file.path.as_str().to_owned())
        .collect()
}

#[test]
fn a_checkpoint_skips_a_file_not_downloaded_at_read_and_reads_it_once() {
    let fx = Fixture::new("no-download-checkpoint", &[("a.txt", b"one")]);
    let (mut matter, _) = fx.import();
    write::append(&fx.file("a.txt"), b" two").unwrap();
    put(&fx.file("new.txt"), b"new");
    fault::fail(&fx.file("a.txt"), 0, ErrorKind::Deadlock);
    fault::fail(&fx.file("new.txt"), 0, ErrorKind::Deadlock);

    let checkpoint = matter.before_turn(&meta(2)).unwrap();

    assert_eq!(
        checkpoint.skipped,
        [cloud_only("a.txt"), cloud_only("new.txt")]
    );
    assert_eq!(checkpoint.changes, []);
    // The versioned file keeps its blob; the new one is not versioned.
    assert_eq!(
        files_at(&matter, matter.last_checkpoint().unwrap()),
        ["a.txt"]
    );
    assert_eq!(fault::failed(&fx.file("a.txt")), 1);
    assert_eq!(fault::failed(&fx.file("new.txt")), 1);
    fault::clear();
}

#[test]
fn another_read_error_still_fails_the_checkpoint() {
    let fx = Fixture::new("no-download-other-error", &[("a.txt", b"one")]);
    let (mut matter, _) = fx.import();
    write::append(&fx.file("a.txt"), b" two").unwrap();
    fault::fail(&fx.file("a.txt"), 0, ErrorKind::PermissionDenied);

    let err = matter.before_turn(&meta(2)).unwrap_err();

    assert!(
        matches!(&err, Error::Io { source, .. } if source.kind() == ErrorKind::PermissionDenied)
    );
    assert!(!err.is_not_downloaded());
    fault::clear();
}

#[test]
fn an_import_counts_a_file_not_downloaded_at_read_as_cloud_only() {
    let fx = Fixture::new(
        "no-download-import",
        &[("a.txt", b"one"), ("b.txt", b"two")],
    );
    fault::fail(&fx.file("b.txt"), 0, ErrorKind::Deadlock);

    let (matter, summary) = fx.import();

    assert_eq!((summary.files, summary.cloud_only), (1, 1));
    assert_eq!(files_at(&matter, cid(1)), ["a.txt"]);
    fault::clear();
}

/// Says cloud-only the second time it is asked about `target`: the walk's stat found it local,
/// the open file's metadata finds it cloud-only.
struct EvictedAtOpen {
    target: std::path::PathBuf,
    asked: AtomicUsize,
}

impl PlaceholderProbe for EvictedAtOpen {
    fn is_cloud_only(&self, path: &std::path::Path, _metadata: &std::fs::Metadata) -> bool {
        path == self.target && self.asked.fetch_add(1, Ordering::Relaxed) == 1
    }
}

#[test]
fn a_file_cloud_only_by_its_open_metadata_is_never_read() {
    let mut fx = Fixture::new(
        "no-download-open-metadata",
        &[("a.txt", b"one"), ("b.txt", b"two")],
    );
    fx.settings.probe = Arc::new(EvictedAtOpen {
        target: fx.file("b.txt"),
        asked: AtomicUsize::new(0),
    });
    // Any read call of b.txt would fail the import.
    fault::fail(&fx.file("b.txt"), 0, ErrorKind::PermissionDenied);

    let (matter, summary) = fx.import();

    assert_eq!((summary.files, summary.cloud_only), (1, 1));
    assert_eq!(files_at(&matter, cid(1)), ["a.txt"]);
    assert_eq!(fault::failed(&fx.file("b.txt")), 0);
    fault::clear();
}

#[test]
fn undo_keeps_a_target_not_downloaded_when_its_outside_edits_walk_reads_it() {
    let fx = Fixture::new("no-download-undo", &[("a.txt", b"one")]);
    let (mut matter, _) = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    write::append(&fx.file("a.txt"), b" agent").unwrap();
    matter
        .after_turn(&meta(3), &std::collections::BTreeSet::new())
        .unwrap();
    // Edited since the step, so the outside-edits walk reads it; evicted by then.
    write::append(&fx.file("a.txt"), b" lawyer").unwrap();
    fault::fail(&fx.file("a.txt"), 0, ErrorKind::Deadlock);
    let decisions = [FileDecision {
        path: path("a.txt"),
        choice: FileChoice::Undo,
    }];
    let request = UndoRequest {
        before: cid(1),
        after: cid(3),
        decisions: &decisions,
        thread: Some(thread()),
        turn: Some(turn(3)),
        outside_edits: cid(4),
        checkpoint: cid(5),
        at: super::at(5),
    };

    let undone = matter.undo(&request).unwrap();

    assert_eq!(undone.kept, [path("a.txt")]);
    assert_eq!(undone.restored, []);
    assert_eq!(read(&fx.file("a.txt")), b"one agent lawyer");
    fault::clear();
}

#[test]
fn restore_and_its_preview_keep_a_file_not_downloaded_at_read() {
    let fx = Fixture::new(
        "no-download-restore",
        &[("a.txt", b"one"), ("b.txt", b"two")],
    );
    let (mut matter, _) = fx.import();
    write::append(&fx.file("a.txt"), b" more").unwrap();
    write::append(&fx.file("b.txt"), b" more").unwrap();
    matter.before_turn(&meta(2)).unwrap();
    write::append(&fx.file("a.txt"), b" again").unwrap();
    fault::fail(&fx.file("a.txt"), 0, ErrorKind::Deadlock);

    let plan = matter.preview_restore(cid(1), RestoreFiles::All).unwrap();
    assert_eq!(plan.kept, [path("a.txt")]);
    assert_eq!(plan.writes.len(), 1);

    let restored = matter
        .restore(&Restore {
            from: cid(1),
            files: RestoreFiles::All,
            outside_edits: cid(3),
            checkpoint: cid(4),
            at: super::at(4),
        })
        .unwrap();
    assert_eq!(restored.kept, [path("a.txt")]);
    assert_eq!(restored.written, [path("b.txt")]);
    assert_eq!(read(&fx.file("a.txt")), b"one more again");
    assert_eq!(read(&fx.file("b.txt")), b"two");
    fault::clear();
}

#[test]
fn compare_with_the_folder_reports_no_change_for_files_not_downloaded() {
    let fx = Fixture::new("no-download-compare", &[("a.txt", b"one")]);
    let (matter, _) = fx.import();
    write::append(&fx.file("a.txt"), b" two").unwrap();
    put(&fx.file("new.txt"), b"new");
    fault::fail(&fx.file("a.txt"), 0, ErrorKind::Deadlock);
    fault::fail(&fx.file("new.txt"), 0, ErrorKind::Deadlock);

    let changes = matter
        .compare(CompareSide::Checkpoint(cid(1)), CompareSide::Folder)
        .unwrap();

    assert_eq!(changes, []);
    fault::clear();
}

#[test]
fn edited_since_does_not_list_a_file_not_downloaded() {
    let fx = Fixture::new("no-download-edited-since", &[("a.txt", b"one")]);
    let (mut matter, _) = fx.import();
    write::append(&fx.file("a.txt"), b" agent").unwrap();
    matter
        .after_turn(&meta(2), &std::collections::BTreeSet::new())
        .unwrap();
    write::append(&fx.file("a.txt"), b" lawyer").unwrap();
    fault::fail(&fx.file("a.txt"), 0, ErrorKind::Deadlock);

    assert_eq!(matter.edited_since(cid(1), cid(2)).unwrap(), []);
    assert_eq!(fault::failed(&fx.file("a.txt")), 1);

    // Downloaded again, it reads as edited.
    fault::clear();
    assert_eq!(
        matter.edited_since(cid(1), cid(2)).unwrap(),
        [path("a.txt")]
    );
}

#[test]
fn a_subfolder_not_downloaded_keeps_its_files_and_the_root_is_an_error() {
    let fx = Fixture::new(
        "no-download-folder",
        &[("a.txt", b"one"), ("sub/b.txt", b"two")],
    );
    let (mut matter, _) = fx.import();
    write::append(&fx.file("sub/b.txt"), b" more").unwrap();
    fault::fail(&fx.file("sub"), 0, ErrorKind::Deadlock);

    let checkpoint = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(checkpoint.changes, []);
    assert_eq!(
        files_at(&matter, matter.last_checkpoint().unwrap()),
        ["a.txt", "sub/b.txt"]
    );
    fault::clear();

    fault::fail(&fx.folder, 0, ErrorKind::Deadlock);
    let err = matter.before_turn(&meta(3)).unwrap_err();
    assert!(err.is_not_downloaded(), "{err}");
    fault::clear();
}

#[test]
fn edited_since_reads_a_file_behind_a_folder_not_downloaded_as_unread() {
    let fx = Fixture::new("no-download-edited-folder", &[("sub/b.txt", b"two")]);
    let (mut matter, _) = fx.import();
    write::append(&fx.file("sub/b.txt"), b" agent").unwrap();
    matter
        .after_turn(&meta(2), &std::collections::BTreeSet::new())
        .unwrap();
    write::append(&fx.file("sub/b.txt"), b" lawyer").unwrap();
    // Listing `sub` to match the name byte for byte fails: the folder is not downloaded.
    fault::fail(&fx.file("sub"), 0, ErrorKind::Deadlock);

    assert_eq!(matter.edited_since(cid(1), cid(2)).unwrap(), []);
    assert_eq!(fault::failed(&fx.file("sub")), 1);

    fault::clear();
    assert_eq!(
        matter.edited_since(cid(1), cid(2)).unwrap(),
        [path("sub/b.txt")]
    );
}

/// Whether the test folder's file system finds `b.txt` under `B.txt` (APFS, NTFS by default).
/// Panics on macOS and Windows when it does not: these tests need the OS's default.
fn case_insensitive(dir: &std::path::Path) -> bool {
    let probe = dir.join("case-probe");
    put(&probe.join("B.txt"), b"probe");
    let found = probe.join("b.txt").symlink_metadata().is_ok();
    write::remove_dir_all(&probe).unwrap();
    assert!(
        found || !cfg!(any(target_os = "macos", windows)),
        "this test needs the OS's default, case-insensitive file system"
    );
    found
}

#[test]
fn undo_keeps_a_write_whose_respelled_delete_was_kept_for_not_downloaded() {
    let fx = Fixture::new("no-download-undo-case", &[("Memo.txt", b"old")]);
    if !case_insensitive(fx.folder.parent().unwrap()) {
        return;
    }
    let (mut matter, _) = fx.import();
    write::remove_file(&fx.file("Memo.txt")).unwrap();
    put(&fx.file("memo.txt"), b"new");
    let checkpoint = matter
        .after_turn(&meta(2), &std::collections::BTreeSet::new())
        .unwrap();
    assert_eq!(checkpoint.changes.len(), 2);
    // Edited since, so the outside-edits walk reads memo.txt; evicted by then.
    write::append(&fx.file("memo.txt"), b" edited").unwrap();
    fault::fail(&fx.file("memo.txt"), 0, ErrorKind::Deadlock);
    let decisions = [
        FileDecision {
            path: path("Memo.txt"),
            choice: FileChoice::Undo,
        },
        FileDecision {
            path: path("memo.txt"),
            choice: FileChoice::Undo,
        },
    ];

    let undone = matter
        .undo(&UndoRequest {
            before: cid(1),
            after: cid(2),
            decisions: &decisions,
            thread: Some(thread()),
            turn: Some(turn(2)),
            outside_edits: cid(3),
            checkpoint: cid(4),
            at: super::at(4),
        })
        .unwrap();

    // The delete of memo.txt is kept (not downloaded), so writing Memo.txt would replace it.
    assert_eq!(undone.kept, [path("Memo.txt"), path("memo.txt")]);
    assert_eq!(undone.restored, []);
    assert_eq!(read(&fx.file("memo.txt")), b"new edited");
    fault::clear();
}
