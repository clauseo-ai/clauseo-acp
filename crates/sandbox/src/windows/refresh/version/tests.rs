//! Version reading and ordering: semver precedence, the `X.Y.Z.0` form, and the signed
//! version against the exe's `FILEVERSION`.

use std::cmp::Ordering;

use super::*;

#[test]
fn the_signed_version_must_be_the_file_versions_core_with_a_zero_fourth_part() {
    assert!(matches_file_version("0.1.0", [0, 1, 0, 0]));
    assert!(matches_file_version("v1.4.0", [1, 4, 0, 0]));
    assert!(
        matches_file_version("1.4.0-beta.1", [1, 4, 0, 0]),
        "a prerelease's core"
    );
    assert!(
        !matches_file_version("0.1.0", [0, 1, 0, 1]),
        "a non-zero fourth part"
    );
    assert!(!matches_file_version("0.1.0", [0, 1, 1, 0]));
    assert!(!matches_file_version("1.4", [1, 4, 0, 0]), "not semver");
    assert!(!matches_file_version("", [0, 0, 0, 0]));
}

#[test]
fn a_prerelease_is_older_than_its_release() {
    for (recorded, candidate) in [
        ("1.4.0-beta.1", "1.4.0"),
        ("1.4.0-beta.1", "1.4.0-beta.2"),
        ("1.4.0-alpha", "1.4.0-beta"),
        ("1.4.0", "1.4.1-beta.1"),
        ("1.4.0", "1.10.0"),
        ("1.4.0.0", "1.4.0"),
        ("1.4.0", "1.4.0.0"),
        ("1.4.0+build.1", "1.4.0+build.2"),
        ("1.4.0", "1.4.0"),
    ] {
        assert!(
            version_not_older(recorded, candidate),
            "{recorded} -> {candidate}"
        );
    }
    for (recorded, candidate) in [
        ("1.4.0", "1.4.0-beta.1"),
        ("1.4.0-beta.2", "1.4.0-beta.1"),
        ("1.4.0", "1.3.9"),
        ("1.10.0", "1.9.0"),
        // Unreadable on either side.
        ("1.4.0", "1.4.0.1"),
        ("1.4.0.1", "1.5.0"),
        ("", "1.4.0"),
        ("1.4.0", "1.4"),
        ("one", "1.4.0"),
    ] {
        assert!(
            !version_not_older(recorded, candidate),
            "{recorded} -> {candidate}"
        );
    }
}

#[test]
fn equal_precedence_is_told_apart() {
    assert_eq!(compare("1.4.0", "1.4.0.0"), Some(Ordering::Equal));
    assert_eq!(compare("1.4.0+a", "1.4.0+b"), Some(Ordering::Equal));
    assert_eq!(compare("1.4.0-beta.1", "1.4.0"), Some(Ordering::Greater));
    assert_eq!(compare("1.4.0", "1.4.0-beta.1"), Some(Ordering::Less));
    assert_eq!(compare("1.4.0", "x"), None);
}
