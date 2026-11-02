//! What the host forces on a Codex agent (codex-acp 2.2.2 over `@openai/codex` 0.162.1): its
//! launch variables, its home, the `.codex` refusal (#184) and the guard's trust check.
//!
//! codex-acp reads `CODEX_CONFIG` with `JSON.parse` (`dist/index.js:45657-45660`) and sends it,
//! less `hooks`, as the `config` object of each `thread/start`, `thread/resume` and `thread/fork`
//! (`:35231-35336`), so the keys are nested objects, never dotted strings. Only `hooks` becomes
//! a `-c hooks=…` argument, once, when it starts the app-server (`:45590-45601`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;
use serde_json::{Map, Value};

use crate::error::Error;

/// The variable codex-acp takes its first mode from (`dist/index.js:34362-34367`).
pub(crate) const MODE_VAR: &str = "INITIAL_AGENT_MODE";
/// Codex's ask mode, the one every session runs in with no Clauseo sandbox (tier C): it edits
/// the matter folder only with the lawyer's approval.
pub(crate) const ASK_MODE: &str = "workspace-write";
/// The variable the Codex app-server takes its home from.
pub(crate) const HOME_VAR: &str = "CODEX_HOME";
/// The variable codex-acp reads its config from.
pub(crate) const CONFIG_VAR: &str = "CODEX_CONFIG";
/// Set to `true`, codex-acp passes the session's MCP servers as they are, without dropping those
/// it judges duplicates (`dist/index.js:35465-35467, 35820-35821`): Clauseo already chose them.
pub(crate) const MCP_FILTER_VAR: &str = "DISABLE_MCP_CONFIG_FILTERING";

/// The keys of a settings `CODEX_CONFIG` the host keeps, and only these: the model and its
/// reasoning effort, so a by-hand check can pick the cheapest model. Any other key in it is
/// dropped, so no settings value can undo a switch the host sets.
pub(crate) const MODEL_KEYS: [&str; 2] = ["model", "model_reasoning_effort"];

/// The variables a Codex agent is launched with, over any settings or profile value of the same
/// name: the ask mode, `home` as `CODEX_HOME`, [`config`] as `CODEX_CONFIG`, and no MCP
/// filtering.
pub(crate) fn launch_env(
    home: &Path,
    settings_env: &BTreeMap<String, String>,
    hooks: Option<Map<String, Value>>,
) -> BTreeMap<OsString, OsString> {
    let config = Value::Object(config(settings_env, hooks)).to_string();
    BTreeMap::from([
        (OsString::from(MODE_VAR), OsString::from(ASK_MODE)),
        (OsString::from(HOME_VAR), home.as_os_str().to_owned()),
        (OsString::from(CONFIG_VAR), OsString::from(config)),
        (OsString::from(MCP_FILTER_VAR), OsString::from("true")),
    ])
}

/// The `CODEX_CONFIG` object: [`MODEL_KEYS`] from a settings `CODEX_CONFIG` that parses as a
/// JSON object, then every one of `clauseo_limits::CODEX_OVERRIDES` (apps, plugins, skill MCP
/// installs, cloud skills and bundled skills off) as nested keys over them, then `hooks`, the
/// object the [hooks seam](crate::actor::turn) gives, when there is one.
pub(crate) fn config(
    settings_env: &BTreeMap<String, String>,
    hooks: Option<Map<String, Value>>,
) -> Map<String, Value> {
    let mut config = Map::new();
    let from_settings = settings_env
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(CONFIG_VAR))
        .and_then(|(_, text)| serde_json::from_str::<Map<String, Value>>(text).ok());
    for (key, value) in from_settings.into_iter().flatten() {
        if MODEL_KEYS.contains(&key.as_str()) && value.is_string() {
            config.insert(key, value);
        }
    }
    for set in clauseo_limits::CODEX_OVERRIDES {
        if let Some((key, value)) = set.split_once('=') {
            insert_dotted(&mut config, key, toml_value(value));
        }
    }
    if let Some(hooks) = hooks {
        config.insert("hooks".to_owned(), Value::Object(hooks));
    }
    config
}

