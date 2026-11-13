//! What an agent's own settings files change about where it goes (the security re-check,
//! round 2): the probe's target comes from the launch's environment (`service.rs`), but each
//! agent also reads files that move its route or host, which that environment never shows.
//! Where one does, or cannot be read, the launch is [`ReachClass::UnknownService`]: unsure means
//! no probe, and the quiet backstop ends a silent turn. Read on the launch prep's thread, never
//! the actor's; the paths come in, so tests point them at a folder of their own.
//!
//! - **Claude Code 2.1.293** copies the `env` of its global config and every settings source
//!   into its own environment (`applyConfigEnvironmentVariables`, `filterSettingsEnv`):
//!   - the policy source: `<managed>/managed-settings.json`, then each
//!     `<managed>/managed-settings.d/*.json` not starting `.`, in name order, `<managed>` being
//!     `/Library/Application Support/ClaudeCode`, `C:\Program Files\ClaudeCode` or
//!     `/etc/claude-code`, the folder of `managed-mcp.json` too
//!     (`agent::loads::managed_mcp_path`); and its cached remote settings,
//!     `<config>/remote-settings.json`;
//!   - the user source, `<config>/settings.json` (`cowork_settings.json` with cowork
//!     plugins), `<config>` being `CLAUDE_CONFIG_DIR` (the launch sets the agent's home);
//!   - the global config: `<config>/.config.json` when present, else
//!     `<CLAUDE_CONFIG_DIR>/.claude.json`.
//!
//!   A key of an `env` object naming a proxy, a base URL or a provider moves it ([`routes`]).
//!   Clauseo's own `_meta.claudeCode.options` (the `--settings` object, `flagSettings`, and the
//!   SDK's `env`) is read the same way ([`meta_moved`]).
//! - **Codex `rust-v0.162.1`** layers `config.toml` files (`config/src/loader/mod.rs:113-137`):
//!   the system one (`/etc/codex/config.toml`, `%ProgramData%\OpenAI\Codex\config.toml`,
//!   `mod.rs:80, 817-820`), the legacy `/etc/codex/managed_config.toml` (Unix,
//!   `loader/layer_io.rs:22, 222-232`) and `${CODEX_HOME}/config.toml`. One moves it when it
//!   sets `model_provider` other than `openai` (`config_toml.rs:175`,
//!   `model-provider-info/src/lib.rs:79`), `openai_base_url` or `chatgpt_base_url`
//!   (`config_toml.rs:412, 427`), a `[model_providers.*]` with a `base_url`
//!   (`config_toml.rs:329`, `lib.rs:144`), or a `[profiles.*]` with any of them
//!   (`config_toml.rs:363`, `profile_toml.rs:31, 44`).
//!
//! A file that is there but cannot be read, is over [`MAX_FILE`] or does not parse moves it
//! too; one that is not there changes nothing. Logs name only the reason's class.

use std::fs::File;
use std::io::{ErrorKind, Read as _};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::actor::turn::launch_checks::CodexPrepared;

#[cfg(doc)]
use super::ReachClass;

/// The largest settings file read: a larger one is not read, and counts as moving the agent.
pub(crate) const MAX_FILE: u64 = 1 << 20;

/// The most `managed-settings.d` documents read; more count as moving the agent.
const MAX_DROP_INS: usize = 64;

/// The prefixes of the `env` keys that move Claude Code's route, host or provider, matched
/// without case; with every key ending `_PROXY` ([`routes`]).
const ROUTE_PREFIXES: [&str; 5] = [
    "ANTHROPIC_",
    "CLAUDE_CODE_USE_",
    "AWS_",
    "GOOGLE_",
    "VERTEX_",
];

/// The files of Claude Code's config folder whose `env` it applies.
const CLAUDE_CONFIG_FILES: [&str; 5] = [
    "settings.json",
    "cowork_settings.json",
    "remote-settings.json",
    ".config.json",
    ".claude.json",
];

