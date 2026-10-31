//! Files and folders the user's account cannot read or write: every such failure is an
//! `Error::Io` saying why, never a finding that the file is missing, unchanged or written.

#![cfg(unix)]

use std::io;
use std::path::{Path, PathBuf};

use clauseo_matters::Error;
use clauseo_protocol::ErrorCode;

use crate::fixture::{
    Fixture, added, assert_no_temp_files, cid, created, deleted, meta, tools, undo,
};
use crate::support;

/// Sets `path`'s mode while alive and puts back `0o755` when dropped, so the test folder can be
/// removed even when an assertion fails.
pub(crate) struct Mode(PathBuf);

impl Mode {
    pub(crate) fn set(path: impl Into<PathBuf>, mode: u32) -> Self {
        let path = path.into();
        support::set_mode(&path, mode);
        Self(path)
    }
}

impl Drop for Mode {
    fn drop(&mut self) {
        support::set_mode(&self.0, 0o755);
    }
}

/// The OS error of an `Error::Io`, panicking on any other error.
fn io_kind(err: &Error) -> (io::ErrorKind, &Path) {
    assert_eq!(err.code(), ErrorCode::MatterCheckpointFailed, "{err:?}");
    match err {
        Error::Io { source, path, .. } => (source.kind(), path),
        other => panic!("not an Io error: {other:?}"),
    }
}

/// A matter folder whose own folder cannot be looked at (no search permission on its parent)
/// fails to open with an I/O error, not as a missing folder the lawyer must locate.
#[test]
fn open_fails_when_the_folder_cannot_be_looked_at() {
    let mut fx = Fixture::new("unreadable-open-folder");
    fx.folder = fx.dir.path().join("locked").join("matter");
    fx.write("a.txt", "a");
    drop(fx.import());

    let _locked = Mode::set(fx.dir.path().join("locked"), 0o000);
    let err = fx.open(&fx.folder).unwrap_err();
    assert_eq!(io_kind(&err).0, io::ErrorKind::PermissionDenied, "{err:?}");
}

/// A `matter.json` that cannot be read fails the open with an I/O error, not as a folder
/// without a marker.
#[test]
fn open_fails_when_matter_json_cannot_be_read() {
    let fx = Fixture::new("unreadable-open-json");
    fx.write("a.txt", "a");
    drop(fx.import());

    let _locked = Mode::set(fx.file(".clauseo/matter.json"), 0o000);
    let err = fx.open(&fx.folder).unwrap_err();
    assert_eq!(io_kind(&err).0, io::ErrorKind::PermissionDenied, "{err:?}");
}

/// A folder that does not read keeps only its own files as they were: a file deleted
/// elsewhere in the matter is still reported deleted.
#[test]
fn a_deletion_beside_a_folder_that_does_not_read_is_reported() {
    let fx = Fixture::new("unreadable-keep-only-its-own");
    fx.write("a.txt", "a");
    fx.write("sub/b.txt", "b");
    let mut matter = fx.import();

    support::remove_file(fx.file("a.txt"));
    let _locked = Mode::set(fx.file("sub"), 0o000);
    let changes = created(matter.before_turn(&meta(2)).unwrap(), 2);
    assert_eq!(changes, [deleted("a.txt", b"a")]);
}

/// A file the walk lists but cannot stat (its folder lists but cannot be searched) fails the
/// checkpoint; it is never left out as if deleted.
#[test]
fn a_listed_file_that_cannot_be_stat_fails_the_checkpoint() {
    let fx = Fixture::new("unreadable-stat");
    fx.write("a.txt", "a");
    fx.write("sub/b.txt", "b");
    let mut matter = fx.import();

    let locked = Mode::set(fx.file("sub"), 0o444);
    let err = matter.before_turn(&meta(2)).unwrap_err();
    assert_eq!(io_kind(&err).0, io::ErrorKind::PermissionDenied, "{err:?}");
    assert_eq!(matter.timeline().len(), 1);
    drop(locked);
    assert_eq!(matter.before_turn(&meta(3)).unwrap().outcome.id(), cid(1));
}

