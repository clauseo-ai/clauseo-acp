//! Detection never writes into the lawyer's own Claude or Codex config and starts no CLI: every
//! entry under `~/.claude`, `~/.claude.json` and `~/.codex` (path, kind, size, mtime and a
//! SHA-256 of the contents) is the same after a status check that detects as before it, nothing
//! appears, and no run started with the lawyer's folders (every run of the check is in Clauseo's
//! own home). The fake CLIs write what the real ones do on any run (`fake_agent`'s
//! `first_meet`: Claude's global config and its backup, Codex's `tmp/arg0`), so a detection run
//! on the lawyer's folders fails this. Each agent is checked signed out and with a stand-in
//! login file (never a real credential).

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clauseo_agents::testing::write_file;
use clauseo_agents::{Found, LoginFrom, SigninState};
use clauseo_protocol::AgentId;
use sha2::{Digest, Sha256};

use crate::support::{Fixture, claude, codex};

/// Claude's stand-in credential file, under the lawyer's home.
const CLAUDE_LOGIN: &str = concat!(".claude/.credentials", ".json");
/// Codex's stand-in login file, under the lawyer's home.
const CODEX_LOGIN: &str = ".codex/auth.json";

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
        // An unreadable file (a test's mode 0) keeps its kind, size and mtime only.
        let hash = std::fs::read(at)
            .ok()
            .map(|bytes| Sha256::digest(bytes).to_vec());
        into.insert(rel, ("file", meta.len(), modified, hash));
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

/// Runs the status check for `agent` (Clauseo signed in to nothing, so it detects), checks that
/// the lawyer's config is as it was and that every run started had Clauseo's own home (so none
/// was detection's), then gives the state.
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
    let home = fx.config.signin.homes[agent].clone().into_os_string();
    let var = if *agent == claude() {
        "CLAUDE_CONFIG_DIR"
    } else {
        "CODEX_HOME"
    };
    let runs = fx.launches();
    assert!(
        runs.iter()
            .all(|run| run.env.get(OsStr::new(var)) == Some(&home)),
        "a run did not have Clauseo's own home: {runs:?}"
    );
    // The one run is Clauseo's own status, signed out (a definite answer, so not asked again).
    assert_eq!(runs.len(), 1, "{runs:?}");
    state
}

fn found(from: LoginFrom) -> SigninState {
    SigninState::LoginFound { from, email: None }
}

fn no_login() -> SigninState {
    SigninState::SignedOut {
        found: Some(Found::NoLogin),
    }
}

#[test]
fn claude_detection_signed_out_writes_nothing_and_starts_no_run() {
    let fx = Fixture::new("nowrite-claude-out", "ok-after:0");
    assert_eq!(detect_unchanged(&fx, &claude()), no_login());
    assert!(!fx.user_home().join(".claude.json").exists());
    assert!(!fx.user_home().join(".claude").exists());
}