/// What a launch's prep reads, taken from the actor before it leaves for the prep's thread.
#[derive(Debug, Clone)]
pub(crate) struct Reads {
    /// The folder of Claude Code's `managed-settings.json` (that of `managed-mcp.json`).
    managed: PathBuf,
    /// The `config.toml` files Codex reads beside its home's.
    codex: Vec<PathBuf>,
    /// Whether the agent is Claude Code.
    claude: bool,
}

impl Reads {
    /// What `actor`'s next launch of an agent (`claude`: Claude Code) reads.
    pub(crate) fn of(actor: &crate::actor::Actor, claude: bool) -> Self {
        // The unit tests' `fake-agent` has no kind; a test names the one it plays.
        #[cfg(test)]
        let claude =
            claude || actor.turn_path.kind_for_tests == Some(clauseo_protocol::AgentKind::Claude);
        let managed = actor.managed_mcp.parent().map(Path::to_path_buf);
        Self {
            managed: managed.unwrap_or_default(),
            codex: actor.turn_path.codex_config_files.clone(),
            claude,
        }
    }

    /// Whether the launch's agent reads a file that moves its route or host: a Codex agent
    /// (`codex`, its home made), or Claude Code with its config folder `claude_home`. On the
    /// prep's thread.
    pub(crate) fn moved(
        &self,
        codex: Option<&CodexPrepared>,
        claude_home: Option<&Path>,
    ) -> Option<Moved> {
        if let Some(codex) = codex {
            let mut files = self.codex.clone();
            files.push(codex.home.join("config.toml"));
            return codex_moved(&files);
        }
        if !self.claude {
            return None;
        }
        claude_moved(&self.managed, claude_home)
    }
}

/// Why the agent's settings leave its route or host unknown: the one word the log gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Moved {
    /// A Claude Code settings file (or Clauseo's own settings for it) sets such a variable, or
    /// one could not be read.
    SettingsEnv,
    /// A Codex `config.toml` sets a provider or a base URL, or one could not be read.
    CodexConfig,
}

impl Moved {
    /// Its word in the log.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::SettingsEnv => "settings_env",
            Self::CodexConfig => "codex_config",
        }
    }
}

/// Where Claude Code finds its files for this launch: the folder of `managed-settings.json`
/// and the config folder the launch gives it (`CLAUDE_CONFIG_DIR`).
pub(crate) fn claude_moved(managed: &Path, config: Option<&Path>) -> Option<Moved> {
    let mut files = vec![managed.join("managed-settings.json")];
    match drop_ins(&managed.join("managed-settings.d")) {
        Ok(more) => files.extend(more),
        Err(()) => return Some(Moved::SettingsEnv),
    }
    if let Some(config) = config {
        files.extend(CLAUDE_CONFIG_FILES.iter().map(|name| config.join(name)));
    }
    files
        .iter()
        .any(|file| match read(file) {
            Ok(None) => false,
            Ok(Some(bytes)) => serde_json::from_slice::<Value>(without_bom(&bytes))
                .map_or(true, |value| env_routes(value.get("env"))),
            Err(()) => true,
        })
        .then_some(Moved::SettingsEnv)
}

/// The `.json` documents of a `managed-settings.d` folder, in name order (none when it is not
/// there); `Err` when it cannot be listed or holds more than [`MAX_DROP_INS`].
fn drop_ins(folder: &Path) -> Result<Vec<PathBuf>, ()> {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if absent(error.kind()) => return Ok(Vec::new()),
        Err(_) => return Err(()),
    };
    let mut names = Vec::new();
    for entry in entries {
        let name = entry.map_err(drop)?.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".json") && !name.starts_with('.') {
            names.push(name.into_owned());
        }
        if names.len() > MAX_DROP_INS {
            return Err(());
        }
    }
    names.sort();
    Ok(names.into_iter().map(|name| folder.join(name)).collect())
}

