//! The Codex fence's state-folder rules (R17 PR-C review rounds 3 to 5): the own state folder's
//! `tmp` readable and never writable, `agent-state` denied entry by entry at the launch, and the
//! deny globs that cover a matter made after it. The access model is `tests.rs`'.

use serde_json::json;

use super::PROFILE;
use super::tests::{access, data, fence, notes, run_root, state};

/// Round 3's H1 and round 4's ruling: Codex puts `CODEX_HOME/tmp/arg0/codex-arg0*` first on
/// `PATH`, and its own unsandboxed `git` calls look `git` up by name, so the shell may never
/// write there; it reads and runs them (`apply_patch`, `codex-execve-wrapper`, and the sandbox
/// helper bubblewrap before 0.9 re-runs). The state folder's `tmp` is no `write` entry and no
/// writable root, and no deny either. The shell's temp folder is the run folder's `tmp` (its
/// `TMPDIR`), writable. Its login link and another matter's folder are denied.
#[test]
fn the_shell_reads_but_never_writes_codex_homes_arg0_folder() {
    let keys = fence();
    for path in [
        state().join("tmp"),
        state()
            .join("tmp")
            .join("arg0")
            .join("codex-arg0abc")
            .join("apply_patch"),
    ] {
        assert_eq!(access(&keys, &path), "read", "{}", path.display());
    }
    let scoped = keys["permissions"][PROFILE]["filesystem"][data().to_str().unwrap()]
        .as_object()
        .unwrap();
    assert_eq!(scoped.get("agent-state/m1/codex-acp/tmp"), None);
    let writable: Vec<&String> = scoped
        .iter()
        .filter(|(_, mode)| *mode == &json!("write"))
        .map(|(subpath, _)| subpath)
        .collect();
    assert!(
        writable
            .iter()
            .all(|subpath| !subpath.contains("codex-acp")),
        "{writable:?}"
    );
    assert_eq!(
        access(&keys, &run_root().join("tmp").join("t.txt")),
        "write"
    );
    for path in [
        state().join("auth.json"),
        data().join("agent-state").join("m2").join("notes"),
    ] {
        assert_eq!(access(&keys, &path), "deny", "{}", path.display());
    }
}

/// Round 4's ruling, option B: `state_denies` lists, through no link, every other matter's
/// folder, every entry of this matter's folder but `notes` and `codex-acp`, and every entry of
/// the state folder but `tmp`, plus the login link, `.env` and `config.toml` by name.
#[test]
fn state_denies_leave_out_tmp_and_notes_only() {
    let dir = crate::pipeline::tests::support::TempDir::new("codex-state-denies");
    let data = dir.path().join("data");
    let state = data.join("agent-state/m1/codex-acp");
    for folder in [
        "agent-state/m1/notes",
        "agent-state/m1/claude-acp",
        "agent-state/m1/codex-acp/tmp/arg0",
        "agent-state/m1/codex-acp/sessions",
        "agent-state/m2/codex-acp",
    ] {
        crate::write::create_dir_all(&data.join(folder)).unwrap();
    }
    crate::write::write_file(&state.join("history.jsonl"), b"x").unwrap();
    let denies = super::state_denies(&data, &state);
    for denied in [
        data.join("agent-state/m2"),
        data.join("agent-state/m1/claude-acp"),
        state.join("sessions"),
        state.join("history.jsonl"),
        state.join("auth.json"),
        state.join(".env"),
        state.join("config.toml"),
    ] {
        assert!(
            denies.contains(&denied),
            "{} in {denies:?}",
            denied.display()
        );
    }
    for kept in [
        state.join("tmp"),
        data.join("agent-state/m1/notes"),
        state.clone(),
        data.join("agent-state/m1"),
    ] {
        assert!(!denies.contains(&kept), "{}", kept.display());
    }
}

/// Round 5: a matter made **after** the launch (not in the launch's `state_denies`) keeps its
/// Codex login link, `.env`, `config.toml`, transcripts, history, log and databases and its whole
/// Claude folder unreadable to the shell, by the profile's deny globs, in every turn. Its `notes`
/// is the named residual; the own state folder's `tmp` stays readable and the own notes
/// writable.
#[test]
fn a_matter_made_after_the_launch_keeps_its_logins_and_transcripts_unreadable() {
    let keys = fence();
    let later = data().join("agent-state").join("m-later");
    for path in [
        later.join("codex-acp").join("auth.json"),
        later.join("codex-acp").join(".env"),
        later.join("codex-acp").join("config.toml"),
        later
            .join("codex-acp")
            .join("sessions")
            .join("2026")
            .join("r.jsonl"),
        later
            .join("codex-acp")
            .join("archived_sessions")
            .join("a.jsonl"),
        later.join("codex-acp").join("history.jsonl"),
        later.join("codex-acp").join("log").join("codex.log"),
        later.join("codex-acp").join("state_5.sqlite-wal"),
        later
            .join("claude-acp")
            .join("projects")
            .join("p")
            .join("s.jsonl"),
        later.join("claude-acp").join(".claude.json"),
    ] {
        assert_eq!(access(&keys, &path), "deny", "{}", path.display());
    }
    assert_eq!(access(&keys, &later.join("notes").join("n.md")), "read");
    assert_eq!(
        access(&keys, &state().join("tmp").join("arg0").join("x")),
        "read"
    );
    assert_eq!(access(&keys, &notes().join("n.md")), "write");
}

/// Round 5, low 1: every name Codex keeps in the own state folder but `tmp` is denied by name,
/// present at the launch or not.
#[test]
fn the_own_kept_names_are_denied_even_when_absent() {
    let dir = crate::pipeline::tests::support::TempDir::new("codex-state-kept-names");
    let data = dir.path().join("data");
    let state = data.join("agent-state/m1/codex-acp");
    crate::write::create_dir_all(&state.join("tmp")).unwrap();
    let denies = super::state_denies(&data, &state);
    for name in [
        "auth.json",
        "config.toml",
        ".env",
        "history.jsonl",
        "cap_sid",
        "sessions",
        "archived_sessions",
        "log",
        "state_5.sqlite",
        "logs_2.sqlite-wal",
    ] {
        assert!(denies.contains(&state.join(name)), "{name}");
    }
    assert!(!denies.contains(&state.join("tmp")));
}
