//! Claude's detection as a Mac: the fixture's config says macOS and its `security` is a fake
//! shell script (Unix) that records each call and exits as told, never Apple's. The query is
//! attributes only; it never starts a CLI and writes nothing into the lawyer's config.
//! - its exact arguments and environment, and what each exit means;
//! - when the lawyer's environment moves Claude's store, no query (Claude Code's rule for
//!   `CLAUDE_CONFIG_DIR` and `CLAUDE_SECURESTORAGE_CONFIG_DIR`);
//! - a recorded reuse is kept when the query could not tell, and stopped on a definite 44;
//! - one query per 10 minutes, and "Look for my Claude Code login" asks afresh;
//! - with the login keychain locked, the background status check runs no Claude at all.
#![cfg(unix)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use clauseo_agents::testing::write_file;
use clauseo_agents::{Clock, Found, LoginFrom, Os, SigninState};

use crate::detect_nowrite::{CLAUDE_LOGIN, detect_with_queries, found, no_login};
use crate::support::{Fixture, claude, now};

/// The fixture as a Mac, with a fake `security` exiting `code`; gives its call record.
fn mac(label: &str, code: i32) -> (Fixture, PathBuf) {
    let mut fx = Fixture::new(label, "ok-after:0");
    fx.config.platform.os = Os::Darwin;
    let record = fx.data.path().join("security-called");
    let program = fx.data.path().join("fake-security");
    fx.config.signin.security_program = Some(program);
    fx.config
        .signin
        .base_env
        .insert(OsString::from("USER"), OsString::from("lawyer"));
    exits(&fx, code);
    (fx, record)
}

/// Makes the fixture's fake `security` exit `code` from now on, recording each call.
fn exits(fx: &Fixture, code: i32) {
    let record = fx.data.path().join("security-called");
    let program = fx.data.path().join("fake-security");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\nexit {code}\n",
        record.display()
    );
    write_file(&program, script.as_bytes()).unwrap();
    clauseo_agents::testing::set_mode(&program, 0o755).unwrap();
}

/// How many times the fake `security` ran.
fn calls(record: &std::path::Path) -> usize {
    std::fs::read_to_string(record).map_or(0, |text| text.matches("find-generic-password").count())
}

fn set(fx: &mut Fixture, name: &str, value: &str) {
    fx.config.signin.base_env.insert(name.into(), value.into());
}

/// The Keychain item there, no file: the login is found, by one attributes-only query with
/// exactly these arguments and only `HOME`, and the lawyer's config is unchanged.
#[test]
fn a_mac_keychain_item_is_found_by_one_attributes_only_query() {
    let (fx, record) = mac("keychain-found", 0);
    let (state, queries) = detect_with_queries(&fx, &claude());
    assert_eq!(state, found(LoginFrom::ClaudeCode));
    assert_eq!(queries.len(), 1, "{queries:?}");
    assert_eq!(
        queries[0].args,
        [
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-a",
            "lawyer"
        ]
    );
    let home = fx.user_home().into_os_string();
    assert_eq!(
        queries[0].env.keys().collect::<Vec<_>>(),
        [&OsString::from("HOME")]
    );
    assert_eq!(queries[0].env.get(&OsString::from("HOME")), Some(&home));
    assert_eq!(calls(&record), 1);
    assert!(!fx.user_home().join(".claude").exists());
}

/// At first detection 44, and any end that could not tell, are no login found.
#[test]
fn a_mac_query_that_does_not_exit_0_finds_no_login() {
    for code in [44, 1] {
        let (fx, record) = mac("keychain-absent", code);
        let (state, queries) = detect_with_queries(&fx, &claude());
        assert_eq!(state, no_login(), "exit {code}");
        assert_eq!(queries.len(), 1, "exit {code}");
        assert_eq!(calls(&record), 1, "exit {code}");
    }
}

/// Claude Code's store rule: `CLAUDE_SECURESTORAGE_CONFIG_DIR` set to a folder, or unset with
/// `CLAUDE_CONFIG_DIR` set, moves the login (a suffixed item, another folder): nothing is found,
/// even with a file in `~/.claude`, and no query runs at all.
#[test]
fn a_moved_claude_store_on_a_mac_runs_no_query() {
    let cases: [&[(&str, &str)]; 3] = [
        &[("CLAUDE_CONFIG_DIR", "/srv/lawyer-claude")],
        &[("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/srv/lawyer-store")],
        &[
            ("CLAUDE_CONFIG_DIR", ""),
            ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/srv/lawyer-store"),
        ],
    ];
    for vars in cases {
        for with_file in [false, true] {
            let (mut fx, record) = mac("keychain-moved", 0);
            for (name, value) in vars {
                set(&mut fx, name, value);
            }
            if with_file {
                write_file(&fx.user_home().join(CLAUDE_LOGIN), b"{}").unwrap();
            }
            let (state, queries) = detect_with_queries(&fx, &claude());
            assert_eq!(state, no_login(), "{vars:?} file {with_file}");
            assert!(queries.is_empty(), "{vars:?}: {queries:?}");
            assert_eq!(calls(&record), 0, "{vars:?}");
        }
    }
}

/// `CLAUDE_SECURESTORAGE_CONFIG_DIR=""` is Claude Code's default store (`~/.claude`, the
/// unsuffixed item) whatever `CLAUDE_CONFIG_DIR` says, the store "Use it" reads: the query runs.
#[test]
fn an_empty_storage_folder_is_the_default_store_and_is_queried() {
    let (mut fx, record) = mac("keychain-empty-store", 0);
    set(&mut fx, "CLAUDE_CONFIG_DIR", "/srv/lawyer-claude");
    set(&mut fx, "CLAUDE_SECURESTORAGE_CONFIG_DIR", "");
    let (state, queries) = detect_with_queries(&fx, &claude());
    assert_eq!(state, found(LoginFrom::ClaudeCode));
    assert_eq!(queries.len(), 1, "{queries:?}");
    assert_eq!(calls(&record), 1);
}

