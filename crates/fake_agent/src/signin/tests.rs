//! `user_home`: the home folder the fake reads, as Codex and Claude Code read it.

use std::ffi::OsString;
use std::path::PathBuf;

use super::user_home;

/// A lookup over `pairs`, as the environment answers it.
fn env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    let pairs = pairs.to_vec();
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
    }
}

#[test]
fn windows_reads_userprofile_first_then_home() {
    let both = env(&[
        ("HOME", "/home/git-bash"),
        ("USERPROFILE", r"C:\Users\lawyer"),
    ]);
    assert_eq!(
        user_home(both, true),
        Some(PathBuf::from(r"C:\Users\lawyer"))
    );
    let home_only = env(&[("HOME", "/home/git-bash")]);
    assert_eq!(
        user_home(home_only, true),
        Some(PathBuf::from("/home/git-bash"))
    );
    let empty = env(&[("HOME", "/home/git-bash"), ("USERPROFILE", "")]);
    assert_eq!(
        user_home(empty, true),
        Some(PathBuf::from("/home/git-bash"))
    );
    assert_eq!(user_home(env(&[]), true), None);
}

#[test]
fn unix_reads_home_alone() {
    let both = env(&[
        ("HOME", "/home/lawyer"),
        ("USERPROFILE", r"C:\Users\lawyer"),
    ]);
    assert_eq!(user_home(both, false), Some(PathBuf::from("/home/lawyer")));
    let profile_only = env(&[("USERPROFILE", r"C:\Users\lawyer")]);
    assert_eq!(user_home(profile_only, false), None);
}
