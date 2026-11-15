//! Detection's moved-folder rules (`CLAUDE_CONFIG_DIR`, `CLAUDE_SECURESTORAGE_CONFIG_DIR` and
//! `CODEX_HOME` in the lawyer's environment) and its 10-minute Keychain memo.

use std::collections::BTreeMap;
use std::ffi::OsString;

use std::time::Duration;

use super::{KEYCHAIN_RECHECK, KeychainMemo, claude_store_moved, folder_moved};
use crate::signin::reuse::KeychainSeen;

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

/// Claude Code 0.3.293's store rule: `CLAUDE_SECURESTORAGE_CONFIG_DIR` set decides alone (a
/// folder moves the store, `""` is the default store); unset, a non-empty `CLAUDE_CONFIG_DIR`
/// moves it.
#[test]
fn claude_s_store_moves_by_claude_code_s_rule() {
    let storage = "CLAUDE_SECURESTORAGE_CONFIG_DIR";
    let config = "CLAUDE_CONFIG_DIR";
    let cases: [(&[(&str, &str)], bool); 8] = [
        (&[], false),
        (&[(config, "")], false),
        (&[(config, "/srv/c")], true),
        (&[(storage, "")], false),
        (&[(storage, "/srv/s")], true),
        (&[(config, "/srv/c"), (storage, "")], false),
        (&[(config, "/srv/c"), (storage, "/srv/s")], true),
        (&[(config, ""), (storage, "/srv/s")], true),
    ];
    for (vars, moved) in cases {
        assert_eq!(claude_store_moved(&env(vars), false), moved, "{vars:?}");
    }
    // Windows reads names in any case; Unix does not.
    let lower = env(&[(config, "C:\\c"), ("claude_securestorage_config_dir", "")]);
    assert!(!claude_store_moved(&lower, true));
    assert!(claude_store_moved(&lower, false));
}

/// The memo keeps an answer for [`KEYCHAIN_RECHECK`] (10 minutes), then lets the query ask again.
#[test]
fn the_keychain_memo_keeps_an_answer_for_10_minutes() {
    assert_eq!(KEYCHAIN_RECHECK, Duration::from_secs(600));
    let memo = KeychainMemo::default();
    let at = Duration::from_secs(100);
    assert_eq!(memo.fresh(at), None);
    memo.keep(at, KeychainSeen::Unknown);
    assert_eq!(memo.fresh(at), Some(KeychainSeen::Unknown));
    assert_eq!(
        memo.fresh(Duration::from_secs(100 + 599)),
        Some(KeychainSeen::Unknown)
    );
    assert_eq!(memo.fresh(at + KEYCHAIN_RECHECK), None);
    // A clock that went back is never fresh.
    assert_eq!(memo.fresh(Duration::from_secs(1)), None);
}
