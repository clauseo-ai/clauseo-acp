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
/// The mode every Codex session runs in, with no Clauseo sandbox (tier C). codex-acp describes
/// it as "edit workspace files; ask before writing outside the workspace" (`dist/index.js:
/// 34277-34281`): Codex creates, changes and deletes files inside the matter folder without
/// asking (G8a saw it, clauseo-acp-private #255), and asks before writing outside it or
/// escalating. Undo per turn is the lawyer's way back.
pub(crate) const ASK_MODE: &str = "workspace-write";
/// The variable the Codex app-server takes its home from.
pub(crate) const HOME_VAR: &str = "CODEX_HOME";
/// The variable codex-acp reads its config from.
pub(crate) const CONFIG_VAR: &str = "CODEX_CONFIG";
/// Set to `true`, codex-acp passes the session's MCP servers as they are, without dropping those
/// it judges duplicates (`dist/index.js:35465-35467, 35820-35821`): Clauseo already chose them.
pub(crate) const MCP_FILTER_VAR: &str = "DISABLE_MCP_CONFIG_FILTERING";

/// The variables a Codex agent never gets, from any source (inherited, the settings' agent
/// variables or the profile hook's), whatever their case. `CODEX_PATH` swaps the app-server
/// binary; `APP_SERVER_LOGS` makes codex-acp log the whole `CODEX_CONFIG`, hooks included, to
/// a file (`dist/index.js:27641-27675, 45673`); `DEFAULT_AUTH_REQUEST` picks a sign-in;
/// `MODEL_PROVIDER` picks a provider, where the home's `config.toml` defines none and sign-in
/// is the login Codex keeps in its home (0025).
pub(crate) const DROPPED_VARS: [&str; 4] = [
    "CODEX_PATH",
    "APP_SERVER_LOGS",
    "DEFAULT_AUTH_REQUEST",
    "MODEL_PROVIDER",
];

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
    profile: Map<String, Value>,
    hooks: Option<Map<String, Value>>,
) -> BTreeMap<OsString, OsString> {
    let config = Value::Object(config(settings_env, profile, hooks)).to_string();
    BTreeMap::from([
        (OsString::from(MODE_VAR), OsString::from(ASK_MODE)),
        (OsString::from(HOME_VAR), home.as_os_str().to_owned()),
        (OsString::from(CONFIG_VAR), OsString::from(config)),
        (OsString::from(MCP_FILTER_VAR), OsString::from("true")),
    ])
}

/// The top-level keys `config` takes from the launch profile's side
/// (`agent::loads::profile::codex_keys`): the profile's own [`PROFILE_KEYS`], and the host's
/// `web_search` and `sandbox_workspace_write` (only its `writable_roots`, the session's work
/// and notes folders). Any other key is refused with a warning: `hooks`, `mcp_servers`,
/// `sandbox_mode`, `approval_policy`, `skills`, `features`, `projects`, `model_providers`,
/// `profiles`, `notify` and the rest.
const LAUNCH_KEYS: [&str; 3] = [
    "developer_instructions",
    "web_search",
    "sandbox_workspace_write",
];

/// The `CODEX_CONFIG` keys the launch profile itself may supply: what R21 and R22 need, which
/// is the instructions (`clauseo_profile::CodexConfig::with_instructions` emits nothing else).
pub(crate) const PROFILE_KEYS: [&str; 1] = ["developer_instructions"];

/// The Windows sandbox Codex gets on Windows, as `windows.sandbox`: Codex's restricted-token
/// sandbox, which needs no admin setup and which G8a verified (clauseo-acp-private #281). With no
/// Windows sandbox set, codex 0.162.1 turns `workspace-write` into read-only on Windows
/// (`codex-rs/config/src/config_toml.rs:687-700` at `rust-v0.162.1`), so under the ask mode it
/// asks before every shell command, reads included (#274's by-hand proof). Record 0025; it is
/// revisited when stage C's Windows sandbox (S15) lands.
pub(crate) const WINDOWS_SANDBOX: &str = "unelevated";

