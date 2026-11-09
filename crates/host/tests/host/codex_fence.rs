//! The data-folder fence in a Codex launch's `CODEX_CONFIG` (`agent::codex_fence`, decision record
//! 0030), checked from `codex.rs`'s launch test: denies scoped under the data folder, `run` whole
//! and `agent-state` entry by entry (the own folders reopen as `write` entries, checked in
//! `profile.rs`), the login home whole, never the session's state folder (its `CODEX_HOME`, R17
//! PR-C) but the login link and the config surfaces in it. Windows gets none.

use std::path::Path;

use clauseo_protocol::MatterId;
use serde_json::{Value, json};

use crate::support::Dir;

/// Checks the fence keys in `config` for a Codex session of `matter` whose run folder is `run`
/// and whose Codex home (its state folder) is `home`, and returns them as they are, for the
/// exact comparison.
pub(crate) fn checked_keys(
    config: &Value,
    dir: &Dir,
    matter: MatterId,
    run: &Path,
    home: &Path,
) -> [(&'static str, Value); 2] {
    let data = crate::profile_rig::path_text(dir.data());
    assert_eq!(config["default_permissions"], json!("clauseo"));
    let profile = &config["permissions"]["clauseo"];
    assert_eq!(profile["extends"], json!(":workspace"));
    let scoped = profile["filesystem"][data.as_str()]
        .as_object()
        .expect("denies scoped under the data folder");
    for subpath in [
        "logs",
        "search.sqlite",
        "state.sqlite",
        "skill-packs",
        "set-aside",
        "run",
        "agents/claude-acp",
        "agents/codex-acp/home",
    ] {
        assert_eq!(scoped.get(subpath), Some(&json!("deny")), "{subpath}");
    }
    let notes = crate::profile_rig::notes(dir, matter);
    for (subpath, mode) in scoped {
        if mode == &json!("write") || ["run", "notes", "agent-state"].contains(&subpath.as_str()) {
            continue;
        }
        assert_eq!(mode, &json!("deny"), "{subpath}");
        let denied = dir.data().join(subpath);
        if denied.starts_with(home) {
            // The login link, the transcripts and the config surfaces in the state folder.
            continue;
        }
        for own in [run, notes.as_path(), home] {
            assert!(!own.starts_with(&denied), "{subpath}");
        }
    }
    [
        ("default_permissions", config["default_permissions"].clone()),
        ("permissions", config["permissions"].clone()),
    ]
}

/// Linux, R17 PR-D1's ripgrep guard (`agent::codex_fence::rg_guard`): with no `rg` on the
/// `PATH` Codex would get, a Codex launch is refused with `agent.launch_failed` before anything
/// starts; with one in a system folder (here a folder outside every session folder) it runs.
#[cfg(target_os = "linux")]
#[test]
fn a_codex_launch_with_no_ripgrep_on_its_path_is_refused() {
    use std::ffi::OsString;
    use std::sync::Arc;

    use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, StreamId};

    use crate::codex::{CODEX, Recording, codex_dir};
    use crate::support::matter_with_thread;

    for (test, with_rg) in [("codex_fence_no_rg", false), ("codex_fence_rg", true)] {
        let dir = codex_dir(test, "codex-handshake");
        let bin = dir.work().join("system-bin");
        crate::write::create_dir_all(&bin);
        if with_rg {
            crate::write::write(&bin.join("rg"), b"#!/bin/sh\n");
            crate::write::make_executable(&bin.join("rg"));
        }
        let base = vec![(OsString::from("PATH"), bin.into_os_string())];
        let recording = Recording::new(&dir);
        let launches = Arc::clone(&recording.envs);
        let host = dir.start_launching(0, Arc::new(recording), |config| {
            config.with_inherited_env(base)
        });
        let (_, thread, _) = matter_with_thread(&dir, &host, "a", &[("brief.txt", "x\n")], CODEX);
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Look at the matter.");
        let ended = events.until_turn_ended(turn).stop;
        if with_rg {
            assert_eq!(ended, StopReason::EndTurn);
            assert_eq!(launches.lock().unwrap().len(), 1);
        } else {
            assert_eq!(ended, StopReason::Failed);
            let code = events.events().find_map(|event| match event {
                DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
                _ => None,
            });
            assert_eq!(code, Some(ErrorCode::AgentLaunchFailed));
            assert!(launches.lock().unwrap().is_empty(), "nothing launched");
        }
        host.shutdown();
    }
}
