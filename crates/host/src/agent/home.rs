//! Clauseo's own home for each agent with a kind: `Locations::agent_home` of its id,
//! `<data_dir>/agents/<id>/home`, made at each launch (`actor/turn/launch_checks.rs`) and forced
//! over any inherited, settings or hook value. Codex gets it as `CODEX_HOME` (`agent/codex.rs`),
//! Claude Code as `CLAUDE_CONFIG_DIR` ([`claude_launch_env`]), so neither reads the lawyer's own
//! `~/.codex` or `~/.claude`: their allow rules, hooks and skills stay out of Clauseo's sessions
//! (decision record 0048). The profile's Codex skills go in the same folder
//! (`clauseo_profile::paths::codex_skills`, from [`AgentKind::id`]), and Claude's plan notes are
//! marked against it (`agent/plan_note.rs` reads the launch's merged environment, which holds
//! this variable by then). Each Claude launch also writes Claude Code's user settings there
//! ([`write_claude_settings`]), or is refused.
//!
//! [`AgentKind::id`]: clauseo_protocol::AgentKind::id

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use serde_json::{Map, Value};

/// The variable Claude Code takes its config folder from: its settings, its login, its plan
/// notes (`<CLAUDE_CONFIG_DIR>/plans`) and its own skills and hooks.
pub(crate) const CLAUDE_CONFIG_VAR: &str = "CLAUDE_CONFIG_DIR";

/// What the host forces on a Claude agent's launch: `home` as `CLAUDE_CONFIG_DIR`, and
/// `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` (`loads::memory::AUTO_MEMORY_OFF_VAR`), over any inherited,
/// settings or hook value: `0` or `false` there would turn Claude Code's auto-memory on over the
/// flag settings' `autoMemoryEnabled: false`.
pub(crate) fn claude_launch_env(home: &Path) -> BTreeMap<OsString, OsString> {
    BTreeMap::from([
        (
            OsString::from(CLAUDE_CONFIG_VAR),
            home.as_os_str().to_owned(),
        ),
        (
            OsString::from(crate::agent::loads::memory::AUTO_MEMORY_OFF_VAR),
            OsString::from("1"),
        ),
    ])
}

/// Claude Code's user settings file in its config folder.
pub(crate) const CLAUDE_SETTINGS_FILE: &str = "settings.json";

/// What each Claude launch sets in [`CLAUDE_SETTINGS_FILE`] of Clauseo's own Claude home
/// (decision record 0040, G11): `syncClaudeAiSkills: false`, so Claude Code neither syncs the
/// account's claude.ai skills nor loads them in a Clauseo session. No switch turns it back on.
pub(crate) const CLAUDE_HOME_SETTINGS: [(&str, bool); 1] = [("syncClaudeAiSkills", false)];

/// The keys Claude Code 2.1.293 writes to its user file itself that a launch keeps, each only
/// with a value of the kind it writes; none loosens a permission or loads anything:
///
/// - `model`, a string: its model picker and `/model`. Clauseo names the model per session, so
///   this only keeps a lawyer's last choice in Claude's own picker as it was.
/// - `fastMode`, a bool: its fast-mode toggle, a preference of the lawyer's.
/// - `skipAutoPermissionPrompt`, a bool: the answer to its auto-mode opt-in dialog, so the
///   dialog is not asked again at every launch.
///
/// Not `permissions` (Claude Code writes `permissions.defaultMode` from its auto-mode nudge):
/// the flag settings pin the mode (`loads::pinned`), so it counts for nothing, and the same key
/// could hold rules.
pub(crate) const CLAUDE_OWN_KEYS: [&str; 3] = ["model", "fastMode", "skipAutoPermissionPrompt"];

/// Whether `value` is of the kind Claude Code writes at `key`, one of [`CLAUDE_OWN_KEYS`].
fn own_value(key: &str, value: &Value) -> bool {
    match key {
        "model" => value.is_string(),
        "fastMode" | "skipAutoPermissionPrompt" => value.is_boolean(),
        _ => false,
    }
}

/// Writes `<home>/settings.json` as exactly what Clauseo ships: [`CLAUDE_HOME_SETTINGS`], and of
/// what the file held before (a plain file that reads and whose JSON is an object; a link is
/// not read) only [`CLAUDE_OWN_KEYS`] with a value of their kind. Every other key goes. Nothing
/// is written when the file already says exactly that. The file is replaced atomically, a link
/// there replaced and never followed; what cannot be replaced (a folder at the name, a file
/// Windows will not let go) is renamed aside ([`clear::set_aside`]) and the file written fresh.
/// An error (that rename or the fresh write failing too, a full disk) is the caller's to refuse
/// the launch on. File system calls: off the actor.
pub(crate) fn write_claude_settings(home: &Path) -> std::io::Result<()> {
    let path = home.join(CLAUDE_SETTINGS_FILE);
    let plain = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file());
    let held = plain.then(|| std::fs::read(&path).ok()).flatten();
    let before = match held.map(|bytes| serde_json::from_slice::<Value>(&bytes)) {
        Some(Ok(Value::Object(settings))) => Some(settings),
        _ => None,
    };
    let mut settings = Map::new();
    if let Some(before) = &before {
        for key in CLAUDE_OWN_KEYS {
            if let Some(value) = before.get(key).filter(|value| own_value(key, value)) {
                settings.insert(key.to_owned(), value.clone());
            }
        }
    }
    for (key, value) in CLAUDE_HOME_SETTINGS {
        settings.insert(key.to_owned(), Value::Bool(value));
    }
    if before.as_ref() == Some(&settings) {
        return Ok(());
    }
    let bytes = serde_json::to_vec_pretty(&Value::Object(settings))?;
    // A file that cannot be replaced (a folder at the name, a file Windows will not let go) is
    // moved aside and written fresh; only when that fails too is it the caller's error.
    match crate::write::replace_private_file(&path, &bytes) {
        Ok(()) => Ok(()),
        Err(first) => match clear::set_aside(home, CLAUDE_SETTINGS_FILE) {
            Ok(Some(aside)) => {
                if clear::remove_entry(&aside).is_err() {
                    tracing::warn!("Claude's old settings file stays aside for now");
                }
                crate::write::replace_private_file(&path, &bytes)
            }
            Ok(None) | Err(_) => Err(first),
        },
    }
}

/// [`write_claude_settings`] for the app: the sign-in jobs run it before each Claude Code
/// process they start (`clauseo_agents::SigninConfig::claude_settings`), so a login, a status
/// check or a logout reads only the user settings Clauseo ships. File system calls: off the
/// host thread.
pub fn write_claude_home_settings(home: &Path) -> std::io::Result<()> {
    write_claude_settings(home)
}

/// The launch's refusal when [`write_claude_settings`] failed, with a warn line naming the
/// cause, never the path: a settings file left as it was could loosen the session, so the agent
/// is not started.
pub(crate) fn settings_unwritten(error: &std::io::Error) -> crate::error::Error {
    tracing::warn!(
        error = %crate::log_file::Cause(error),
        "Claude's home settings were not written: the launch fails"
    );
    crate::error::Error::refused(
        clauseo_protocol::ErrorCode::AgentLaunchFailed,
        "Clauseo could not write Claude's settings file.",
    )
}

#[cfg(test)]
mod tests;

mod clear;
pub(crate) use clear::{LOADED as CLAUDE_HOME_LOADED, clear_home};
