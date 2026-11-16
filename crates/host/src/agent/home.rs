//! Clauseo's own home for each agent with a kind: `Locations::agent_home` of its id,
//! `<data_dir>/agents/<id>/home`, made at each launch (`actor/turn/launch_checks.rs`) and forced
//! over any inherited, settings or hook value. Codex gets it as `CODEX_HOME` (`agent/codex.rs`),
//! Claude Code as `CLAUDE_CONFIG_DIR` ([`claude_launch_env`]), so neither reads the lawyer's own
//! `~/.codex` or `~/.claude`: their allow rules, hooks and skills stay out of Clauseo's sessions
//! (decision record 0048). The profile's Codex skills go in the same folder
//! (`clauseo_profile::paths::codex_skills`, from [`AgentKind::id`]), and Claude's plan notes are
//! marked against it (`agent/plan_note.rs` reads the launch's merged environment, which holds
//! this variable by then). Each Claude launch also writes Claude Code's user settings there
//! ([`write_claude_settings`]).
//!
//! [`AgentKind::id`]: clauseo_protocol::AgentKind::id

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use serde_json::{Map, Value};

/// The variable Claude Code takes its config folder from: its settings, its login, its plan
/// notes (`<CLAUDE_CONFIG_DIR>/plans`) and its own skills and hooks.
pub(crate) const CLAUDE_CONFIG_VAR: &str = "CLAUDE_CONFIG_DIR";

/// What the host forces on a Claude agent's launch: `home` as `CLAUDE_CONFIG_DIR`.
pub(crate) fn claude_launch_env(home: &Path) -> BTreeMap<OsString, OsString> {
    BTreeMap::from([(
        OsString::from(CLAUDE_CONFIG_VAR),
        home.as_os_str().to_owned(),
    )])
}

/// Claude Code's user settings file in its config folder.
pub(crate) const CLAUDE_SETTINGS_FILE: &str = "settings.json";

/// What each Claude launch sets in [`CLAUDE_SETTINGS_FILE`] of Clauseo's own Claude home
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

#[cfg(test)]
mod tests;
