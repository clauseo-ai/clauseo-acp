//! Claude Code's OS policy tiers, beside `managed-settings.json`: a settings document whose
//! `env` moves its route or host as that file's does ([`super::routes`]). Claude Code 2.1.293
//! (`@anthropic-ai/claude-agent-sdk-*` 0.3.293; byte offsets in its darwin-arm64 `claude` and
//! win32-x64 `claude.exe`):
//!
//! - **macOS managed preferences** (darwin 184595107, 184595517, 184595623): the domain
//!   `com.anthropic.claudecode`, read from `/Library/Managed Preferences/<user>/<domain>.plist`
//!   (`os.userInfo().username`), then `/Library/Managed Preferences/<domain>.plist`, each turned
//!   to JSON by `/usr/bin/plutil -convert json -o - --` (184595226). The whole property list is
//!   the settings document, so its top-level `env` dictionary is the one read.
//! - **The Windows registry** (win32 209565901, 210983054): the value `Settings` of
//!   `HKLM\SOFTWARE\Policies\ClaudeCode` and of `HKCU\SOFTWARE\Policies\ClaudeCode`, read by
//!   `reg.exe query <key> /v Settings` and taken only as `REG_SZ` or `REG_EXPAND_SZ`
//!   (`REG_(?:EXPAND_)?SZ`, 210996797); its text is a settings document in JSON.
//!
//! Precedence: the first plist with policy content is the macOS managed-preferences tier
//! (darwin 186014972, 186030378), then HKLM (186030532); HKCU applies only when no admin tier
//! has content (186031607). The admin tiers' `env` objects are then joined, not picked
//! (`CLAUDE_CODE_DISABLE_ADMIN_ENV_UNION`, `tiersWithEnv`; darwin 185036794, 185039368), so a
//! route key in any of them reaches Claude Code. Every tier is read here, HKCU and the second
//! plist too: one that would not apply only costs a probe.
//!
//! A tier that is not there changes nothing. One that is there but cannot be read, is over
//! [`super::MAX_FILE`] or does not parse moves the agent, as an unreadable `managed-settings.json`
//! does; the log names only the tier.

use std::io::Cursor;
#[cfg(any(target_os = "macos", test))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(test)]
mod tests;

/// Claude Code's managed-preferences domain.
#[cfg(any(target_os = "macos", test))]
const DOMAIN: &str = "com.anthropic.claudecode";

/// What a tier read gives: its settings text (`None` when it is not there), or `Err` when it is
/// there but cannot be read.
pub(super) type TierRead = Result<Option<String>, ()>;

/// Whether this machine's OS policy tiers move Claude Code; on the launch prep's thread.
pub(super) fn moved() -> bool {
    plists_moved(&plist_paths()) || registry_moved(&registry_values())
}

/// The managed-preferences files Claude Code reads on this machine: none but on macOS.
fn plist_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let user = clauseo_settings::policy::managed_prefs::login_user()
            .ok()
            .flatten();
        plist_paths_for(user.as_deref())
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// The managed-preferences files for the login user `user` (the computer's alone without one,
/// as Claude Code skips the user's file when it finds no name).
#[cfg(any(target_os = "macos", test))]
fn plist_paths_for(user: Option<&str>) -> Vec<PathBuf> {
    use clauseo_settings::policy::managed_prefs::{MANAGED_DIR, file_paths};
    user.and_then(|user| file_paths(DOMAIN, user).ok())
        .unwrap_or_else(|| vec![Path::new(MANAGED_DIR).join(format!("{DOMAIN}.plist"))])
}

/// The registry's `Settings` values, HKLM's then HKCU's: none but on Windows.
fn registry_values() -> Vec<TierRead> {
    clauseo_settings::claude_registry_settings()
}

/// Whether one of the property lists at `paths` moves Claude Code ([`plist_moves`]).
fn plists_moved(paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| match super::read(path) {
        Ok(None) => false,
        Ok(Some(bytes)) => plist_moves(&bytes),
        Err(()) => unreadable("managed_preferences"),
    })
}

/// Whether a managed-preferences property list (XML or binary) moves Claude Code: its top-level
/// `env` is a dictionary with a key [`super::routes`] matches, or no dictionary at all, or the
/// file is no property list. An empty file is nothing, as Claude Code reads it.
fn plist_moves(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let Ok(value) = plist::Value::from_reader(Cursor::new(bytes)) else {
        return true;
    };
    let Some(settings) = value.as_dictionary() else {
        return false;
    };
    match settings.get("env") {
        None => false,
        Some(plist::Value::Dictionary(env)) => env.keys().any(|key| super::routes(key)),
        Some(_) => true,
    }
}

/// Whether one of the registry's `Settings` values moves Claude Code: its JSON's `env`
/// [`super::routes`], it is no JSON, or it could not be read. An empty value is nothing.
fn registry_moved(values: &[TierRead]) -> bool {
    values.iter().any(|value| match value {
        Ok(None) => false,
        Ok(Some(text)) => !text.trim().is_empty() && super::json_moves(text.trim_end().as_bytes()),
        Err(()) => unreadable("registry"),
    })
}

/// Logs that `tier` is there but cannot be read; true, as it moves the agent.
fn unreadable(tier: &'static str) -> bool {
    tracing::info!(
        tier,
        "offline probe: a Claude Code policy tier cannot be read"
    );
    true
}