/// The file there on a Mac: found by the stat, with no query.
#[test]
fn a_mac_login_file_is_found_with_no_query() {
    let (fx, record) = mac("keychain-file", 44);
    write_file(&fx.user_home().join(CLAUDE_LOGIN), b"{}").unwrap();
    let (state, queries) = detect_with_queries(&fx, &claude());
    assert_eq!(state, found(LoginFrom::ClaudeCode));
    assert!(queries.is_empty(), "{queries:?}");
    assert_eq!(calls(&record), 0);
}

/// Linux keeps the stat alone: a `security` that would say "found" is never asked.
#[test]
fn linux_runs_no_query() {
    let (mut fx, record) = mac("keychain-linux", 0);
    fx.config.platform.os = Os::Linux;
    let (state, queries) = detect_with_queries(&fx, &claude());
    assert_eq!(state, no_login());
    assert!(queries.is_empty(), "{queries:?}");
    assert_eq!(calls(&record), 0);
}

/// A recorded reuse whose query could not tell (exit 1 here; a timeout or a failed start are the
/// same) is kept: the record stays and the state is not `ReusedSignedOut`. At start nothing is
/// shown yet, so the card shows Sign in while it checks again in the background.
#[test]
fn a_recorded_reuse_is_kept_when_the_query_cannot_tell() {
    let (fx, record) = mac("keychain-reuse-unknown", 1);
    let source = fx.marker(&claude(), "login-source");
    write_file(&source, b"reused\n").unwrap();
    let (agents, events) = fx.start_for(claude());
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), SigninState::SignedOut { found: None });
    drop(agents);
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "reused\n");
    // The retry within the check took the kept answer: one query.
    assert_eq!(calls(&record), 1);
}

/// A recorded reuse whose query says 44, "could not be found", stops: `ReusedSignedOut`, and
/// the record goes.
#[test]
fn a_recorded_reuse_stops_on_a_definite_44() {
    let (fx, record) = mac("keychain-reuse-absent", 44);
    let source = fx.marker(&claude(), "login-source");
    write_file(&source, b"reused\n").unwrap();
    let (agents, events) = fx.start_for(claude());
    agents.check_status(&claude()).unwrap();
    assert_eq!(
        events.state(),
        SigninState::SignedOut {
            found: Some(Found::ReusedSignedOut)
        }
    );
    drop(agents);
    assert!(!source.exists());
    assert_eq!(calls(&record), 1);
}

/// A clock whose monotonic reading a test moves.
struct StepClock(AtomicU64);

impl Clock for StepClock {
    fn now(&self) -> SystemTime {
        now()
    }

    fn monotonic(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

/// Status checks ask the Keychain at most once per 10 minutes (a locked keychain may show its
/// unlock dialog for the query, so it can show at most once in that time); "Look for my Claude
/// Code login" asks afresh.
#[test]
fn the_keychain_is_asked_at_most_once_per_10_minutes() {
    let (mut fx, record) = mac("keychain-memo", 44);
    let clock = Arc::new(StepClock(AtomicU64::new(1000)));
    fx.config.clock = Arc::<StepClock>::clone(&clock);
    let (agents, events) = fx.start_for(claude());
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), no_login());
    assert_eq!(calls(&record), 1);
    clock.0.fetch_add(9 * 60, Ordering::SeqCst);
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), no_login());
    assert_eq!(calls(&record), 1, "asked again within 10 minutes");
    clock.0.fetch_add(2 * 60, Ordering::SeqCst);
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), no_login());
    assert_eq!(calls(&record), 2, "not asked again after 10 minutes");
    exits(&fx, 0);
    agents.find_login(&claude()).unwrap();
    assert_eq!(events.state(), found(LoginFrom::ClaudeCode));
    assert_eq!(calls(&record), 3, "Look for my login did not ask afresh");
    drop(agents);
}

/// The login keychain locked: the background status check starts no Claude run and no query
/// (Claude's own `security -w` would show the unlock dialog), keeps a recorded reuse, and the
/// card shows Sign in while it checks again later; unlocked, the same check runs as before.
#[test]
fn a_locked_keychain_runs_no_background_claude_and_keeps_the_state() {
    let (mut fx, record) = mac("keychain-locked", 0);
    fx.config.signin.keychain_locked = Some(true);
    let source = fx.marker(&claude(), "login-source");
    write_file(&source, b"reused\n").unwrap();
    let (agents, events) = fx.start_for(claude());
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), SigninState::SignedOut { found: None });
    drop(agents);
    assert!(fx.launches().is_empty(), "{:?}", fx.launches());
    assert_eq!(calls(&record), 0);
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "reused\n");

    let (mut fx, record) = mac("keychain-unlocked", 44);
    fx.config.signin.keychain_locked = Some(false);
    assert_eq!(detect_with_queries(&fx, &claude()).0, no_login());
    assert_eq!(calls(&record), 1);
}

/// Locked or not, the lawyer's own "Look for my Claude Code login" runs: the guard is for the
/// checks the app starts by itself.
#[test]
fn look_for_my_login_runs_with_the_keychain_locked() {
    let (mut fx, record) = mac("keychain-locked-find", 0);
    fx.config.signin.keychain_locked = Some(true);
    let (agents, events) = fx.start_for(claude());
    agents.find_login(&claude()).unwrap();
    assert_eq!(events.state(), found(LoginFrom::ClaudeCode));
    drop(agents);
    assert_eq!(calls(&record), 1);
    assert_ne!(fx.launches(), Vec::new());
}
