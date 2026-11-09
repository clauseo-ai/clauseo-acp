//! Detection never writes into the lawyer's own Claude or Codex config: every file under
//! `~/.claude`, `~/.claude.json` and `~/.codex` (path, kind, size, mtime and a SHA-256 of the
//! contents) is the same after a status check that detects as before it, and nothing appears.
//! The fake CLIs write what the real ones do on any run (`fake_agent`'s `first_meet`: Claude's
//! global config and its backup, Codex's `tmp/arg0`), so a detection run on the lawyer's
//! folders fails this. Each agent is checked signed out and with a stand-in login (the fake's
//! marker for Claude, a dummy `auth.json` for Codex; never a real credential).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clauseo_agents::testing::write_file;
use clauseo_agents::{Found, LoginFrom, SigninState};
use clauseo_protocol::AgentId;
use sha2::{Digest, Sha256};

use crate::support::{Fixture, claude, codex};

/// One entry of a snapshot: kind, size, mtime and, for a file, its contents' SHA-256.
type Entry = (&'static str, u64, Option<SystemTime>, Option<Vec<u8>>);

/// Every entry under `root` (itself included), by path relative to it, never following a link.
fn walk(root: &Path, at: &Path, into: &mut BTreeMap<PathBuf, Entry>) {
    let Ok(meta) = std::fs::symlink_metadata(at) else {
        return;
    };
    let rel = at.strip_prefix(root).unwrap().to_path_buf();
    let modified = meta.modified().ok();
    if meta.file_type().is_symlink() {
        into.insert(rel, ("link", meta.len(), modified, None));
    } else if meta.is_dir() {
        into.insert(rel, ("dir", 0, modified, None));
        for entry in std::fs::read_dir(at).unwrap() {
            walk(root, &entry.unwrap().path(), into);
        }
    } else {
        let hash = Sha256::digest(std::fs::read(at).unwrap()).to_vec();
        into.insert(rel, ("file", meta.len(), modified, Some(hash)));
    }
}

/// The lawyer's Claude and Codex config: `~/.claude`, `~/.claude.json` and `~/.codex`.
fn snapshot(fx: &Fixture) -> BTreeMap<PathBuf, Entry> {
    let home = fx.user_home();
    let mut all = BTreeMap::new();
    for name in [".claude", ".claude.json", ".codex"] {
        walk(&home, &home.join(name), &mut all);
    }
    all
}

/// Runs the status check for `agent` (Clauseo signed in to nothing, so it detects) and checks
/// that the lawyer's config is as it was, then gives the state.
fn detect_unchanged(fx: &Fixture, agent: &AgentId) -> SigninState {
    let before = snapshot(fx);
    let (agents, events) = fx.start_for(agent.clone());
    agents.check_status(agent).unwrap();
    let state = events.state();
    drop(agents);
    assert_eq!(
        snapshot(fx),
        before,
        "detection changed the lawyer's config"
    );
    assert!(
        !fx.user_home().join(".codex/tmp").exists(),
        "a Codex run wrote ~/.codex/tmp"
    );
    // Claude's run's own config folder is gone after it.
    let scratch = fx.marker(&claude(), "detect-scratch");
    let left = std::fs::read_dir(&scratch).map_or(0, Iterator::count);
    assert_eq!(left, 0, "a detection folder was left in {scratch:?}");
    state
}

fn claude_found() -> SigninState {
    SigninState::LoginFound {
        from: LoginFrom::ClaudeCode,
        email: Some("lawyer@example.com".to_owned()),
    }
}

fn no_login() -> SigninState {
    SigninState::SignedOut {
        found: Some(Found::NoLogin),
    }
}

fn codex_found() -> SigninState {
    SigninState::LoginFound {
        from: LoginFrom::Codex,
        email: None,
    }
}

#[test]
fn claude_detection_signed_out_writes_nothing_in_the_lawyer_s_home() {
    let fx = Fixture::new("nowrite-claude-out", "ok-after:0");
    assert_eq!(detect_unchanged(&fx, &claude()), no_login());
    assert!(!fx.user_home().join(".claude.json").exists());
    assert!(!fx.user_home().join(".claude").exists());
}

#[test]
fn claude_detection_with_a_login_writes_nothing_in_the_lawyer_s_home() {
    let fx = Fixture::new("nowrite-claude-in", "ok-after:0");
    let home = fx.user_home();
    write_file(
        &home.join(".claude/fake-signin-marker"),
        br#"{"authMethod":"claude.ai","email":"lawyer@example.com"}"#,
    )
    .unwrap();
    write_file(&home.join(".claude.json"), br#"{"numStartups":3}"#).unwrap();
    write_file(&home.join(".claude/settings.json"), br#"{"theme":"dark"}"#).unwrap();
    assert_eq!(detect_unchanged(&fx, &claude()), claude_found());
}

#[test]
fn codex_detection_signed_out_writes_nothing_in_the_lawyer_s_home() {
    let fx = Fixture::new("nowrite-codex-out", "ok-after:0");
    assert_eq!(detect_unchanged(&fx, &codex()), no_login());
    assert!(!fx.user_home().join(".codex").exists());
}

#[test]
fn codex_detection_with_a_login_file_writes_nothing_in_the_lawyer_s_home() {
    let fx = Fixture::new("nowrite-codex-in", "ok-after:0");
    write_file(&fx.user_home().join(".codex/auth.json"), b"{}").unwrap();
    write_file(&fx.user_home().join(".codex/config.toml"), b"").unwrap();
    assert_eq!(detect_unchanged(&fx, &codex()), codex_found());
    // No Codex process was started on the lawyer's folder: every run had Clauseo's home.
    for run in fx.launches() {
        assert!(
            run.env.contains_key(std::ffi::OsStr::new("CODEX_HOME")),
            "{:?}",
            run.args
        );
    }
}

/// A link standing at `~/.codex/auth.json` is no login found: only a regular file is.
#[cfg(unix)]
#[test]
fn a_link_at_codex_s_login_file_is_no_login_found() {
    let fx = Fixture::new("nowrite-codex-link", "ok-after:0");
    let target = fx.data.path().join("elsewhere.json");
    write_file(&target, b"{}").unwrap();
    write_file(&fx.user_home().join(".codex/config.toml"), b"").unwrap();
    clauseo_agents::testing::symlink(&target, &fx.user_home().join(".codex/auth.json")).unwrap();
    assert_eq!(detect_unchanged(&fx, &codex()), no_login());
}

/// With `CODEX_HOME` set in the lawyer's environment their Codex keeps its login there, and
/// "Use it" links `~/.codex`'s file only: nothing is found, whichever folder holds a file, and
/// neither folder changes.
#[test]
fn a_set_codex_home_finds_no_login() {
    let mut fx = Fixture::new("nowrite-codex-home", "ok-after:0");
    let moved = fx.data.path().join("lawyer-codex-home");
    fx.config
        .signin
        .base_env
        .insert("CODEX_HOME".into(), moved.clone().into_os_string());
    write_file(&moved.join("auth.json"), b"{}").unwrap();
    write_file(&fx.user_home().join(".codex/auth.json"), b"{}").unwrap();
    let mut before = BTreeMap::new();
    walk(&moved, &moved, &mut before);
    assert_eq!(detect_unchanged(&fx, &codex()), no_login());
    let mut after = BTreeMap::new();
    walk(&moved, &moved, &mut after);
    assert_eq!(after, before);
}