#[test]
fn claude_detection_with_a_login_writes_nothing_and_starts_no_run() {
    let fx = Fixture::new("nowrite-claude-in", "ok-after:0");
    let home = fx.user_home();
    write_file(&home.join(CLAUDE_LOGIN), b"{}").unwrap();
    write_file(
        &home.join(".claude/fake-signin-marker"),
        br#"{"authMethod":"claude.ai","email":"lawyer@example.com"}"#,
    )
    .unwrap();
    write_file(&home.join(".claude.json"), br#"{"numStartups":3}"#).unwrap();
    write_file(&home.join(".claude/settings.json"), br#"{"theme":"dark"}"#).unwrap();
    assert_eq!(
        detect_unchanged(&fx, &claude()),
        found(LoginFrom::ClaudeCode)
    );
}

#[test]
fn codex_detection_signed_out_writes_nothing_and_starts_no_run() {
    let fx = Fixture::new("nowrite-codex-out", "ok-after:0");
    assert_eq!(detect_unchanged(&fx, &codex()), no_login());
    assert!(!fx.user_home().join(".codex").exists());
}

#[test]
fn codex_detection_with_a_login_file_writes_nothing_and_starts_no_run() {
    let fx = Fixture::new("nowrite-codex-in", "ok-after:0");
    write_file(&fx.user_home().join(CODEX_LOGIN), b"{}").unwrap();
    write_file(&fx.user_home().join(".codex/config.toml"), b"").unwrap();
    assert_eq!(detect_unchanged(&fx, &codex()), found(LoginFrom::Codex));
}

/// Detection never opens the lawyer's files: with Claude's credential file, its global config
/// and its settings all unreadable (mode 0), detection still finds the login (a stat needs no
/// read), and no file's mode, size or mtime changes. That Clauseo's code names none of them but
/// the credential file, and that only in `reuse.rs`, is `tests/credential_names.rs`'s scan.
#[cfg(unix)]
#[test]
fn claude_detection_opens_none_of_the_lawyer_s_files() {
    use std::os::unix::fs::PermissionsExt as _;
    let fx = Fixture::new("nowrite-claude-unreadable", "ok-after:0");
    let home = fx.user_home();
    let files = [
        home.join(CLAUDE_LOGIN),
        home.join(".claude.json"),
        home.join(".claude/settings.json"),
    ];
    for file in &files {
        write_file(file, br#"{"primaryApiKey":"not-a-key"}"#).unwrap();
        clauseo_agents::testing::set_mode(file, 0).unwrap();
    }
    let seen = || {
        files.clone().map(|file| {
            let meta = std::fs::symlink_metadata(file).unwrap();
            (
                meta.permissions().mode() & 0o7777,
                meta.len(),
                meta.modified().ok(),
            )
        })
    };
    let before = seen();
    assert_eq!(
        detect_unchanged(&fx, &claude()),
        found(LoginFrom::ClaudeCode)
    );
    assert_eq!(seen(), before);
    for file in &files {
        clauseo_agents::testing::set_mode(file, 0o600).unwrap();
    }
}

/// A link standing at either agent's login file is no login found: only a regular file is.
#[cfg(unix)]
#[test]
fn a_link_at_a_login_file_is_no_login_found() {
    for (agent, login) in [(claude(), CLAUDE_LOGIN), (codex(), CODEX_LOGIN)] {
        let fx = Fixture::new("nowrite-link", "ok-after:0");
        let target = fx.data.path().join("elsewhere.json");
        write_file(&target, b"{}").unwrap();
        let link = fx.user_home().join(login);
        write_file(&link.with_file_name("keep"), b"").unwrap();
        clauseo_agents::testing::symlink(&target, &link).unwrap();
        assert_eq!(detect_unchanged(&fx, &agent), no_login(), "{login}");
    }
}

/// With `CLAUDE_CONFIG_DIR` or `CODEX_HOME` set in the lawyer's environment their CLI keeps its
/// login there, and "Use it" takes the default one only: nothing is found, whichever folder
/// holds a file, and neither folder changes.
#[test]
fn a_moved_agent_folder_finds_no_login() {
    for (agent, var, login, file) in [
        (
            claude(),
            "CLAUDE_CONFIG_DIR",
            CLAUDE_LOGIN,
            ".credentials.json",
        ),
        (codex(), "CODEX_HOME", CODEX_LOGIN, "auth.json"),
    ] {
        let mut fx = Fixture::new("nowrite-moved", "ok-after:0");
        let moved = fx.data.path().join("lawyer-agent-folder");
        fx.config
            .signin
            .base_env
            .insert(var.into(), moved.clone().into_os_string());
        write_file(&moved.join(file), b"{}").unwrap();
        write_file(&fx.user_home().join(login), b"{}").unwrap();
        let mut before = BTreeMap::new();
        walk(&moved, &moved, &mut before);
        assert_eq!(detect_unchanged(&fx, &agent), no_login(), "{var}");
        let mut after = BTreeMap::new();
        walk(&moved, &moved, &mut after);
        assert_eq!(after, before, "{var}");
    }
}
