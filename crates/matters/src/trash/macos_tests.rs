//! The real Trash, once: a file goes there through `NSFileManager` and is removed from it
//! again. No Finder, so no Automation prompt.

use std::io::Write as _;

use super::{OsTrash, Trash, TrashError};
use crate::test_dir::TestDir;
use crate::write;

#[test]
fn a_file_goes_to_the_trash_and_the_test_takes_it_out() {
    let dir = TestDir::new("trash-mac-real");
    // A name of this run's own, so the entry in the Trash is this test's.
    let name = format!("clauseo-trash-test-{}.txt", std::process::id());
    let file = dir.path().join(&name);
    write::create_new(&file)
        .unwrap()
        .write_all(b"to the Trash")
        .unwrap();

    let went = OsTrash.trash(&file);

    let went = went
        .unwrap()
        .expect("NSFileManager says where the file went");
    let bytes = std::fs::read(&went);
    // The test's own entry leaves the Trash before any assert can fail.
    write::remove_file(&went).unwrap();
    assert!(!file.exists(), "gone from the folder");
    assert_eq!(bytes.unwrap(), b"to the Trash");
    let in_trash = went.to_string_lossy().into_owned();
    assert!(in_trash.contains(".Trash"), "{in_trash}");
    assert!(in_trash.contains("clauseo-trash-test-"), "{in_trash}");
}

#[test]
fn a_missing_file_is_not_found() {
    let dir = TestDir::new("trash-mac-missing");

    let err = OsTrash.trash(&dir.path().join("absent.txt")).unwrap_err();

    assert!(
        matches!(&err, TrashError::Failed(err) if err.kind() == std::io::ErrorKind::NotFound),
        "{err:?}"
    );
}
