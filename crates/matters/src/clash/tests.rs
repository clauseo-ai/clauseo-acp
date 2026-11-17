//! The entry in the way, named as the folder spells it.

use clauseo_protocol::MatterPath;

use super::clash;
use crate::test_dir::TestDir;
use crate::write;

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

/// A folder where the file goes, and a file where its folder goes, each by its own name.
#[test]
fn the_entry_in_the_way_is_named() {
    let dir = TestDir::new("clash-named");
    write::create_dir_all(&dir.path().join("a.txt")).unwrap();
    drop(write::create_new(&dir.path().join("sub")).unwrap());
    assert_eq!(
        clash(dir.path(), &path("a.txt")).unwrap(),
        Some(path("a.txt"))
    );
    assert_eq!(
        clash(dir.path(), &path("sub/b.txt")).unwrap(),
        Some(path("sub"))
    );
    assert_eq!(clash(dir.path(), &path("new.txt")).unwrap(), None);
}

/// Unix: the entry a name finds is named as the folder holds it, by its inode, where only
/// another spelling is there (as on APFS); here the found entry's metadata stands in for the
/// stat a case-insensitive folder would answer.
#[cfg(unix)]
#[test]
fn another_spelling_is_named_as_the_folder_holds_it() {
    let dir = TestDir::new("clash-spelled");
    write::create_dir_all(&dir.path().join("memo.docx")).unwrap();
    let metadata = std::fs::symlink_metadata(dir.path().join("memo.docx")).unwrap();
    assert_eq!(
        super::spelled(dir.path(), "Memo.docx", &metadata),
        "memo.docx"
    );
    assert_eq!(
        super::spelled(dir.path(), "memo.docx", &metadata),
        "memo.docx"
    );
}

/// macOS and Windows: the version's `Memo.docx` meets the folder `memo.docx`, which is named.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_folder_spelled_otherwise_is_named_as_on_disk() {
    let dir = TestDir::new("clash-case");
    write::create_dir_all(&dir.path().join("memo.docx")).unwrap();
    assert_eq!(
        clash(dir.path(), &path("Memo.docx")).unwrap(),
        Some(path("memo.docx"))
    );
}
