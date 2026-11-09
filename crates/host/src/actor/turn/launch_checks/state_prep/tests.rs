//! Tests of the lawyer's login lookup: the `CODEX_HOME` name match and the file comparison.

use super::{env_value, is_lawyers_file};
use crate::pipeline::tests::support::TempDir;
use crate::write::write_file;
use std::ffi::OsString;

fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn codex_home_is_found_ignoring_case_only_on_windows() {
    let vars = env(&[("Path", "x"), ("codex_home", "/lower")]);
    assert_eq!(
        env_value(&vars, "CODEX_HOME", true),
        Some(OsString::from("/lower"))
    );
    assert_eq!(env_value(&vars, "CODEX_HOME", false), None);
}

#[test]
fn an_exact_name_matches_on_every_os_and_the_first_entry_wins() {
    let vars = env(&[("CODEX_HOME", "/first"), ("Codex_Home", "/second")]);
    for windows in [false, true] {
        assert_eq!(
            env_value(&vars, "CODEX_HOME", windows),
            Some(OsString::from("/first"))
        );
    }
    assert_eq!(
        env_value(&env(&[("CODEX_HOMES", "/x")]), "CODEX_HOME", true),
        None
    );
}

#[test]
fn a_byte_identical_copy_of_the_lawyer_s_login_is_the_lawyer_s() {
    let dir = TempDir::new("state-prep-copy");
    let lawyers = dir.path().join("lawyers.json");
    let copy = dir.path().join("copy.json");
    write_file(&lawyers, b"{\"tokens\":1}").unwrap();
    write_file(&copy, b"{\"tokens\":1}").unwrap();
    assert!(is_lawyers_file(&copy, std::slice::from_ref(&lawyers)));
    // Different tokens, a missing file or an empty pair: not the lawyer's.
    write_file(&copy, b"{\"tokens\":2}").unwrap();
    assert!(!is_lawyers_file(&copy, std::slice::from_ref(&lawyers)));
    assert!(!is_lawyers_file(
        &dir.path().join("none.json"),
        std::slice::from_ref(&lawyers)
    ));
    write_file(&copy, b"").unwrap();
    write_file(&lawyers, b"").unwrap();
    assert!(!is_lawyers_file(&copy, &[lawyers]));
}
