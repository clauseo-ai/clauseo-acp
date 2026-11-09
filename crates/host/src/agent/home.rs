//! Clauseo's own homes for each agent with a kind. The login home, `Locations::agent_home` of
//! its id, `<data>/agents/<id>/home`, made at each launch (`actor/turn/launch_checks.rs`), is
//! where the login lives: Claude Code's login store and Codex's own `auth.json`, which S18's
//! status reads, the limits readers and sign-in runs use as their `CLAUDE_CONFIG_DIR` and
//! `CODEX_HOME`. Since R17 PR-C a session's config folder is the matter's state folder instead
//! (`agent/state_folder.rs`, `<data>/agent-state/<matter>/<agent>`), forced over any inherited,
//! settings or hook value: Claude Code gets it as `CLAUDE_CONFIG_DIR` ([`claude_launch_env`])
//! with `CLAUDE_SECURESTORAGE_CONFIG_DIR` naming the one login store, Codex as `CODEX_HOME`
//! (`agent/codex.rs`). Neither reads the lawyer's own `~/.codex` or `~/.claude` (decision
//! record 0048), and no matter's session reads another's transcripts. Claude's plan notes are
//! marked against the state folder (`agent/plan_note.rs` reads the launch's merged environment,
//! which holds this variable by then). Each Claude launch writes Claude Code's user settings
//! into the state folder, Clauseo's keys only ([`write_state_settings`]), and keeps them in the
//! login home for the readers that run there ([`write_claude_settings`]).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use serde_json::{Map, Value};

/// The variable Claude Code takes its config folder from: its settings, its plan notes
/// (`<CLAUDE_CONFIG_DIR>/plans`), its transcripts and its own skills and hooks.
pub(crate) const CLAUDE_CONFIG_VAR: &str = "CLAUDE_CONFIG_DIR";

/// The variable Claude Code takes its login store's folder from, over `CLAUDE_CONFIG_DIR`
/// (`research/42-reusing-the-claude-code-login.md` §1): a path is that folder's
/// `.credentials.json` (and its Keychain item's name on macOS), `""` the lawyer's default store.
pub(crate) const CLAUDE_STORE_VAR: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";

/// What the host forces on a Claude agent's launch: `state`, the matter's state folder, as
/// `CLAUDE_CONFIG_DIR`, and the login store as `CLAUDE_SECURESTORAGE_CONFIG_DIR`: `store`, the
/// login home, on Clauseo's own sign-in, or `""` (the lawyer's default store) with `None`,
/// while desktop says the login is reused (`HostHandle::set_reused_login`). Claude's `HOME`
/// is never changed.
pub(crate) fn claude_launch_env(
    state: &Path,
    store: Option<&Path>,
) -> BTreeMap<OsString, OsString> {
    BTreeMap::from([
        (
            OsString::from(CLAUDE_CONFIG_VAR),
            state.as_os_str().to_owned(),
        ),
        (
            OsString::from(CLAUDE_STORE_VAR),
            store.map_or_else(OsString::new, |store| store.as_os_str().to_owned()),
        ),
    ])
}

/// Claude Code's user settings file in its config folder.
pub(crate) const CLAUDE_SETTINGS_FILE: &str = "settings.json";

/// What each Claude launch sets in [`CLAUDE_SETTINGS_FILE`] of the state folder and the login home
/// (decision record 0040, G11): `syncClaudeAiSkills: false`, so Claude Code neither syncs the
/// account's claude.ai skills nor loads them in a Clauseo session. No switch turns it back on.
pub(crate) const CLAUDE_HOME_SETTINGS: [(&str, bool); 1] = [("syncClaudeAiSkills", false)];

/// Writes [`CLAUDE_HOME_SETTINGS`] into `<home>/settings.json`, keeping every other key a JSON
/// object there holds; a file that is not one is replaced. Nothing is written when the file
/// already has them. The file is replaced atomically, a link there replaced and never followed
/// for the write. File system calls: off the actor.
pub(crate) fn write_claude_settings(home: &Path) -> std::io::Result<()> {
    let path = home.join(CLAUDE_SETTINGS_FILE);
    let mut settings = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(settings)) => settings,
            _ => Map::new(),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(error) => return Err(error),
    };
    let set = CLAUDE_HOME_SETTINGS
        .iter()
        .all(|(key, value)| settings.get(*key) == Some(&Value::Bool(*value)));
    if set {
        return Ok(());
    }
    for (key, value) in CLAUDE_HOME_SETTINGS {
        settings.insert(key.to_owned(), Value::Bool(value));
    }
    let bytes = serde_json::to_vec_pretty(&Value::Object(settings))?;
    crate::write::replace_private_file(&path, &bytes)
}

/// Writes `<state>/settings.json` whole with [`CLAUDE_HOME_SETTINGS`] and nothing else, so no
/// key a session left there (hooks, permissions, `env`, `apiKeyHelper`) reaches the matter's
/// later sessions (R17 PR-C). A firm or key login comes from the hook's env, never from this
/// file. Nothing is written when the file already holds exactly these bytes; otherwise it is
/// replaced atomically, a link there replaced and never followed. File system calls: off the
/// actor.
pub(crate) fn write_state_settings(state: &Path) -> std::io::Result<()> {
    let path = state.join(CLAUDE_SETTINGS_FILE);
    let settings: Map<String, Value> = CLAUDE_HOME_SETTINGS
        .iter()
        .map(|(key, value)| ((*key).to_owned(), Value::Bool(*value)))
        .collect();
    let bytes = serde_json::to_vec_pretty(&Value::Object(settings))?;
    // Read only as a plain file with one name (`state_folder::safe_read`): the agent can plant
    // it as a link or a hard link to a credential.
    let held = crate::agent::state_folder::safe_read::read_plain(&path)?;
    if held == crate::agent::state_folder::safe_read::Found::Bytes(bytes.clone()) {
        return Ok(());
    }
    crate::write::replace_private_file(&path, &bytes)
}

#[cfg(test)]
mod tests;
