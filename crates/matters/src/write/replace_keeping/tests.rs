//! The steps after a `ReplaceFileW` failure, with the call simulated by plain renames so they run
//! on every OS: what each documented code leaves on disk, and what is done after it.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use super::{Failed, Step, after_failed_put_back, next_step, replace_keeping};
use crate::test_dir::TestDir;
use crate::write::{Lost, create_new, remove_file, rename};

/// `ERROR_UNABLE_TO_REMOVE_REPLACED`, `ERROR_UNABLE_TO_MOVE_REPLACEMENT` and `..._2`.
const UNABLE_TO_REMOVE_REPLACED: i32 = 1175;
const UNABLE_TO_MOVE_REPLACEMENT: i32 = 1176;
const UNABLE_TO_MOVE_REPLACEMENT_2: i32 = 1177;

fn file(dir: &TestDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    create_new(&path).unwrap().write_all(bytes).unwrap();
    path
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

fn names(dir: &TestDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn code(code: i32) -> io::Error {
    io::Error::from_raw_os_error(code)
}

#[test]
fn each_failure_goes_by_what_is_on_disk() {
    assert_eq!(next_step(false, true, true), Step::Done);
    // 1175, and 1176 with a backup name: both files keep their names.
    assert_eq!(next_step(true, true, false), Step::Rename);
    // 1177: the old file under the backup name.
    assert_eq!(next_step(true, false, true), Step::PutBack);
    // Gone with no backup: the new content goes to the free name.
    assert_eq!(next_step(true, false, false), Step::Rename);
}

#[test]
fn a_replace_removes_the_backup_and_loses_nothing() {
    let dir = TestDir::new("replace-keeping-ok");
    let target = file(&dir, "Lease.docx", b"old");
    let temp = file(&dir, "new.tmp", b"new");
    let lost = replace_keeping(&temp, &target, |replaced, replacement, backup| {
        rename(replaced, backup)?;
        rename(replacement, replaced)
    })
    .unwrap();
    assert_eq!(lost, Lost::default());
    assert_eq!(read(&target), b"new");
    assert_eq!(names(&dir), ["Lease.docx"]);
}

/// 1175 and 1176 (with a backup name) moved nothing: the plain rename, the metadata lost, with
/// the code.
#[test]
fn nothing_moved_gets_the_plain_rename() {
    for failure in [UNABLE_TO_REMOVE_REPLACED, UNABLE_TO_MOVE_REPLACEMENT] {
        let dir = TestDir::new("replace-keeping-unmoved");
        let target = file(&dir, "Lease.docx", b"old");
        let temp = file(&dir, "new.tmp", b"new");
        let lost = replace_keeping(&temp, &target, |_, _, _| Err(code(failure))).unwrap();
        assert_eq!(lost, Lost::one(Some(failure)));
        assert_eq!(read(&target), b"new");
        assert_eq!(names(&dir), ["Lease.docx"]);
    }
}

/// 1177: the old file is under the backup name; it is put back and the failure returned, with
/// the temp file still there for the caller to remove.
#[test]
fn the_old_file_under_the_backup_name_is_put_back() {
    let dir = TestDir::new("replace-keeping-put-back");
    let target = file(&dir, "Lease.docx", b"old");
    let temp = file(&dir, "new.tmp", b"new");
    let failed = replace_keeping(&temp, &target, |replaced, _, backup| {
        rename(replaced, backup)?;
        Err(code(UNABLE_TO_MOVE_REPLACEMENT_2))
    })
    .unwrap_err();
    let Failed::Unchanged(err) = failed else {
        panic!("{failed:?}");
    };
    assert_eq!(err.raw_os_error(), Some(UNABLE_TO_MOVE_REPLACEMENT_2));
    assert_eq!(read(&target), b"old");
    assert_eq!(names(&dir), ["Lease.docx", "new.tmp"]);
}

/// The target gone and the rename failing too: the target is not there, so the failure says so.
#[test]
fn a_target_gone_after_a_failed_rename_is_told_apart() {
    let dir = TestDir::new("replace-keeping-gone");
    let target = file(&dir, "Lease.docx", b"old");
    let missing = dir.path().join("missing.tmp");
    let failed = replace_keeping(&missing, &target, |replaced, _, _| {
        remove_file(replaced)?;
        Err(code(UNABLE_TO_MOVE_REPLACEMENT))
    })
    .unwrap_err();
    assert!(matches!(failed, Failed::TargetGone(_)), "{failed:?}");
}

/// A staged rewrite whose old file went to the backup name and could not be put back keeps its
/// temp file (the new content) and says the folder changed. The put-back is made to fail by a
/// read-only folder, which root ignores: skipped then.
#[cfg(unix)]
#[test]
fn a_staged_rewrite_keeps_its_temp_file_when_the_target_is_gone() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = TestDir::new("replace-keeping-staged");
    let target = file(&dir, "Lease.docx", b"old");
    let staged = crate::write::stage(&target, &mut |file: &mut std::fs::File| {
        file.write_all(b"new")
            .map_err(|err| crate::Error::io("write", "x", err))
    })
    .unwrap();
    let temp = staged.temp.clone().unwrap();
    let folder = dir.path().to_owned();
    let simulate: &crate::write::ReplaceFn = &move |replaced, _, backup| {
        rename(replaced, backup)?;
        let locked = std::fs::Permissions::from_mode(0o555);
        crate::write::set_permissions(&folder, locked)?;
        Err(code(UNABLE_TO_MOVE_REPLACEMENT_2))
    };
    let failed = staged.rename_with(Some(simulate)).unwrap_err();
    let open = std::fs::Permissions::from_mode(0o755);
    crate::write::set_permissions(dir.path(), open).unwrap();
    if !failed.changed {
        eprintln!("skipped: the read-only folder did not stop the put-back (root)");
        assert_eq!(read(&target), b"old");
        return;
    }
    let message = failed.error.to_string();
    assert!(
        message.contains("Lease.docx.clauseo-0.orig.tmp"),
        "{message}"
    );
    assert!(message.contains(".clauseo-0.tmp"), "{message}");
    assert_eq!(
        read(&dir.path().join("Lease.docx.clauseo-0.orig.tmp")),
        b"old"
    );
    assert!(!target.exists());
    assert_eq!(read(&temp), b"new");
}

/// A restore of a deleted file: no `ReplaceFileW`, and a failed rename leaves the folder as it
/// was, so the temp file may go.
#[test]
fn a_target_missing_before_gets_the_plain_rename_alone() {
    let dir = TestDir::new("replace-keeping-missing");
    let target = dir.path().join("Lease.docx");
    let missing = dir.path().join("missing.tmp");
    let failed = replace_keeping(&missing, &target, |_, _, _| panic!("called")).unwrap_err();
    assert!(matches!(failed, Failed::Unchanged(_)), "{failed:?}");

    let temp = file(&dir, "new.tmp", b"new");
    let lost = replace_keeping(&temp, &target, |_, _, _| panic!("called")).unwrap();
    assert_eq!(lost, Lost::default());
    assert_eq!(read(&target), b"new");
}

/// The backup is named for its file, so the next open can put it back.
#[test]
fn the_backup_is_named_for_its_file() {
    let dir = TestDir::new("replace-keeping-name");
    let target = file(&dir, "Lease.docx", b"old");
    let temp = file(&dir, "new.tmp", b"new");
    replace_keeping(&temp, &target, |replaced, replacement, backup| {
        assert_eq!(backup, dir.path().join("Lease.docx.clauseo-0.orig.tmp"));
        rename(replaced, backup)?;
        rename(replacement, replaced)
    })
    .unwrap();
}

/// After 1177 and a put-back that failed: the new content goes in place; the backup stays.
#[test]
fn a_failed_put_back_puts_the_new_content_in_place() {
    let dir = TestDir::new("replace-keeping-second-try");
    let backup = file(&dir, "Lease.docx.clauseo-0.orig.tmp", b"old");
    let temp = file(&dir, "new.tmp", b"new");
    let target = dir.path().join("Lease.docx");
    let denied = code(5);
    let lost = after_failed_put_back(&temp, &target, &backup, &denied, Some(1177)).unwrap();
    assert_eq!(lost, Lost::one(Some(1177)));
    assert_eq!(read(&target), b"new");
    assert_eq!(read(&backup), b"old");

    // Both failing: the error names the two files.
    let gone = dir.path().join("gone.tmp");
    let failed = after_failed_put_back(
        &gone,
        &target.with_file_name("B.docx"),
        &backup,
        &denied,
        None,
    )
    .unwrap_err();
    let Failed::TargetGone(err) = failed else {
        panic!("{failed:?}");
    };
    let message = err.to_string();
    assert!(message.contains("Lease.docx.clauseo-0.orig.tmp") && message.contains("gone.tmp"));
}

/// Names of 236 to 255 UTF-16 units: the call gets a backup name the folder takes (the suffix up
/// to 236, the short hashed name from 237), so the old file's metadata is kept, nothing lost,
/// and nothing is left beside the file.
#[test]
fn a_long_name_gets_a_backup_name_the_folder_takes() {
    for len in [236, 237, 240, 255] {
        let dir = TestDir::new("replace-keeping-long");
        let name = format!("{}.docx", "a".repeat(len - 5));
        let target = file(&dir, &name, b"old");
        let temp = file(&dir, "new.tmp", b"new");
        let mut given = None;
        let lost = replace_keeping(&temp, &target, |replaced, replacement, backup| {
            given = backup.file_name().map(std::ffi::OsStr::to_os_string);
            rename(replaced, backup)?;
            rename(replacement, replaced)
        })
        .unwrap();
        let given = given.expect("called").into_string().unwrap();
        assert!(given.encode_utf16().count() <= 255, "{len}: {given}");
        assert_eq!(given.starts_with(".clauseo-"), len > 236, "{len}: {given}");
        assert_eq!(lost, Lost::default(), "{len}");
        assert_eq!(read(&target), b"new");
        assert_eq!(names(&dir), [name]);
    }
}
