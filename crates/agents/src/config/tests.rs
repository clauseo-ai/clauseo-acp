//! The lawyer's home folder: `USERPROFILE` on Windows, as Codex reads it, and `HOME` elsewhere.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use super::lawyer_home_in;
use crate::platform::Os;

fn env(pairs: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn windows_picks_userprofile_over_home() {
    let both = env(&[
        ("HOME", "/home/git-bash"),
        ("USERPROFILE", r"C:\Users\lawyer"),
    ]);
    assert_eq!(
        lawyer_home_in(&both, Os::Win32),
        Some(PathBuf::from(r"C:\Users\lawyer"))
    );
}

#[test]
fn windows_matches_the_name_in_any_case_and_ignores_home() {
    let lower = env(&[("UserProfile", r"C:\Users\lawyer")]);
    assert_eq!(
        lawyer_home_in(&lower, Os::Win32),
        Some(PathBuf::from(r"C:\Users\lawyer"))
    );
    assert_eq!(
        lawyer_home_in(&env(&[("HOME", "/home/x")]), Os::Win32),
        None
    );
}

#[test]
fn unix_picks_home_over_userprofile() {
    let both = env(&[
        ("HOME", "/home/lawyer"),
        ("USERPROFILE", r"C:\Users\lawyer"),
    ]);
    for os in [Os::Linux, Os::Darwin] {
        assert_eq!(
            lawyer_home_in(&both, os),
            Some(PathBuf::from("/home/lawyer"))
        );
        assert_eq!(lawyer_home_in(&env(&[("home", "/home/x")]), os), None);
    }
}

#[test]
fn an_empty_or_missing_home_is_none() {
    assert_eq!(lawyer_home_in(&env(&[("HOME", "")]), Os::Linux), None);
    assert_eq!(
        lawyer_home_in(&env(&[("USERPROFILE", "")]), Os::Win32),
        None
    );
    assert_eq!(lawyer_home_in(&env(&[]), Os::Linux), None);
}
