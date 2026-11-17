//! Codex's permission profile in each Codex session's `CODEX_CONFIG`, macOS and Linux: a
//! profile extending `:workspace` whose only entries name the session's run folder (with the
//! shell browser's `agent-browser` folder in it), the conversation's work folder and the
//! matter's notes `write`, as ACP's `additionalDirectories` do
//! (`actor::turn::profile_loads::codex_directories`).
//!
//! At codex tag `rust-v0.162.1`:
//! - Per-path rules exist only in a named profile: `default_permissions = "<name>"` selects
//!   `[permissions.<name>]`, whose `filesystem` maps a path to `read`, `write` or `deny`
//!   (`codex-rs/config/src/permissions_toml.rs:113-118, 224-244`). The entries are scoped under
//!   `<data>` (`{"<data>": {"run/<s>": "write", ...}}`): a scoped key's base is read as a
//!   literal path, never a glob (`codex-rs/core/src/config/permissions.rs:572-623, 674-705`).
//!   Under a profile `CODEX_CONFIG`'s `sandbox_workspace_write.writable_roots` no longer reaches
//!   a turn (stage B, live, macOS), so the profile names the three folders itself.
//! - codex-acp sends its `workspaceWrite` policy with every `turn/start`, its writable roots the
//!   session's `additionalDirectories` (codex-acp 2.2.2 `dist/index.js:35598-35612,
//!   35878-35885`). Codex rebuilds the turn's policy from it and keeps only the profile's `deny`
//!   entries (`codex-rs/core/src/session/session.rs:491-497`,
//!   `codex-rs/protocol/src/permissions.rs:709-731`), so an ordinary command writes the matter
//!   folder, the run, work and notes folders and `/tmp`, and reads the rest.
//! - The profile has no `deny` entry, on purpose (Rohan, 2026-11-17: as safe as Codex in its own
//!   automatic mode, no safer). With any denied read Codex runs a command its reviewer approved
//!   still sandboxed, without network (`codex-rs/core/src/tools/sandboxing.rs:252-296`,
//!   `unsandboxed_execution_allowed`); with none, the approved command runs as in plain Codex.
//!   And a `read` entry would be dropped by each turn's rebuild, so none stands in for the denies
//!   either.
//!
//! Not on Windows: the unelevated sandbox Clauseo gives Codex there takes `writable_roots`.

use std::path::Path;

use clauseo_settings::Locations;
use serde_json::{Map, Value};

/// The profile's name in `[permissions]`.
pub(crate) const PROFILE: &str = "clauseo";
/// The built-in profile it extends: Codex's workspace-write.
pub(crate) const EXTENDS: &str = ":workspace";

/// The session's folders the profile names `write`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Session<'a> {
    /// `<data>/run/<session id>`.
    pub(crate) run_root: &'a Path,
    /// `<data>/agent-state/<matter>/work/<thread>`, the conversation's.
    pub(crate) work: &'a Path,
    /// `<data>/agent-state/<matter>/notes`.
    pub(crate) notes: &'a Path,
}

/// The `CODEX_CONFIG` keys of the profile: `default_permissions` naming [`PROFILE`], and
/// `permissions.<PROFILE>` extending [`EXTENDS`] with the session's run, work and notes
/// folders as `write`, scoped under `<data>`; a folder outside `<data>` is keyed by its own
/// path. Empty on Windows (`windows`). A path that is not UTF-8 is left out with a warning; the
/// launch already fails on a run folder or notes that are not (`Folders::of`), and those sit in
/// the data folder.
pub(crate) fn keys(
    locations: &Locations,
    session: Session<'_>,
    windows: bool,
) -> Map<String, Value> {
    let mut keys = Map::new();
    if windows {
        return keys;
    }
    let data = locations.data_dir();
    let Some(base) = data.to_str() else {
        tracing::warn!("the data folder is not UTF-8: Codex gets no permission profile");
        return keys;
    };
    let mut scoped = Map::new();
    let mut filesystem = Map::new();
    for path in [session.run_root, session.work, session.notes] {
        match path.strip_prefix(data).ok().and_then(Path::to_str) {
            Some(relative) if !relative.is_empty() => {
                scoped.insert(relative.replace('\\', "/"), Value::from("write"));
            }
            _ => {
                if let Some(text) = path.to_str() {
                    filesystem.insert(text.to_owned(), Value::from("write"));
                } else {
                    tracing::warn!("a session folder is not UTF-8 and is left out");
                }
            }
        }
    }
    filesystem.insert(base.to_owned(), Value::Object(scoped));
    let mut profile = Map::new();
    profile.insert("extends".to_owned(), Value::from(EXTENDS));
    profile.insert("filesystem".to_owned(), Value::Object(filesystem));
    let mut permissions = Map::new();
    permissions.insert(PROFILE.to_owned(), Value::Object(profile));
    keys.insert("default_permissions".to_owned(), Value::from(PROFILE));
    keys.insert("permissions".to_owned(), Value::Object(permissions));
    keys
}

#[cfg(test)]
#[path = "codex_fence/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "codex_fence/writable_tests.rs"]
mod writable_tests;
