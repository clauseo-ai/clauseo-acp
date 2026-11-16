//! The data-folder fence in a Codex launch's `CODEX_CONFIG` (`agent::codex_fence`, decision
//! record 0030), checked from `codex.rs`'s launch test: denies scoped under the data folder,
//! the siblings of the own run folder and notes as the launch listed them, never `run` or
//! `agent-state` whole, so an escalated command reaches the own folders too (they are `write`
//! entries as well, checked in `profile.rs`), never Codex's home. Windows gets none.

use std::path::Path;

use clauseo_protocol::MatterId;
use serde_json::{Value, json};

use crate::support::Dir;

/// Checks the fence keys in `config` for a Codex session of `matter` whose run folder is `run`
/// and whose Codex home is `home`, and returns them as they are, for the exact comparison.
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
        "agents/claude-acp",
        "agents/signin",
        "agents/codex-acp/home/auth.json",
        "agents/codex-acp/home/history.jsonl",
    ] {
        assert_eq!(scoped.get(subpath), Some(&json!("deny")), "{subpath}");
    }
    for whole in ["run", "agent-state"] {
        assert_eq!(scoped.get(whole), None, "{whole}");
    }
    let notes = crate::profile_rig::notes(dir, matter);
    for (subpath, mode) in scoped {
        if mode == &json!("write") {
            continue;
        }
        assert_eq!(mode, &json!("deny"), "{subpath}");
        let denied = dir.data().join(subpath);
        for own in [run, notes.as_path(), home] {
            assert!(!own.starts_with(&denied), "{subpath}");
        }
    }
    [
        ("default_permissions", config["default_permissions"].clone()),
        ("permissions", config["permissions"].clone()),
    ]
}