/// Whether Clauseo's own `_meta` for a Claude session moves it: an `env` in
/// `claudeCode.options.settings` (Claude Code's `--settings`) or in `claudeCode.options` (the
/// SDK's environment for Claude Code) with a key [`routes`] matches, or `settings` given as a
/// file's path, which the host does not read.
pub(crate) fn meta_moved(meta: &Map<String, Value>) -> bool {
    let options = meta
        .get("claudeCode")
        .and_then(|claude| claude.get("options"));
    let Some(options) = options else {
        return false;
    };
    let settings = options.get("settings");
    matches!(settings, Some(Value::String(_)))
        || env_routes(settings.and_then(|settings| settings.get("env")))
        || env_routes(options.get("env"))
}

/// Whether a settings `env` moves the agent: an object with a key [`routes`] matches, or a
/// value that is not an object at all.
fn env_routes(env: Option<&Value>) -> bool {
    match env {
        None | Some(Value::Null) => false,
        Some(Value::Object(env)) => env.keys().any(|key| routes(key)),
        Some(_) => true,
    }
}

/// Whether an environment variable's name moves Claude Code's route, host or provider: any
/// `*_PROXY`, or a name starting with one of [`ROUTE_PREFIXES`], without case.
pub(crate) fn routes(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    key.ends_with("_PROXY") || ROUTE_PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

/// Whether one of Codex's `config.toml` files moves its provider or host.
pub(crate) fn codex_moved(files: &[PathBuf]) -> Option<Moved> {
    files
        .iter()
        .any(|file| match read(file) {
            Ok(None) => false,
            Ok(Some(bytes)) => std::str::from_utf8(without_bom(&bytes))
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok())
                .is_none_or(|config| codex_config_moves(&config)),
            Err(()) => true,
        })
        .then_some(Moved::CodexConfig)
}

/// Whether a Codex config, or any of its `[profiles.*]`, sets a provider or a base URL.
fn codex_config_moves(config: &toml::Table) -> bool {
    let profiles = match config.get("profiles") {
        None => false,
        Some(toml::Value::Table(profiles)) => profiles
            .values()
            .any(|profile| profile.as_table().is_none_or(codex_layer_moves)),
        Some(_) => true,
    };
    profiles || codex_layer_moves(config)
}

/// Whether one config table sets a provider other than `openai`, a base URL, or a provider
/// with a `base_url`.
fn codex_layer_moves(layer: &toml::Table) -> bool {
    let provider = layer
        .get("model_provider")
        .is_some_and(|provider| provider.as_str() != Some("openai"));
    let base_url = ["openai_base_url", "chatgpt_base_url"]
        .iter()
        .any(|key| layer.contains_key(*key));
    let providers = match layer.get("model_providers") {
        None => false,
        Some(toml::Value::Table(providers)) => providers.values().any(|provider| {
            provider
                .as_table()
                .is_none_or(|provider| provider.contains_key("base_url"))
        }),
        Some(_) => true,
    };
    provider || base_url || providers
}

/// The `config.toml` files Codex reads on this OS beside its home's: the system one and, on
/// Unix, the legacy managed one.
pub(crate) fn codex_system_files() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let data = std::env::var_os("ProgramData")
            .filter(|data| !data.is_empty())
            .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
        vec![data.join("OpenAI").join("Codex").join("config.toml")]
    }
    #[cfg(not(windows))]
    {
        vec![
            PathBuf::from("/etc/codex/config.toml"),
            PathBuf::from("/etc/codex/managed_config.toml"),
        ]
    }
}

/// `path`'s bytes: `None` when it is not there, `Err` when it is there but cannot be read or
/// holds more than [`MAX_FILE`].
fn read(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if absent(error.kind()) => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(drop)?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_FILE) {
        return Err(());
    }
    Ok(Some(bytes))
}

/// Whether an open's error says the file is not there (or its folder is not a folder).
fn absent(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::NotFound | ErrorKind::NotADirectory)
}

/// `bytes` without a leading UTF-8 byte order mark.
fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

#[cfg(test)]
mod tests;
