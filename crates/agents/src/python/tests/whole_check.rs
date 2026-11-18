//! The whole-check's tests: a runtime whose files changed, went missing or were cut short.

use super::*;

/// An installed runtime in `data`, made writable so a test can change it.
#[cfg(unix)]
fn installed(data: &TestDir) -> (PathBuf, PythonEntry, PythonArchive, PathBuf) {
    let (path, entry, archive) = stage(data, &fake_archive("echo 3.13.18"));
    let dir = runtime_dir(&data.0, &archive);
    install(&path, &entry, &archive, &dir, &launcher()).unwrap();
    assert!(python_runtime_valid(&dir, &archive, Os::Linux));
    write::make_writable(&dir).unwrap();
    (path, entry, archive, dir)
}

/// The limit of the whole-check, which compares sizes only: a file changed to other bytes of
/// the same size stays valid. Only the repair check, which hashes every byte, finds it.
#[cfg(unix)]
#[test]
fn a_same_size_change_fails_only_the_contents_check_and_a_missing_manifest_the_receipt() {
    let data = TestDir::new("python-contents");
    let (_, _, archive, dir) = installed(&data);
    let os_py = dir.join("lib/python3.13/os.py");
    write::set_mode(&os_py, 0o644).unwrap();
    write::write_atomic(&os_py, b"# OS\n").unwrap();
    whole::forget(&dir);
    assert!(python_runtime_valid(&dir, &archive, Os::Linux));
    assert!(!contents_match(&dir), "a changed file");
    write::remove_file(&dir.join(PYTHON_MANIFEST)).unwrap();
    assert!(
        !python_runtime_valid(&dir, &archive, Os::Linux),
        "no manifest"
    );
}

/// A file gone is noticed at the next start (the check's result is kept for the rest of this
/// one): the runtime is then not valid.
#[cfg(unix)]
#[test]
fn a_runtime_missing_one_file_is_not_valid_from_the_next_start() {
    let data = TestDir::new("python-file-gone");
    let (_, _, archive, dir) = installed(&data);
    write::remove_file(&dir.join("lib/python3.13/os.py")).unwrap();
    assert!(
        python_runtime_valid(&dir, &archive, Os::Linux),
        "kept this start"
    );
    whole::forget(&dir);
    assert!(!python_runtime_valid(&dir, &archive, Os::Linux));
}

#[cfg(unix)]
#[test]
fn a_truncated_file_is_not_valid() {
    let data = TestDir::new("python-truncated");
    let (_, _, archive, dir) = installed(&data);
    let os_py = dir.join("lib/python3.13/os.py");
    write::set_mode(&os_py, 0o644).unwrap();
    write::write_atomic(&os_py, b"# o").unwrap();
    whole::forget(&dir);
    assert!(!python_runtime_valid(&dir, &archive, Os::Linux));
    assert!(
        !whole::sizes_match(&dir, "x 0 bin\n"),
        "a folder is no file"
    );
}

#[cfg(unix)]
#[test]
fn a_removed_runtime_folder_is_not_valid_and_installs_again() {
    let data = TestDir::new("python-folder-gone");
    let (path, entry, archive, dir) = installed(&data);
    write::remove_dir_all(&dir).unwrap();
    assert!(!python_runtime_valid(&dir, &archive, Os::Linux));
    install(&path, &entry, &archive, &dir, &launcher()).unwrap();
    assert!(python_runtime_valid(&dir, &archive, Os::Linux));
    assert!(contents_match(&dir));
}
