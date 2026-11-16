//! `shell_environment_policy` in Codex's managed configuration. Clauseo sets the environment of
//! the model's commands itself (their `PATH` among them), so a managed config layer that sets
//! this key refuses a Codex launch.
//!
//! Codex `rust-v0.162.1` takes the key from its config layers (`config_toml.rs:183`); its
//! requirements layers have no such key (`config_requirements.rs:936-985`). The layers read here
//! (`config/src/loader/`, `config_layer_source.rs:31-48`):
//! - the system `config.toml` (`/etc/codex/config.toml`, `%ProgramData%\OpenAI\Codex\config.toml`;
//!   `mod.rs:75, 735-737, 767-769`): below the home's, but a key the home does not set (any of
//!   the policy's but `ignore_default_excludes`) survives the merge;
//! - the legacy `/etc/codex/managed_config.toml` (Unix; `layer_io.rs:21, 204-208`) and, on macOS,
//!   the managed preferences' forced `config_toml_base64` of `com.openai.codex` (`macos.rs:27-28,
//!   57-94`): both over the launch's own `-c` overrides.
//!
//! One that sets the key, at its top or in a `[profiles.*]` table, or that cannot be read or
//! parsed, refuses a Codex launch with `agent.firm_policy_weakens` and the unnamed words.
//! Not read: the config fragments of Codex's cloud bundle (`mod.rs:198-216`), which Codex
//! fetches itself, with no file here to read.

use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;

use crate::error::Error;

/// The key.
const KEY: &str = "shell_environment_policy";

/// The macOS managed-preferences key holding a config layer, base64 TOML.
#[cfg(any(target_os = "macos", test))]
const MDM_CONFIG_KEY: &str = "config_toml_base64";

/// The refusal when one of Codex's managed config layers sets [`KEY`] (the module doc): the
/// system and legacy managed `files` (`settings::codex_system_files`), then, on macOS, the
/// managed preferences.
pub(crate) fn codex_refusal(files: &[PathBuf]) -> Result<(), Error> {
    let found = files
        .iter()
        .find(|file| file_sets(file))
        .map(|file| file.display().to_string())
        .or_else(|| mdm_sets().then(|| "the macOS managed preferences".to_owned()));
    let Some(source) = found else {
        return Ok(());
    };
    tracing::warn!(
        source = %source,
        "a Codex managed config sets {KEY}, or cannot be read: the launch is refused"
    );
    Err(Error::refused(
        ErrorCode::AgentFirmPolicyWeakens,
        format!(
            "the Codex configuration at {source} sets `{KEY}`, or cannot be read; only an \
             administrator can change it, so Codex is not run"
        ),
    ))
}

/// Whether the config file `file` sets [`KEY`] or cannot be read; not there sets nothing.
fn file_sets(file: &Path) -> bool {
    match super::super::read(file) {
        Ok(None) => false,
        Ok(Some(bytes)) => toml_sets(&bytes),
        Err(()) => true,
    }
}

/// Whether a config layer's `bytes` set [`KEY`], or are no TOML table.
fn toml_sets(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(super::super::without_bom(bytes)) else {
        return true;
    };
    toml::from_str::<toml::Table>(text).map_or(true, |config| sets(&config))
}

/// Whether `config` sets [`KEY`] at its top or in one of its profiles.
fn sets(config: &toml::Table) -> bool {
    let profiles = match config.get("profiles") {
        None => false,
        Some(toml::Value::Table(profiles)) => profiles.values().any(|profile| {
            profile
                .as_table()
                .is_none_or(|profile| profile.contains_key(KEY))
        }),
        Some(_) => true,
    };
    config.contains_key(KEY) || profiles
}

/// Whether the macOS managed preferences of `com.openai.codex` hold a config layer that sets
/// [`KEY`], or one that cannot be read; never elsewhere.
fn mdm_sets() -> bool {
    #[cfg(target_os = "macos")]
    {
        use clauseo_settings::policy::managed_prefs::{MANAGED_DIR, file_paths, login_user};
        const DOMAIN: &str = "com.openai.codex";
        let user = login_user().ok().flatten();
        let paths = user
            .and_then(|user| file_paths(DOMAIN, &user).ok())
            .unwrap_or_else(|| vec![Path::new(MANAGED_DIR).join(format!("{DOMAIN}.plist"))]);
        paths.iter().any(|path| match super::super::read(path) {
            Ok(None) => false,
            Ok(Some(bytes)) => plist_sets(&bytes),
            Err(()) => true,
        })
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// Whether a managed-preferences property list's [`MDM_CONFIG_KEY`] holds a config layer that
/// sets [`KEY`], or the list or the key's value cannot be read; a list without the key sets
/// nothing.
#[cfg(any(target_os = "macos", test))]
fn plist_sets(bytes: &[u8]) -> bool {
    use base64::Engine as _;
    if bytes.is_empty() {
        return false;
    }
    let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(bytes)) else {
        return true;
    };
    let Some(layer) = value
        .as_dictionary()
        .and_then(|items| items.get(MDM_CONFIG_KEY))
    else {
        return false;
    };
    let Some(encoded) = layer.as_string() else {
        return true;
    };
    let engine = base64::engine::general_purpose::STANDARD;
    engine
        .decode(encoded.trim())
        .map_or(true, |decoded| toml_sets(&decoded))
}

#[cfg(test)]
#[path = "codex_env_tests.rs"]
mod tests;
