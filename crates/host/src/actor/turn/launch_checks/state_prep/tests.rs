//! Tests of the lawyer's login lookup: the `CODEX_HOME` name match.

use super::env_value;
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
