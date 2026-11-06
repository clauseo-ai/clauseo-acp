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
        "run.old-*",
        "run",
        "agents/claude-acp",
        "agents/codex-acp/home",
    ] {
        assert_eq!(scoped.get(subpath), Some(&json!("deny")), "{subpath}");
    }
    let notes = crate::profile_rig::notes(dir, matter);
    for (subpath, mode) in scoped {
        if mode == &json!("write") || ["run", "agent-state"].contains(&subpath.as_str()) {
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
