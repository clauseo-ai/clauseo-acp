//! macOS: write-back on APFS, where names ignore case (by default) and a stat can fail other
//! than "not found".

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::Arc;

use clauseo_protocol::MatterPath;

use super::{Names, Op, respelled, writable};
use crate::history::Oid;
use crate::test_dir::TestDir;
use crate::types::{Limits, Retention, Settings};
use crate::{Error, OsProbe, write};

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

fn put(file: &Path) {
    write::create_dir_all(file.parent().unwrap()).unwrap();
    drop(write::create_new(file).unwrap());
}

/// A stat through a file (`f.txt/b`, `ENOTDIR`) is an error, not an absent file.
#[test]
fn a_stat_through_a_file_is_an_error() {
    let dir = TestDir::new("writeback-mac-notdir");
    put(&dir.path().join("f.txt"));
    let through = path("f.txt/b");
    let found = Names::default().metadata(dir.path(), &through);
    assert!(
        matches!(&found, Err(Error::Io { op: "stat", .. })),
        "{found:?}"
    );
    let other = Names::default().other_spelling(dir.path(), &through);
    assert!(
        matches!(&other, Err(Error::Io { op: "stat", .. })),
        "{other:?}"
    );
}

/// A file in a folder that cannot be searched (mode 000, `EACCES`) is not writable: the
/// check fails instead of reading the refused stat as "nothing there".
#[test]
fn a_path_whose_stat_is_refused_is_an_error() {
    let dir = TestDir::new("writeback-mac-eacces");
    let locked = dir.path().join("locked");
    put(&locked.join("a.txt"));
    write::set_permissions(&locked, Permissions::from_mode(0o000)).unwrap();
    // Root reads through the mode; then there is no refusal to test.
    let refused = std::fs::symlink_metadata(locked.join("a.txt")).is_err();
    let settings = Settings {
        matters_root: dir.path().join("root"),
        refused_trees: Vec::new(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
    };
    let result = writable(dir.path(), &settings, &path("locked/a.txt"));
    write::set_permissions(&locked, Permissions::from_mode(0o755)).unwrap();
    if refused {
        assert!(
            matches!(&result, Err(Error::Io { op: "stat", .. })),
            "{result:?}"
        );
    }
}

/// Writing `memo.txt` would replace `Memo.txt`, another file to the history; deleting an
/// unrelated file in the same step does not make that write ours.
#[test]
fn a_write_onto_another_files_other_spelling_clashes() {
    let dir = TestDir::new("writeback-mac-respelled");
    put(&dir.path().join("Memo.txt"));
    put(&dir.path().join("x.txt"));
    assert!(
        std::fs::symlink_metadata(dir.path().join("memo.txt")).is_ok(),
        "needs a case-insensitive volume (APFS's default)"
    );
    let write_memo = Op::Write(path("memo.txt"), Oid([0; 20]));
    let delete_x = Op::Delete(path("x.txt"));
    let clashes = respelled(&mut Names::default(), dir.path(), &[&write_memo, &delete_x]).unwrap();
    assert_eq!(clashes, [path("memo.txt")]);
}

/// Undoing a case-only rename deletes `Brief.txt` and writes `brief.txt`: the file the write
/// finds is the one the step deletes (same inode), so nothing clashes.
#[test]
fn undoing_a_case_only_rename_does_not_clash() {
    let dir = TestDir::new("writeback-mac-case-rename");
    put(&dir.path().join("Brief.txt"));
    let delete = Op::Delete(path("Brief.txt"));
    let write_back = Op::Write(path("brief.txt"), Oid([0; 20]));
    let clashes = respelled(&mut Names::default(), dir.path(), &[&delete, &write_back]).unwrap();
    assert!(clashes.is_empty(), "{clashes:?}");
}
