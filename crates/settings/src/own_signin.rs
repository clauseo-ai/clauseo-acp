//! Whether Claude Code would offer its own Anthropic sign-in, the lawyer's claude.ai OAuth token,
//! to a custom endpoint (decision record 0048, "A firm endpoint needs a key").
//!
//! Claude Code 2.1.293 (`isAnthropicAuthEnabled`) keeps its own sign-in on unless one of these
//! holds, and its client then sends that token as a Bearer to `ANTHROPIC_BASE_URL`, whatever host
//! it names. Re-check the list at each Claude Code bump (`hook-premise-kit/BUMP-RECHECK.md`):
//!
//! - bare mode (`--bare`): Clauseo never passes it;
//! - a unix socket for the API: Clauseo never sets one;
//! - a provider other than first-party: `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX` or
//!   `CLAUDE_CODE_USE_FOUNDRY` truthy (a plan's `provider` sets the first two);
//! - `ANTHROPIC_AUTH_TOKEN` set (a plan's `env` can never set it: `plans.rs`);
//! - an API key (`ANTHROPIC_API_KEY`, the stored key) or an `apiKeyHelper` in its settings.
//!
//! A custom `ANTHROPIC_BASE_URL` turns none of it off; the default one, `https://api.anthropic.com`
//! in any spelling ([`is_default_address`]), is no custom endpoint. Bare mode, a socket and
//! `apiKeyHelper` are not looked at here: missing them only refuses a launch that was safe.
//!
//! The variables checked are those of the launch's final env together with the `env` Claude Code
//! applies over it from its settings files and policy tiers ([`SettingsEnv`], `sources.rs`).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use crate::schema::{Plan, PlanProvider};

#[cfg(windows)]
mod registry;
mod sources;

pub use sources::{ClaudeSources, SettingsEnv, claude_managed_dir};

/// The base URL variable.
const BASE_URL: &str = "ANTHROPIC_BASE_URL";

/// The variables that turn the sign-in off when set and not empty.
const KEY_VARS: [&str; 2] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

/// The vendor's own address, which a base URL may name without being a custom endpoint.
const DEFAULT_ADDRESS: &str = "https://api.anthropic.com";

/// The variables that name a provider other than first-party when truthy.
const PROVIDER_VARS: [&str; 3] = [
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

/// Whether `plan` sends Claude to a custom endpoint with Claude Code's own sign-in still on: an
/// endpoint and the first-party provider (`anthropic` or none). Bedrock and Vertex use the cloud's
/// credentials and turn that sign-in off, so they launch with no key as before.
pub fn plan_keeps_own_signin(plan: &Plan) -> bool {
    plan.endpoint
        .as_deref()
        .is_some_and(|endpoint| !is_default_address(endpoint))
        && !matches!(
            plan.provider,
            Some(PlanProvider::Bedrock | PlanProvider::Vertex)
        )
}

/// Whether a Claude launch with the whole environment `env`, under its settings sources'
/// `settings`, would offer Claude Code's own sign-in to a custom endpoint: `ANTHROPIC_BASE_URL`
/// set to other than the default address (or `settings` unsure, so it may be), and nothing on the
/// module's list turns that sign-in off. `env` is read as the spawn applies it, then `settings`'
/// pairs over it: the last value of a name wins, names in any ASCII case being one on Windows,
/// as there `std::process::Command` keeps the last spelling and Node reads any.
pub fn own_signin_reaches_endpoint<'a>(
    env: impl IntoIterator<Item = (&'a OsStr, &'a OsStr)>,
    settings: &SettingsEnv,
) -> bool {
    reaches(env, settings, cfg!(windows))
}

/// [`own_signin_reaches_endpoint`], names folded as on Windows when `windows`.
fn reaches<'a>(
    env: impl IntoIterator<Item = (&'a OsStr, &'a OsStr)>,
    settings: &SettingsEnv,
    windows: bool,
) -> bool {
    let mut last: BTreeMap<String, OsString> = BTreeMap::new();
    let mut put = |name: &OsStr, value: &OsStr| {
        let Some(name) = name.to_str() else {
            return;
        };
        let name = if windows {
            name.to_ascii_uppercase()
        } else {
            name.to_owned()
        };
        if checked(&name) {
            last.insert(name, value.to_owned());
        }
    };
    for (name, value) in env {
        put(name, value);
    }
    for (name, value) in settings.pairs() {
        put(name, value);
    }
    let set = |name: &str| last.get(name).is_some_and(|value| !value.is_empty());
    let base_url = last
        .get(BASE_URL)
        .is_some_and(|url| !url.is_empty() && !url.to_str().is_some_and(is_default_address));
    let off = KEY_VARS.iter().any(|var| set(var))
        || PROVIDER_VARS
            .iter()
            .any(|var| last.get(*var).is_some_and(|value| truthy(value)));
    (base_url || settings.unsure()) && !off
}

/// Whether `url` names the vendor's default address: `https://api.anthropic.com` in any case,
/// with or without a trailing `/` or the default port `:443`.
pub fn is_default_address(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    let url = url.strip_suffix('/').unwrap_or(&url);
    url.strip_suffix(":443").unwrap_or(url) == DEFAULT_ADDRESS
}

/// Whether the check reads the variable `name` (any ASCII case).
fn checked(name: &str) -> bool {
    std::iter::once(BASE_URL)
        .chain(KEY_VARS)
        .chain(PROVIDER_VARS)
        .any(|var| name.eq_ignore_ascii_case(var))
}

/// The `Settings` values of Claude Code's registry policy, HKLM's then HKCU's: text when a
/// `REG_SZ` or `REG_EXPAND_SZ`, `None` when not there, `Err` when unreadable. None but on
/// Windows. The host's offline probe reads them too.
pub fn claude_registry_settings() -> Vec<Result<Option<String>, ()>> {
    #[cfg(windows)]
    {
        registry::settings_values()
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// Claude Code's `isEnvTruthy`: `1`, `true`, `yes` or `on`, in any case, spaces around ignored.
fn truthy(value: &OsStr) -> bool {
    value.to_str().is_some_and(|text| {
        let text = text.trim().to_ascii_lowercase();
        matches!(text.as_str(), "1" | "true" | "yes" | "on")
    })
}

#[cfg(test)]
mod tests;
