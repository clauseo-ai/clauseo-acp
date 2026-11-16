//! The install folder's walk on a real NTFS volume: a plain chain is read; a junction at a
//! middle folder, at the last folder, or at either file's name is refused, never followed; a
//! folder at a file's name or a second hard link is refused; and nothing in the folder changes.

use super::*;
use crate::test_support::TempDir;
use crate::windows::test_junction::junction;

/// `<temp>\<test>\a\b\c` holding the two files, and the folder itself.
fn chain(test: &str) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new(test);
    let _exe = dir.file(r"a\b\c\clauseo-sandbox-setup.exe", "MZ exe");
    let _sig = dir.file(r"a\b\c\clauseo-sandbox-setup.exe.sig", "sig");
    let folder = dunce::canonicalize(dir.path().join(r"a\b\c")).unwrap();
    (dir, folder)
}

#[test]
fn a_plain_chain_is_read() {
    let (_dir, folder) = chain("refresh-walk-plain");
    let sources = read_sources(&folder).unwrap();
    assert_eq!(sources.exe, b"MZ exe");
    assert_eq!(sources.sig, b"sig");
}

#[test]
fn a_junction_at_a_middle_folder_is_refused() {
    let (dir, _folder) = chain("refresh-walk-middle");
    // `x\b` is a junction to `a\b`: the same files, reached through a reparse point.
    let _x = dir.dir("x");
    junction(&dir.path().join(r"x\b"), &dir.path().join(r"a\b")).unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let through = root.join(r"x\b\c");
    assert!(std::fs::metadata(through.join("clauseo-sandbox-setup.exe")).is_ok());
    let refused = read_sources(&through);
    assert!(
        matches!(
            refused,
            Err((SourceFile::Folder, SourceError::ReparsePoint(_)))
        ),
        "{refused:?}"
    );
}

#[test]
fn a_junction_at_the_last_folder_is_refused() {
    let (dir, _folder) = chain("refresh-walk-last");
    junction(&dir.path().join("y"), &dir.path().join(r"a\b\c")).unwrap();
    let through = dunce::canonicalize(dir.path()).unwrap().join("y");
    let refused = read_sources(&through);
    assert!(
        matches!(
            refused,
            Err((SourceFile::Folder, SourceError::ReparsePoint(_)))
        ),
        "{refused:?}"
    );
}

#[test]
fn a_junction_at_a_file_name_or_a_second_link_is_refused() {
    let dir = TempDir::new("refresh-walk-files");
    let target = dir.dir("elsewhere");
    let folder = dir.dir("install");
    junction(&folder.join("clauseo-sandbox-setup.exe"), &target).unwrap();
    let _sig = dir.file(r"install\clauseo-sandbox-setup.exe.sig", "sig");
    let folder = dunce::canonicalize(folder).unwrap();
    let refused = read_sources(&folder);
    assert!(
        matches!(
            refused,
            Err((SourceFile::Exe, SourceError::ReparsePoint(_)))
        ),
        "{refused:?}"
    );
    let (dir, folder) = chain("refresh-walk-linked");
    let _other = dir.hard_link("other.sig", &folder.join("clauseo-sandbox-setup.exe.sig"));
    assert_eq!(
        read_sources(&folder),
        Err((SourceFile::Sig, SourceError::Linked(2)))
    );
}

#[test]
fn a_folder_at_a_file_name_is_refused_as_not_a_file() {
    let dir = TempDir::new("refresh-walk-folder");
    let _exe = dir.dir(r"install\clauseo-sandbox-setup.exe");
    let _sig = dir.file(r"install\clauseo-sandbox-setup.exe.sig", "sig");
    let folder = dunce::canonicalize(dir.path().join("install")).unwrap();
    assert_eq!(
        read_sources(&folder),
        Err((SourceFile::Exe, SourceError::NotRegular))
    );
}

#[test]
fn a_missing_file_or_folder_is_missing() {
    let (_dir, folder) = chain("refresh-walk-missing");
    assert_eq!(
        read_sources(&folder.join("gone")),
        Err((SourceFile::Folder, SourceError::Missing))
    );
    let empty = folder.parent().unwrap().to_owned();
    assert_eq!(
        read_sources(&empty),
        Err((SourceFile::Exe, SourceError::Missing))
    );
}
