//! Codex's permission profile in a Codex launch's `CODEX_CONFIG` (`agent::codex_fence`),
//! checked from `codex.rs`'s launch test: it extends `:workspace`, names only the session's run,
//! work and notes folders `write`, scoped under the data folder, and has no `deny` entry, so a
//! command Codex's reviewer approves runs as in plain Codex. Windows gets none.

use std::path::Path;

use clauseo_protocol::MatterId;
use serde_json::{Value, json};

use crate::support::Dir;

/// Checks the profile keys in `config` for a Codex session of `matter` whose run folder is
/// `run`, and returns them as they are, for the exact comparison.
pub(crate) fn checked_keys(
    config: &Value,
    dir: &Dir,
    matter: MatterId,
    run: &Path,
) -> [(&'static str, Value); 2] {
    let data = crate::profile_rig::path_text(dir.data());
    assert_eq!(config["default_permissions"], json!("clauseo"));
    let profile = &config["permissions"]["clauseo"];
    assert_eq!(profile["extends"], json!(":workspace"));
    assert!(
        !profile.to_string().contains("deny"),
        "no deny entry: {profile}"
    );
    let scoped = profile["filesystem"][data.as_str()]
        .as_object()
        .expect("entries scoped under the data folder");
    let notes = crate::profile_rig::notes(dir, matter);
    let mut written: Vec<_> = scoped
        .iter()
        .map(|(subpath, mode)| {
            assert_eq!(mode, &json!("write"), "{subpath}");
            dir.data().join(subpath)
        })
        .collect();
    written.sort();
    let work = crate::profile_rig::work_of(run).expect("the conversation's work folder");
    let mut own = vec![run.to_path_buf(), work, notes];
    own.sort();
    assert_eq!(written, own);
    [
        ("default_permissions", config["default_permissions"].clone()),
        ("permissions", config["permissions"].clone()),
    ]
}
