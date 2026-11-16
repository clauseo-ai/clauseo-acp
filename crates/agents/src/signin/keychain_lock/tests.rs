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

/// What the probe below prints before the record's home folder.
#[cfg(target_os = "macos")]
const PROBE_LINE: &str = "user-record-home:";

/// Set only in the child [`the_home_is_the_user_record_s_with_home_set_elsewhere`] starts.
#[cfg(target_os = "macos")]
const PROBE_VAR: &str = "CLAUSEO_KEYCHAIN_HOME_PROBE";

/// The child's half: prints `user_home_uncached` under the environment the parent gave it.
/// Does nothing in a normal run.
#[cfg(target_os = "macos")]
#[test]
fn user_record_home_probe() {
    if std::env::var_os(PROBE_VAR).is_none() {
        return;
    }
    let home = super::user_home_uncached().expect("the user record has a home folder");
    println!("{PROBE_LINE}{}", home.display());
}

/// `HOME` and `CFFIXED_USER_HOME` pointing elsewhere change nothing: the folder is the user
/// record's. The environment is set on a child (this test binary, running the probe alone), so
/// no other test's environment changes.
#[cfg(target_os = "macos")]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "runs this test binary once more, with its own environment, and reads what it prints"
)]
fn the_home_is_the_user_record_s_with_home_set_elsewhere() {
    let elsewhere = "/private/var/empty/clauseo-not-the-home";
    let exe = std::env::current_exe().unwrap();
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "signin::keychain_lock::tests::user_record_home_probe",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PROBE_VAR, "1")
        .env("HOME", elsewhere)
        .env("CFFIXED_USER_HOME", elsewhere)
        .output()
        .unwrap();
    assert!(output.status.success(), "the probe failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let printed = stdout
        .lines()
        // libtest prints `test … ... ` before the probe's line on the same line.
        .find_map(|line| line.split_once(PROBE_LINE).map(|(_, home)| home))
        .expect("the probe printed the home folder");
    let record = nix::unistd::User::from_uid(nix::unistd::getuid())
        .unwrap()
        .unwrap()
        .dir;
    assert_eq!(Path::new(printed), record);
    assert_ne!(Path::new(printed), Path::new(elsewhere));
}
