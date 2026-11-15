//! The lock decision with fake reads: each keychain's `SecKeychainStatus` (`None`: the call
//! failed), the user record's home folder, and which files exist. A real Mac reads 0x7 for an
//! unlocked keychain and 0x2 for a locked one (K0).

use std::path::Path;

use super::decide;

const OPEN: Option<u32> = Some(0x7);
const LOCKED: Option<u32> = Some(0x2);
const HOME: &str = "/Users/lawyer";
const LOGIN_DB: &str = "/Users/lawyer/Library/Keychains/login.keychain-db";
const LOGIN_OLD: &str = "/Users/lawyer/Library/Keychains/login.keychain";

/// `decide` with `files` existing, and `login` as the status of the one keychain opened,
/// which must be at `expected`.
fn run(
    default: Option<u32>,
    home: Option<&str>,
    files: &[&str],
    expected: &str,
    login: Option<u32>,
) -> bool {
    decide(
        default,
        home.map(Path::new),
        |path| files.iter().any(|file| path == Path::new(file)),
        |path| {
            assert_eq!(
                path,
                Path::new(expected),
                "only the login keychain is opened"
            );
            login
        },
    )
}

#[test]
fn default_and_login_unlocked_is_not_locked() {
    assert!(!run(OPEN, Some(HOME), &[LOGIN_DB], LOGIN_DB, OPEN));
}

#[test]
fn a_locked_login_keychain_is_locked_with_the_default_unlocked() {
    assert!(run(OPEN, Some(HOME), &[LOGIN_DB], LOGIN_DB, LOCKED));
}

#[test]
fn a_locked_default_keychain_is_locked() {
    assert!(run(LOCKED, Some(HOME), &[LOGIN_DB], LOGIN_DB, OPEN));
}

#[test]
fn the_older_login_keychain_is_read_when_the_current_one_is_missing() {
    assert!(!run(OPEN, Some(HOME), &[LOGIN_OLD], LOGIN_OLD, OPEN));
    assert!(run(OPEN, Some(HOME), &[LOGIN_OLD], LOGIN_OLD, LOCKED));
}

#[test]
fn the_system_keychain_is_never_read() {
    // K0: System.keychain reads 0x2 (locked) to every user; only the login keychain is opened
    // (`run` fails on any other path), so it never makes the lawyer read as locked.
    assert!(!run(
        OPEN,
        Some(HOME),
        &[LOGIN_DB, "/Library/Keychains/System.keychain"],
        LOGIN_DB,
        OPEN
    ));
}

#[test]
fn a_failed_home_lookup_is_locked() {
    assert!(run(OPEN, None, &[LOGIN_DB], LOGIN_DB, OPEN));
}

#[test]
fn no_login_keychain_file_is_locked() {
    assert!(run(OPEN, Some(HOME), &[], LOGIN_DB, OPEN));
}

#[test]
fn a_failing_call_is_locked() {
    assert!(
        run(None, Some(HOME), &[LOGIN_DB], LOGIN_DB, OPEN),
        "the default's status failed"
    );
    assert!(
        run(OPEN, Some(HOME), &[LOGIN_DB], LOGIN_DB, None),
        "the login keychain's failed"
    );
}
