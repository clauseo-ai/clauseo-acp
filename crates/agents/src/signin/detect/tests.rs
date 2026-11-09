//! Codex detection's `CODEX_HOME` rule.

use std::collections::BTreeMap;
use std::ffi::OsString;

use super::codex_home_moved;

fn env(pairs: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn a_set_codex_home_moves_the_login_and_an_empty_one_does_not() {
    assert!(!codex_home_moved(&env(&[("HOME", "/home/lawyer")]), false));
    assert!(codex_home_moved(
        &env(&[("CODEX_HOME", "/srv/codex")]),
        false
    ));
    assert!(!codex_home_moved(&env(&[("CODEX_HOME", "")]), false));
    // Windows reads names in any case; Unix does not.
    assert!(codex_home_moved(&env(&[("Codex_Home", "C:\\codex")]), true));
    assert!(!codex_home_moved(
        &env(&[("codex_home", "/srv/codex")]),
        false
    ));
}
