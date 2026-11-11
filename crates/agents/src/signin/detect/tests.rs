//! Detection's moved-folder rule: `CLAUDE_CONFIG_DIR` and `CODEX_HOME` in the lawyer's
//! environment.

use std::collections::BTreeMap;
use std::ffi::OsString;

use super::folder_moved;

fn env(pairs: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn a_set_folder_variable_moves_the_login_and_an_empty_one_does_not() {
    for name in ["CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
        assert!(!folder_moved(
            &env(&[("HOME", "/home/lawyer")]),
            name,
            false
        ));
        assert!(folder_moved(&env(&[(name, "/srv/agent")]), name, false));
        assert!(!folder_moved(&env(&[(name, "")]), name, false));
        // Windows reads names in any case; Unix does not.
        let lower = name.to_ascii_lowercase();
        assert!(folder_moved(&env(&[(&lower, "C:\\agent")]), name, true));
        assert!(!folder_moved(&env(&[(&lower, "/srv/agent")]), name, false));
    }
    // One agent's variable never moves the other's folder.
    assert!(!folder_moved(
        &env(&[("CODEX_HOME", "/srv/codex")]),
        "CLAUDE_CONFIG_DIR",
        false
    ));
}