/// `edited_since` fails with an I/O error when a target's folder cannot be searched: it never
/// counts that file as missing or as unchanged.
#[test]
fn edited_since_fails_when_a_targets_folder_cannot_be_searched() {
    let fx = Fixture::new("unreadable-edited-folder");
    fx.write("keep.txt", "keep");
    let mut matter = fx.import();
    fx.write("sub/new.txt", "new");
    let changes = created(
        matter
            .after_turn(&meta(3), &tools(&["sub/new.txt"]))
            .unwrap(),
        3,
    );
    assert_eq!(changes, [added("sub/new.txt", b"new")]);

    let _locked = Mode::set(fx.file("sub"), 0o000);
    let err = matter.edited_since(cid(1), cid(3)).unwrap_err();
    assert_eq!(io_kind(&err).0, io::ErrorKind::PermissionDenied, "{err:?}");
}

/// `edited_since` fails with an I/O error when a target edited since cannot be read: it never
/// counts that file as unchanged.
#[test]
fn edited_since_fails_when_a_target_cannot_be_read() {
    let fx = Fixture::new("unreadable-edited-file");
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    fx.write("a.txt", "two");
    created(matter.after_turn(&meta(3), &tools(&["a.txt"])).unwrap(), 3);

    // Edited since, so the stat cache cannot vouch for it and it must be read.
    fx.write("a.txt", "three, by the lawyer");
    let _locked = Mode::set(fx.file("a.txt"), 0o000);
    let err = matter.edited_since(cid(1), cid(3)).unwrap_err();
    assert_eq!(io_kind(&err).0, io::ErrorKind::PermissionDenied, "{err:?}");
}

/// An undo that cannot delete a file (its folder is read-only) fails, and the file is still
/// there: it never reports the file undone.
#[test]
fn undo_fails_when_a_file_cannot_be_deleted() {
    let fx = Fixture::new("unreadable-undo-delete");
    fx.write("keep.txt", "keep");
    let mut matter = fx.import();
    fx.write("sub/new.txt", "new");
    created(
        matter
            .after_turn(&meta(3), &tools(&["sub/new.txt"]))
            .unwrap(),
        3,
    );

    let locked = Mode::set(fx.file("sub"), 0o555);
    let err = undo(&mut matter, (1, 3), &["sub/new.txt"], (4, 5)).unwrap_err();
    assert_eq!(io_kind(&err).0, io::ErrorKind::PermissionDenied, "{err:?}");
    drop(locked);
    assert_eq!(fx.read("sub/new.txt"), b"new");
}

/// An undo that cannot write a file beside its target (the folder is read-only) fails with
/// the OS's reason, permission denied, and leaves the target as it was.
#[test]
fn undo_says_why_it_cannot_write_into_a_folder() {
    let fx = Fixture::new("unreadable-undo-write");
    fx.write("sub/a.txt", "one");
    let mut matter = fx.import();
    fx.write("sub/a.txt", "two");
    created(
        matter.after_turn(&meta(3), &tools(&["sub/a.txt"])).unwrap(),
        3,
    );

    let locked = Mode::set(fx.file("sub"), 0o555);
    let err = undo(&mut matter, (1, 3), &["sub/a.txt"], (4, 5)).unwrap_err();
    assert_eq!(io_kind(&err).0, io::ErrorKind::PermissionDenied, "{err:?}");
    drop(locked);
    assert_eq!(fx.read("sub/a.txt"), b"two");
    assert_no_temp_files(&fx.folder);
}

/// An undo that must make a folder again but cannot (its parent is read-only) fails with the
/// OS's reason, permission denied, naming the folder it could not make.
#[test]
fn undo_says_why_it_cannot_make_a_folder() {
    let fx = Fixture::new("unreadable-undo-mkdir");
    fx.write("sub/gone/a.txt", "one");
    let mut matter = fx.import();
    support::remove_dir_all(fx.file("sub/gone"));
    let changes = created(
        matter
            .after_turn(&meta(3), &tools(&["sub/gone/a.txt"]))
            .unwrap(),
        3,
    );
    assert_eq!(changes, [deleted("sub/gone/a.txt", b"one")]);

    let locked = Mode::set(fx.file("sub"), 0o555);
    let err = undo(&mut matter, (1, 3), &["sub/gone/a.txt"], (4, 5)).unwrap_err();
    let (kind, at) = io_kind(&err);
    assert_eq!(kind, io::ErrorKind::PermissionDenied, "{err:?}");
    assert_eq!(at, fx.file("sub/gone"));
    drop(locked);
    assert!(!fx.exists("sub/gone"));
}