/// The `CODEX_CONFIG` object: [`MODEL_KEYS`] from a settings `CODEX_CONFIG` that parses as a
/// JSON object; then the launch profile's keys (`agent::loads::profile::codex_keys`), only
/// [`LAUNCH_KEYS`], merged add-only ([`merge_add_only`]); then every one of
/// `clauseo_limits::CODEX_OVERRIDES` (apps, plugins, skill MCP installs, cloud skills and
/// bundled skills off, and no skills instructions) as nested keys over them all, so R15's pins
/// win; then `hooks`, the object the [hooks seam](crate::actor::turn) gives, when there is one,
/// over everything. On Windows, `windows.sandbox` is [`WINDOWS_SANDBOX`], set with R15's pins.
/// Nothing here reaches `<agent_home>/config.toml` ([`home_config`]).
pub(crate) fn config(
    settings_env: &BTreeMap<String, String>,
    profile: Map<String, Value>,
    hooks: Option<Map<String, Value>>,
) -> Map<String, Value> {
    config_on(settings_env, profile, hooks, cfg!(windows))
}

/// [`config`] as it is on Windows when `windows` is true, and elsewhere when it is false.
pub(crate) fn config_on(
    settings_env: &BTreeMap<String, String>,
    profile: Map<String, Value>,
    hooks: Option<Map<String, Value>>,
    windows: bool,
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
    let mut profile = profile;
    profile.retain(|key, _| {
        let allowed = LAUNCH_KEYS.contains(&key.as_str());
        if !allowed {
            tracing::warn!(
                key,
                "a CODEX_CONFIG key the launch profile may not set is refused"
            );
        }
        allowed
    });
    merge_add_only(&mut config, profile);
    for set in clauseo_limits::CODEX_OVERRIDES {
        if let Some((key, value)) = set.split_once('=') {
            insert_dotted(&mut config, key, toml_value(value));
        }
    }
    if windows {
        insert_dotted(&mut config, "windows.sandbox", Value::from(WINDOWS_SANDBOX));
    }
    if let Some(hooks) = hooks {
        config.insert("hooks".to_owned(), Value::Object(hooks));
    }
    config
}

/// Merges `keys` into `config` without replacing anything: objects on both sides merge key by
/// key, arrays on both sides gain the items they lack, and any other value already there stays
/// (the new one is dropped with a warning). A dotted key, which codex reads as a path, clashes
/// with its first part: it goes in only when `config` has no key of that first part.
pub(crate) fn merge_add_only(config: &mut Map<String, Value>, keys: Map<String, Value>) {
    for (key, value) in keys {
        if let Some((first, _)) = key.split_once('.') {
            if config.contains_key(first) || config.contains_key(&key) {
                tracing::warn!(
                    key,
                    "a dotted CODEX_CONFIG key clashes with one already set"
                );
            } else {
                config.insert(key, value);
            }
            continue;
        }
        match (config.get_mut(&key), value) {
            (None, value) => {
                config.insert(key, value);
            }
            (Some(Value::Object(held)), Value::Object(value)) => merge_add_only(held, value),
            (Some(Value::Array(held)), Value::Array(value)) => {
                for item in value {
                    if !held.contains(&item) {
                        held.push(item);
                    }
                }
            }
            (Some(_), _) => {
                tracing::warn!(
                    key,
                    "a CODEX_CONFIG key already set is kept over the profile's"
                );
            }
        }
    }
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

/// The header of the `config.toml` the host writes in a Codex agent's home.
const HOME_CONFIG_HEADER: &str = "# Clauseo manages this file. It is written again each time \
     Clauseo starts Codex, and any edit is replaced.\n# The switches Clauseo turns off for Codex's \
     app-server at start-up (clauseo_limits::CODEX_OVERRIDES).\n";

/// The `config.toml` text for a Codex agent's home: [`HOME_CONFIG_HEADER`], then each of
/// `clauseo_limits::CODEX_OVERRIDES` as a TOML dotted key (each override is already TOML:
/// `key=value` as Codex's `-c` takes it).
pub(crate) fn home_config() -> String {
    let mut text = HOME_CONFIG_HEADER.to_owned();
    for set in clauseo_limits::CODEX_OVERRIDES {
        if let Some((key, value)) = set.split_once('=') {
            text.push_str(key);
            text.push_str(" = ");
            text.push_str(value);
            text.push('\n');
        }
    }
    text
}

/// Writes [`home_config`] to `<home>/config.toml`, atomically and 0600 on Unix, over whatever is
/// there. codex-acp 2.2.2 gives `CODEX_CONFIG` only to the session (as each thread's `config`)
/// and starts the app-server with `-c` for hooks alone (`dist/index.js:45590-45601`), so
/// app-server starts from this file: without it plugins are on, and every start syncs a plugin
/// cache into `CODEX_HOME/plugins` (G8a counted 176 folders and 57 skills;
/// `core-plugins/src/manager.rs:2814-2824` at `rust-v0.162.1`), fetched from
/// `github.com/openai/plugins` (#203), whose skills reach the session's command list. Nothing
/// else in the home is read or touched: `auth.json` and the rest are Codex's.
///
/// The hooks table never goes in this file, only through `CODEX_CONFIG`: the guard's trust entry
/// is keyed by the session-flags source (`/<session-flags>/config.toml:pre_tool_use:0:0`), so a
/// hook written here would be keyed by this file's path, its `trusted_hash` would not match,
/// and Codex would silently not run it.
///
/// A `config.toml` that already holds exactly these bytes (a plain file, 0600 on Unix) is left
/// as it is: no write, so no rename over a file another program holds open on Windows. Before a
/// write, temp files a crashed write left beside it (`config.toml.clauseo-*.new`) are removed.
pub(crate) fn write_home_config(home: &Path) -> Result<(), Error> {
    let path = home.join("config.toml");
    let text = home_config();
    if holds(&path, text.as_bytes()) {
        return Ok(());
    }
    crate::write::sweep_temp_files(&path);
    crate::write::replace_private_file(&path, text.as_bytes())
        .map_err(|error| home_failed(&path, &error.to_string()))
}

/// Whether the file at `path`, our own `config.toml`, is a plain file (0600 on Unix) holding
/// exactly `bytes`.
fn holds(path: &Path, bytes: &[u8]) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    let private = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o777 == 0o600
    };
    #[cfg(not(unix))]
    let private = true;
    meta.is_file()
        && private
        && usize::try_from(meta.len()).is_ok_and(|len| len == bytes.len())
        && std::fs::read(path).is_ok_and(|held| held == bytes)
}

