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
/// installs, cloud skills and bundled skills off, and no skills instructions) as nested keys
/// over them, then `hooks`, the object the [hooks seam](crate::actor::turn) gives, when there is
/// one.
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
/// as it is: no write, so no rename over a file another program holds open on Windows.
pub(crate) fn write_home_config(home: &Path) -> Result<(), Error> {
    let path = home.join("config.toml");
    let text = home_config();
    if holds(&path, text.as_bytes()) {
        return Ok(());
    }
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
/// stat that fails for any reason but not-found counts as present: the check fails closed.
///
/// Lift this once codex-acp has an option to not trust the session root
/// (agentclientprotocol/codex-acp#690), or once the direct Codex driver exists. The guard
/// separately refuses an agent's writes to `.codex/**`.
pub(crate) fn refuse_codex_folder(folder: &Path) -> Result<(), Error> {
    let entry = folder.join(".codex");
    // `symlink_metadata`: a link named `.codex` counts too, wherever it points.
    match std::fs::symlink_metadata(&entry) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        found => {
            tracing::warn!(
                folder = %folder.display(),
                error = ?found.err(),
                "a Codex session was refused: the matter folder holds a .codex entry, or its \
                 stat failed"
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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                crate::write::create_private_dir(&at)
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
    crate::write::make_private_dir(home).map_err(|error| home_failed(home, &error.to_string()))
}

/// Refuses a Codex launch whose matter folder `folder` (resolved through links) is the agent's
/// home, holds it, or sits inside it: an agent working there could read the login or write
/// what Codex loads from the home. `home` exists by now; it is resolved the same way. On
/// macOS and Windows the names compare without case, as their file systems do by default.
pub(crate) fn refuse_home_overlap(folder: &Path, home: &Path) -> Result<(), Error> {
    let resolved =
        dunce::canonicalize(home).map_err(|error| home_failed(home, &error.to_string()))?;
    if !(within(folder, &resolved) || within(&resolved, folder)) {
        return Ok(());
    }
    tracing::warn!(
        folder = %folder.display(),
        home = %resolved.display(),
        "a Codex session was refused: the matter folder overlaps Codex's folder"
    );
    Err(Error::refused(
        ErrorCode::MatterFolderRefused,
        "Codex can't work in this matter: its folder overlaps Clauseo's own data, where Codex \
         keeps its sign-in.",
    ))
}

/// Whether `inner` is `outer` or sits inside it, comparing whole names.
fn within(inner: &Path, outer: &Path) -> bool {
    let fold = cfg!(any(windows, target_os = "macos"));
    let same = |a: std::path::Component<'_>, b: std::path::Component<'_>| {
        if fold {
            a.as_os_str().to_string_lossy().to_lowercase()
                == b.as_os_str().to_string_lossy().to_lowercase()
        } else {
            a == b
        }
    };
    let mut inner = inner.components();
    outer
        .components()
        .all(|part| inner.next().is_some_and(|other| same(other, part)))
}

/// A Codex launch's checks and writes, in order, on the `clauseo-codex-fs` thread
/// ([`prepare::off_actor`]): the matter folder `stored` resolved through links, the `.codex`
/// refusal, the home made under `data`, the overlap refusal, then `config.toml`. The resolved
/// folder comes back.
pub(crate) fn prepare_launch(stored: &Path, data: &Path, home: &Path) -> Result<PathBuf, Error> {
    let folder = session_folder(stored);
    refuse_codex_folder(&folder)?;
    make_home(data, home)?;
    refuse_home_overlap(&folder, home)?;
    write_home_config(home)?;
    Ok(folder)
}

pub(crate) mod prepare;

#[cfg(test)]
mod tests;