/// Sets `value` at the dotted `key` (`a.b.c`), making each object on the way and replacing any
/// value there that is not one.
fn insert_dotted(config: &mut Map<String, Value>, key: &str, value: Value) {
    let mut parts = key.split('.').peekable();
    let mut at = config;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            at.insert(part.to_owned(), value);
            return;
        }
        let slot = at
            .entry(part.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        let Value::Object(inner) = slot else {
            return;
        };
        at = inner;
    }
}

/// A `-c` override's TOML value as JSON: `true` and `false`, a number or a quoted string as JSON
/// reads it, and anything else as a bare string (as Codex's own `-c` parser falls back to).
fn toml_value(text: &str) -> Value {
    match text {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => serde_json::from_str(text).unwrap_or_else(|_| Value::from(text)),
    }
}

/// #184: codex-acp marks every session root `trusted` (`dist/index.js:35450-35455`), so a
/// `.codex/` in the matter folder would load as project config (its MCP servers, hooks, model
/// and sandbox switches). A Codex agent is refused in a folder holding a `.codex` entry of any
/// kind, before it starts. `folder` is the matter folder as resolved through links.
///
/// Lift this once codex-acp has an option to not trust the session root
/// (agentclientprotocol/codex-acp#690), or once the direct Codex driver exists. The guard
/// separately refuses an agent's writes to `.codex/**`.
pub(crate) fn refuse_codex_folder(folder: &Path) -> Result<(), Error> {
    let entry = folder.join(".codex");
    // `symlink_metadata`: a link named `.codex` counts too, wherever it points.
    if std::fs::symlink_metadata(&entry).is_ok() {
        tracing::warn!(
            folder = %folder.display(),
            "a Codex session was refused: the matter folder holds a .codex entry"
        );
        return Err(Error::refused(
            ErrorCode::MatterFolderRefused,
            format!(
                "Codex can't work in this matter while its folder holds a \".codex\" folder \
                 ({}). Codex would load the settings inside it. Rename or remove that \".codex\" \
                 folder, then try again.",
                entry.display()
            ),
        ));
    }
    Ok(())
}

/// The matter folder `stored` as resolved through links, or `stored` as it is when it does not
/// resolve.
// replaced by agent::session_folder once #231 lands
pub(crate) fn session_folder(stored: &Path) -> PathBuf {
    dunce::canonicalize(stored).unwrap_or_else(|_| stored.to_owned())
}

/// Whether `_codex/hooks/list`'s answer shows the guard trusted: at least one session-flags hook
/// (codex-acp lists only those, `dist/index.js:39929-39933`), and each with `trustStatus`
/// `trusted`. Codex silently skips a hook that is not trusted (`hooks/.../discovery.rs:714-721`
/// at `rust-v0.162.1`), so anything else means the guard would not run.
pub(crate) fn guard_trusted(answer: &Value) -> bool {
    let hooks = answer.get("hooks").and_then(Value::as_array);
    hooks.is_some_and(|hooks| {
        !hooks.is_empty()
            && hooks
                .iter()
                .all(|hook| hook.get("trustStatus").and_then(Value::as_str) == Some("trusted"))
    })
}

/// The failure of a Codex session whose guard is not trusted.
pub(crate) fn guard_untrusted(answer: &Value) -> Error {
    tracing::error!(
        %answer,
        "the Codex guard hook is not trusted, so Codex would not run it; the session start fails"
    );
    Error::refused(
        ErrorCode::AgentLaunchFailed,
        "Codex did not start: Clauseo's file guard could not be turned on for it. No message \
         was sent.",
    )
}

/// Makes `home` (and its parents) for the agent, private to the user on Unix (0700). A `home`
/// that is a link or not a folder is refused, as the limits reader refuses it.
pub(crate) fn make_home(home: &Path) -> Result<(), Error> {
    let failed = |error: String| {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("the agent's home {}: {error}", home.display()),
        )
    };
    crate::write::create_private_dir_all(home).map_err(|error| failed(error.to_string()))?;
    let meta = std::fs::symlink_metadata(home).map_err(|error| failed(error.to_string()))?;
    if !meta.is_dir() {
        return Err(failed("not a folder".to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
