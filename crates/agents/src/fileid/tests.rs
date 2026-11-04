//! `same_file` on every OS: a hard link is the same file, another file is not, a link to the
//! file is not, and a missing file is an error.

use super::same_file;
use crate::write::{self, TestDir};

#[test]
fn a_hard_link_is_the_same_file() {
    let dir = TestDir::new("fileid-hard");
    let original = dir.file("home/login", b"marker");
    let link = dir.0.join("link");
    write::hard_link(&original, &link).unwrap();
    assert!(same_file(&original, &link).unwrap());
    assert!(same_file(&link, &original).unwrap());
    assert!(same_file(&original, &original).unwrap());
}

#[test]
fn another_file_is_not_the_same_file() {
    let dir = TestDir::new("fileid-other");
    let one = dir.file("one", b"same bytes");
    let two = dir.file("two", b"same bytes");
    assert!(!same_file(&one, &two).unwrap());
}

#[test]
fn a_missing_file_is_an_error() {
    let dir = TestDir::new("fileid-missing");
    let one = dir.file("one", b"x");
    let gone = dir.0.join("gone");
    assert!(same_file(&one, &gone).is_err());
    assert!(same_file(&gone, &one).is_err());
}

#[cfg(unix)]
#[test]
fn a_symlink_is_never_the_same_file() {
    let dir = TestDir::new("fileid-symlink");
    let original = dir.file("login", b"marker");
    let link = dir.0.join("link");
    write::symlink(&original, &link).unwrap();
    assert!(!same_file(&original, &link).unwrap());
    assert!(!same_file(&link, &original).unwrap());
    assert!(
        !same_file(&link, &link).unwrap(),
        "a link compares as its own entry"
    );
}

/// Skips (passes with a note) when this account may not make symbolic links.
#[cfg(windows)]
#[test]
fn a_symlink_is_never_the_same_file() {
    let dir = TestDir::new("fileid-symlink");
    let original = dir.file("login", b"marker");
    let link = dir.0.join("link");
    if let Err(err) = write::symlink_file(&original, &link) {
        eprintln!("skipped: this account cannot make symbolic links ({err})");
        return;
    }
    assert!(!same_file(&original, &link).unwrap());
    assert!(!same_file(&link, &original).unwrap());
    assert!(
        !same_file(&link, &link).unwrap(),
        "a reparse point is never the same file"
    );
}
