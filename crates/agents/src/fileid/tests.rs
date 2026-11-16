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

/// Whether `error` (from [`super::win32_io_error`]) reads as a missing file or folder: only Win32
/// `ERROR_FILE_NOT_FOUND` (2) and `ERROR_PATH_NOT_FOUND` (3). The code decides, not `kind()`,
/// because std's kind also reads `ERROR_INVALID_DRIVE` (15), `ERROR_BAD_NETPATH` (53) and
/// `ERROR_BAD_NET_NAME` (67) as `NotFound`; access denied, a sharing violation and a reparse
/// refusal are never missing either. The rule a caller must use; no production caller reads a
/// `same_file` error as missing yet, so it lives here, pinned by the tests below.
#[cfg(windows)]
fn is_missing(error: &std::io::Error) -> bool {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    error.raw_os_error().is_some_and(|code| {
        code == ERROR_FILE_NOT_FOUND.0.cast_signed() || code == ERROR_PATH_NOT_FOUND.0.cast_signed()
    })
}

/// On Windows only `ERROR_FILE_NOT_FOUND` (2) and `ERROR_PATH_NOT_FOUND` (3) read as missing.
/// Access denied (5), a sharing violation (32), a reparse refusal (`ERROR_CANT_ACCESS_FILE`,
/// 1920) and the codes std also reads as `NotFound` (15, 53, 67) keep their Win32 code and are
/// not missing, so a caller that takes "missing" from it never mistakes them for an absent file.
#[cfg(windows)]
#[test]
fn only_a_missing_file_or_path_reads_as_not_found_on_windows() {
    use windows::Win32::Foundation::WIN32_ERROR;
    let io_error = |code: u32| {
        let error = windows::core::Error::from(WIN32_ERROR(code).to_hresult());
        super::win32_io_error(error)
    };
    for code in [2_u32, 3, 15, 53, 67, 5, 32, 1920] {
        let error = io_error(code);
        assert_eq!(
            error.raw_os_error(),
            Some(code.cast_signed()),
            "code {code} keeps its Win32 code"
        );
        assert_eq!(
            is_missing(&error),
            matches!(code, 2 | 3),
            "code {code} is missing only for 2 and 3"
        );
    }
    let not_win32 =
        windows::core::Error::from(windows::core::HRESULT(0x8000_4005_u32.cast_signed()));
    let error = super::win32_io_error(not_win32);
    assert_eq!(error.raw_os_error(), Some(0x8000_4005_u32.cast_signed()));
    assert!(
        !is_missing(&error),
        "an error that is no Win32 error is not missing"
    );
}

/// On Windows a missing file and a missing folder on the path are both missing, by their Win32
/// codes (2 and 3), from `same_file`'s own open.
#[cfg(windows)]
#[test]
fn a_missing_file_or_folder_is_missing_by_its_win32_code_on_windows() {
    let dir = TestDir::new("fileid-win32-missing");
    let one = dir.file("one", b"x");
    let gone = dir.0.join("gone");
    let error = same_file(&one, &gone).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(2));
    assert!(is_missing(&error));
    let under_nothing = dir.0.join("no-such-folder").join("gone");
    let error = same_file(&one, &under_nothing).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(3));
    assert!(is_missing(&error));
}