/// The failure of a Codex launch whose home or `config.toml` could not be made: the path and
/// the error go to the log only (the path holds the user's name).
fn home_failed(path: &Path, error: &str) -> Error {
    tracing::warn!(path = %path.display(), %error, "Codex's folder could not be prepared");
    Error::refused(
        ErrorCode::AgentLaunchFailed,
        "Clauseo couldn't prepare Codex's folder.",
    )
}

/// #184: codex-acp marks every session root `trusted` (`dist/index.js:35450-35455`), so a
/// `.codex/` in the matter folder would load as project config (its MCP servers, hooks, model
/// and sandbox switches). A Codex agent is refused in a folder holding a `.codex` entry of any
/// kind, before it starts, and before each of its prompts and session calls (the entry may
/// appear later: codex-acp restarts a crashed app-server and resumes the session itself,
/// `dist/index.js:37755-37772`). `folder` is the matter folder as resolved through links. A
/// stat that fails for any reason but not-found fails closed too, as `matter.folder_missing`
/// (the folder may be a share that does not answer), not as a `.codex` to rename. A `.codex`
/// that is not found passes only when `folder` itself is a folder: a matter folder that is gone
/// is `matter.folder_missing` too, not a pass.
///
/// Lift this once codex-acp has an option to not trust the session root
/// (agentclientprotocol/codex-acp#690), or once the direct Codex driver exists. The guard
/// separately refuses an agent's writes to `.codex/**`.
pub(crate) fn refuse_codex_folder(folder: &Path) -> Result<(), Error> {
    let entry = folder.join(".codex");
    // `symlink_metadata`: a link named `.codex` counts too, wherever it points.
    match std::fs::symlink_metadata(&entry) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::metadata(folder) {
                Ok(meta) if meta.is_dir() => Ok(()),
                Ok(_) => Err(folder_failed(None, "the matter folder is not a folder")),
                Err(error) => Err(folder_failed(
                    Some(error.kind()),
                    "the matter folder's stat failed",
                )),
            }
        }
        Err(error) => Err(folder_failed(
            Some(error.kind()),
            "the matter folder's .codex stat failed",
        )),
        Ok(_) => {
            tracing::warn!(
                folder = %folder.display(),
                "a Codex session was refused: the matter folder holds a .codex entry"
            );
            Err(Error::refused(
                ErrorCode::MatterCodexFolder,
                "Codex can't work in this matter while its folder holds a \".codex\" folder: \
                 Codex would load the settings inside it. Rename or remove that \".codex\" \
                 folder, then try again.",
            ))
        }
    }
}

