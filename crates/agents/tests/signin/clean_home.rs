//! Detection never runs a CLI in a lawyer's home that holds nothing of it (the Windows VM pass
//! of #664): the real Claude Code makes `~/.claude` and `~/.claude.json` as it starts, and Codex
//! `~/.codex/tmp/arg0`, so a run there would leave their files on a machine that never had
//! either. A clean home is stat'ed, never run in, and left exactly as it was; a home with
//! Claude Code's `.claude.json` is run in as before.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;

use clauseo_agents::testing::write_file;
use clauseo_agents::{Found, SigninState};

use crate::support::{Fixture, Launch, claude, codex};

/// Every path under `root`, relative to it, folders included.
fn tree(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            found.insert(relative);
            if std::fs::symlink_metadata(&path).unwrap().is_dir() {
                stack.push(path);
            }
        }
    }
    found
}

/// The runs in the lawyer's own profile: detection's, the only ones with neither Clauseo's
/// `CLAUDE_CONFIG_DIR` nor its `CODEX_HOME`.
fn lawyer_runs(fx: &Fixture) -> Vec<Launch> {
    fx.launches()
        .into_iter()
        .filter(|run| {
            !run.env.contains_key(OsStr::new("CLAUDE_CONFIG_DIR"))
                && !run.env.contains_key(OsStr::new("CODEX_HOME"))
        })
        .collect()
}

fn no_login() -> SigninState {
    SigninState::SignedOut {
        found: Some(Found::NoLogin),
    }
}

/// A home with no Claude Code or Codex files: both detections report no login with no run in
/// the lawyer's profile, and the home has no new file or folder (no `.claude`, `.claude.json`
/// or `.codex/tmp/arg0`).
#[test]
fn a_clean_home_is_never_run_in_and_stays_untouched() {
    let fx = Fixture::new("clean-home", "ok-after:0");
    let home = fx.user_home();
    write_file(
        &home.join("Documents").join("brief.txt"),
        b"a lawyer's file",
    )
    .unwrap();
    let before = tree(&home);

    let (agents, events) = fx.start_for(claude());
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), no_login());
    drop(agents);

    let (agents, events) = fx.start_for(codex());
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.until_end().1, no_login());
    drop(agents);

    assert_eq!(lawyer_runs(&fx), [], "a run in the lawyer's profile");
    assert_eq!(tree(&home), before, "the clean home changed");
}

/// With the lawyer's `~/.claude.json` there (Claude Code has run on this machine), detection
/// runs the CLI's status in the lawyer's profile as before.
#[test]
fn a_home_with_claude_json_is_run_in_as_before() {
    let fx = Fixture::new("claude-json-home", "ok-after:0");
    write_file(&fx.user_home().join(".claude.json"), b"{}").unwrap();
    let (agents, events) = fx.start_for(claude());
    agents.check_status(&claude()).unwrap();
    assert_eq!(events.state(), no_login());
    let runs = lawyer_runs(&fx);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert!(runs[0].is("status"), "{runs:?}");
    assert_eq!(
        runs[0].env.get(OsStr::new("HOME")),
        Some(&fx.user_home().into_os_string())
    );
}

/// With the lawyer's `~/.codex` there, Codex's detection runs `login status` there as before.
#[test]
fn a_home_with_a_codex_folder_is_run_in_as_before() {
    let fx = Fixture::new("codex-folder-home", "ok-after:0");
    write_file(&fx.user_home().join(".codex").join("config.toml"), b"").unwrap();
    let (agents, events) = fx.start_for(codex());
    agents.check_status(&codex()).unwrap();
    assert_eq!(events.until_end().1, no_login());
    let runs = lawyer_runs(&fx);
    assert!(
        runs.iter().any(|run| run.command() == ["login", "status"]),
        "{runs:?}"
    );
}
