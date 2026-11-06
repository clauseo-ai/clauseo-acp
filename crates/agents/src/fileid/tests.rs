//! `same_file` on every OS: a hard link is the same file, another file is not, a link to the
//! file is not, and a missing file is an error.

use super::{file_stamp, same_file};
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

#[test]
fn a_stamp_changes_with_another_file_under_the_name() {
    let dir = TestDir::new("fileid-stamp");
    let login = dir.file("login", b"one");
    let first = file_stamp(&login).unwrap().unwrap();
    assert_eq!(
        file_stamp(&login).unwrap(),
        Some(first),
        "unchanged: the same stamp"
    );
    // Another file renamed over the name: another id.
    let other = dir.file("other", b"two");
    write::rename(&other, &login).unwrap();
    let swapped = file_stamp(&login).unwrap().unwrap();
    assert_ne!(swapped, first);
    assert_ne!(swapped.id, first.id);
    assert!(file_stamp(&dir.0.join("gone")).is_err());
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

/// Whether a test that needs a privilege it lacks may skip: only off CI. `ci` is the `CI`
/// variable, which GitHub's runners set to `true`; set and not empty, `false` or `0`, it is CI,
/// where Windows runners can make symbolic links, so the test must run there.
fn may_skip_unprivileged(ci: Option<&std::ffi::OsStr>) -> bool {
    ci.is_none_or(|ci| ci.is_empty() || ci == "false" || ci == "0")
}

#[test]
fn only_a_local_run_may_skip_for_want_of_a_privilege() {
    use std::ffi::OsStr;
    assert!(may_skip_unprivileged(None));
    for local in ["", "false", "0"] {
        assert!(may_skip_unprivileged(Some(OsStr::new(local))), "{local:?}");
    }
    for ci in ["true", "1", "yes"] {
        assert!(!may_skip_unprivileged(Some(OsStr::new(ci))), "{ci:?}");
    }
}

/// Skips (passes with a note) when this account may not make symbolic links, on a local run
/// alone: with `CI` set it fails with the error, so Windows CI always proves this.
#[cfg(windows)]
#[test]
fn a_symlink_is_never_the_same_file() {
    let dir = TestDir::new("fileid-symlink");
    let original = dir.file("login", b"marker");
    let link = dir.0.join("link");
    if let Err(err) = write::symlink_file(&original, &link) {
        let ci = std::env::var_os("CI");
        assert!(
            may_skip_unprivileged(ci.as_deref()),
            "CI must make symbolic links to prove same_file: {err}"
        );
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
