//! Clauseo's own home for each agent with a kind: `Locations::agent_home` of its id,
//! `<data_dir>/agents/<id>/home`, made at each launch (`actor/turn/launch_checks.rs`) and forced
//! over any inherited, settings or hook value. Codex gets it as `CODEX_HOME` (`agent/codex.rs`),
//! Claude Code as `CLAUDE_CONFIG_DIR` ([`claude_launch_env`]), so neither reads the lawyer's own
//! `~/.codex` or `~/.claude`: their allow rules, hooks and skills stay out of Clauseo's sessions
//! (decision record 0048). The profile's Codex skills go in the same folder
//! (`clauseo_profile::paths::codex_skills`, from [`AgentKind::id`]), and Claude's plan notes are
//! marked against it (`agent/plan_note.rs` reads the launch's merged environment, which holds
//! this variable by then). Each Claude launch also writes Claude Code's user settings there
//! afresh, holding only Clauseo's own keys ([`write_claude_settings`]), and the turn checks
//! before each prompt that they still do ([`claude_settings_intact`]).
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

/// The user settings file Claude Code 2.1.293 reads instead of [`CLAUDE_SETTINGS_FILE`] when
/// `CLAUDE_CODE_USE_COWORK_PLUGINS` is set (its `V_`). No launch sets that variable, and no
/// inherited one reaches the agent (`agent::spec`, every `CLAUDE_*`), but the profile hook or
/// an agent's settings `env` could still name it, so each launch removes the file
/// ([`write_claude_settings`]): with the variable set and no file, Claude Code reads no user
/// settings at all.
pub(crate) const CLAUDE_COWORK_SETTINGS_FILE: &str = "cowork_settings.json";

/// What each Claude launch writes as [`CLAUDE_SETTINGS_FILE`] of Clauseo's own Claude home, and
/// nothing else (decision record 0040, G11): `syncClaudeAiSkills: false`, so Claude Code neither
/// syncs the account's claude.ai skills nor loads them in a Clauseo session. No switch turns it
/// back on. Every security key Clauseo needs goes in the flag settings instead
/// (`agent::loads::pinned`), which outrank this file.
pub(crate) const CLAUDE_HOME_SETTINGS: [(&str, bool); 1] = [("syncClaudeAiSkills", false)];

/// [`CLAUDE_HOME_SETTINGS`] as the JSON object the launch writes.
fn home_settings() -> Map<String, Value> {
    CLAUDE_HOME_SETTINGS
        .iter()
        .map(|(key, value)| ((*key).to_owned(), Value::Bool(*value)))
        .collect()
}

/// Writes `<home>/settings.json` afresh at each launch, holding [`CLAUDE_HOME_SETTINGS`] only,
/// and removes `<home>/cowork_settings.json` ([`CLAUDE_COWORK_SETTINGS_FILE`]). Claude Code
/// 2.1.293 reads that user file under `--setting-sources=user` and merges it under the flag
/// settings: its arrays (`permissions.allow`, `additionalDirectories`,
/// `sandbox.filesystem.allowWrite`, `sandbox.excludedCommands`), its `hooks` and its `env`
/// (copied into the process environment) join Clauseo's, and no flag setting can take them out.
/// So whatever was there (other keys, a file that is not JSON, one planted by a tool of an
/// earlier session) is replaced, never merged. The write is atomic, and a link at either name is
/// replaced or removed, never followed. An error (a folder at the name) is the caller's to
/// refuse the launch on. File system calls: off the actor.
pub(crate) fn write_claude_settings(home: &Path) -> std::io::Result<()> {
    crate::write::remove_old_file(&home.join(CLAUDE_COWORK_SETTINGS_FILE))?;
    let bytes = serde_json::to_vec_pretty(&Value::Object(home_settings()))?;
    crate::write::replace_private_file(&home.join(CLAUDE_SETTINGS_FILE), &bytes)
}

/// The launch's refusal when [`write_claude_settings`] failed (a folder at the name, a full
/// disk), with a warn line naming the cause, never the path: a user file left as it was could
/// loosen the session, so Claude is not started.
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

/// Whether Claude's user settings in `home` still say what [`write_claude_settings`] wrote,
/// give or take the keys Claude Code writes there itself ([`claude_own_key`]):
/// `settings.json` a plain file (not a link) whose JSON object holds [`CLAUDE_HOME_SETTINGS`]
/// exactly and no other key, and no `cowork_settings.json` at all. The JSON is compared, not
/// the bytes: Claude Code rewrites its user file through its own settings writer
/// (`bn("userSettings", …)`), which may change the spacing or key order. A file that cannot be
/// read, is over 64 KiB or does not parse does not match. File system calls: two stats and one
/// small read.
pub(crate) fn claude_settings_intact(home: &Path) -> bool {
    use std::io::Read as _;
    const MAX: u64 = 64 * 1024;
    if std::fs::symlink_metadata(home.join(CLAUDE_COWORK_SETTINGS_FILE)).is_ok() {
        return false;
    }
    let path = home.join(CLAUDE_SETTINGS_FILE);
    if !std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let Ok(file) = crate::write::open_read_nofollow(&path) else {
        return false;
    };
    if !file.metadata().is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let mut bytes = Vec::new();
    if file.take(MAX + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 > MAX {
        return false;
    }
    let Ok(Value::Object(mut settings)) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    settings.retain(|key, value| !claude_own_key(key, value));
    settings == home_settings()
}

/// Whether `key: value` is one Claude Code 2.1.293 writes to its user file itself, with a value
/// that loosens nothing, so the per-prompt check lets it be. Each is read in the binary bundled
/// with claude-agent-sdk 0.3.293 (`bn("userSettings", {…})`):
///
/// - `model`, a string: the model picker and `/model` (`bn("userSettings",{model:n})`, and the
///   `opus`, `sonnet` and `fable` defaults it writes). Clauseo passes the model per session.
/// - `fastMode`, a bool: the fast-mode toggle (`bn("userSettings",{fastMode:e?!0:void 0})`).
/// - `skipAutoPermissionPrompt`, a bool: the auto-mode opt-in dialog's answer (schema: "Whether
///   the user has accepted the auto mode opt-in dialog"). Unverified: only its removal was found
///   (`bn("userSettings",{skipAutoPermissionPrompt:void 0})` in a start-up migration); the
///   write that sets it goes through a writer whose key could not be read.
/// - `permissions` holding `defaultMode` alone, set to `auto`, `default`, `acceptEdits` or
///   `plan`: the auto-mode nudge's accept (`bn("userSettings",{permissions:{defaultMode:
///   "auto"}})`, two spots). The flag settings pin the mode (`loads::pinned`), so this one
///   counts for nothing. `bypassPermissions` and `dontAsk`, or any other key in
///   `permissions`, still fail the check.
///
/// The writer drops a key set to `undefined` (`fastMode: void 0` at start-up), which leaves
/// Clauseo's file as it was.
fn claude_own_key(key: &str, value: &Value) -> bool {
    match key {
        "model" => value.is_string(),
        "fastMode" | "skipAutoPermissionPrompt" => value.is_boolean(),
        "permissions" => value.as_object().is_some_and(|permissions| {
            permissions.len() == 1
                && permissions
                    .get("defaultMode")
                    .and_then(Value::as_str)
                    .is_some_and(|mode| ["auto", "default", "acceptEdits", "plan"].contains(&mode))
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