/// The fail-closed refusal of a Codex session whose matter folder could not be checked: `why`
/// and the io error's kind go to the log, never the path or the error's text (either may hold
/// the user's name).
fn folder_failed(kind: Option<std::io::ErrorKind>, why: &str) -> Error {
    tracing::warn!(error_kind = ?kind, "a Codex session was refused: {why}");
    Error::refused(
        ErrorCode::MatterFolderMissing,
        "Clauseo couldn't check the matter's folder for Codex.",
    )
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
    // Each hook's trust status and the count only: the answer also holds paths.
    let hooks = answer.get("hooks").and_then(Value::as_array);
    let trust: Vec<&str> = hooks
        .into_iter()
        .flatten()
        .map(|hook| {
            hook.get("trustStatus")
                .and_then(Value::as_str)
                .unwrap_or("none")
        })
        .collect();
    tracing::error!(
        hooks = trust.len(),
        ?trust,
        "the Codex guard hook is not trusted, so Codex would not run it; the session start fails"
    );
    Error::refused(
        ErrorCode::AgentLaunchFailed,
        "Codex did not start: Clauseo's file guard could not be turned on for it. No message \
         was sent.",
    )
}

/// Makes `home`, `<data>/agents/<id>/home`, one folder at a time below `data`, each made
/// private to the user on Unix (0700), and `home` set to 0700 when it already exists. Each of
/// `agents`, `agents/<id>` and `home` that exists must be a real folder: a link (or a
/// junction) anywhere below `data` is refused and never followed, so nothing is made where it
/// points. The data folder itself may be a link: `CLAUSEO_DATA_DIR` may name one, and the
/// store lives there too.
pub(crate) fn make_home(data: &Path, home: &Path) -> Result<(), Error> {
    let below = home
        .strip_prefix(data)
        .map_err(|_outside| home_failed(home, "not under the data folder"))?;
    let mut at = data.to_owned();
    for part in below.components() {
        at.push(part);
        let made = match std::fs::symlink_metadata(&at) {
            // Another launch may make it first: `AlreadyExists` is fine, and the check below
            // still refuses a link or a file there.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match crate::write::create_private_dir(&at) {
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
                    made => made,
                }
            }
            other => other.map(drop),
        };
        // `symlink_metadata` never follows a link, so a link is not a folder here.
        let meta = made
            .and_then(|()| std::fs::symlink_metadata(&at))
            .map_err(|error| home_failed(&at, &error.to_string()))?;
        if !meta.is_dir() {
            return Err(home_failed(&at, "a link or not a folder"));
        }
    }
    // Off Unix a folder's privacy is its parent's: there is nothing to set.
    #[cfg(unix)]
    crate::write::make_private_dir(home).map_err(|error| home_failed(home, &error.to_string()))?;
    Ok(())
}

/// A Codex launch's checks and writes, in order, on the `clauseo-codex-fs` thread
/// ([`prepare::off_actor`]), after every agent's data-folder refusal
/// (`agent/data_folder.rs`, which covers every agent's home): the matter folder `stored`
/// resolved through links ([`crate::agent::session_folder`], the spelling the agent runs in),
/// the `.codex` refusal, the home made under `data`, then `config.toml`. The resolved folder
/// comes back.
pub(crate) fn prepare_launch(stored: &Path, data: &Path, home: &Path) -> Result<PathBuf, Error> {
    let folder = crate::agent::session_folder(stored);
    refuse_codex_folder(&folder)?;
    make_home(data, home)?;
    write_home_config(home)?;
    Ok(folder)
}

pub(crate) mod prepare;

#[cfg(test)]
mod tests;
