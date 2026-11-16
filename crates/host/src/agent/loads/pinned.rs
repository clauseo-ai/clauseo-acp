//! The single-value security keys every Claude launch sets in its flag settings, whatever the
//! OS, mode or hook ([`pin_security_keys`]).
//!
//! Claude Code 2.1.293 still reads the user settings file under `--setting-sources=user`
//! (`<CLAUDE_CONFIG_DIR>/settings.json`, Clauseo's own Claude home, `agent::home`), and policy
//! settings. It merges them with the flag settings, lowest first: user, flag, policy (`BSr`). An
//! array is joined and de-duplicated, an object merged key by key, and a single value goes to
//! the highest source that sets it (`S4`). So a single value the flag settings leave unset is
//! the user file's to set (`sandbox.enabled: false`, `disableAllHooks: true`), and the file is
//! re-read during the session (a watcher rebuilds the rules and the sandbox on a change). Each
//! key below is set explicitly, so the user file's value never counts. The user file's arrays,
//! hooks and `env` cannot be outranked this way: the launch rewrites that file instead
//! (`agent::home::write_claude_settings`), and the turn checks it before each prompt.
//!
//! The keys, each in 2.1.293's settings schema (read in the bundled binary of
//! claude-agent-sdk 0.3.293):
//!
//! - `disableAllHooks: false`: a merged `true` drops every hook but managed ones, the guard's
//!   `PreToolUse` among them (`if(t.disableAllHooks===!0)return o?.hooks??{}`).
//! - `permissions.defaultMode`: the session's mode (`loads::claude_mode`: `auto`, or `default`
//!   in forced ask). The SDK passes `--permission-mode` too, which comes first; this only keeps
//!   a user `defaultMode` from being the fallback.
//! - `permissions.disableBypassPermissionsMode: "disable"` (schema: `j(["disable"])`, "Disable
//!   the ability to bypass permission prompts"): bypass is off already
//!   (`allowDangerouslySkipPermissions: false`); this closes it in Claude Code as well.
//! - `sandbox.enabled`: on with Claude's sandbox (macOS, Linux), off without it (Windows, see
//!   `loads::sandbox`), so a user file cannot turn on a sandbox Clauseo did not configure.
//! - `sandbox.autoAllowBashIfSandboxed`: on with the sandbox outside forced ask, else off.
//! - `sandbox.allowUnsandboxedCommands: false`, `sandbox.failIfUnavailable: true`: as
//!   `loads::sandbox` explains; on Windows too, so a sandbox turned on by policy fails closed.
//! - `sandbox.allowAppleEvents`, `sandbox.enableWeakerNestedSandbox`,
//!   `sandbox.enableWeakerNetworkIsolation`, `sandbox.network.allowLocalBinding` and
//!   `sandbox.network.allowAllUnixSockets`: all `false`, their defaults. Claude Code's own
//!   remote-tools check names each as weakening the sandbox's isolation ("the sandbox on this
//!   device is configured with a setting that weakens its isolation …").
//!
//! Left out: `disableAutoMode` (Clauseo runs auto), `blockReadsOutsideWorkingDirectories` (only
//! tightens, "true in any settings source wins", and would break reading the skills),
//! `allowManagedHooksOnly`, `allowManagedPermissionRulesOnly` and
//! `sandbox.network.allowManagedDomainsOnly` (policy-only), `skipDangerousModePermissionPrompt`
//! (bypass is disabled), and the arrays (`sandbox.network.allowUnixSockets`, `allowMachLookup`),
//! which only the rewritten user file keeps out.

use serde_json::{Map, Value};

use super::take_object;

/// A pinned key: its path in the settings and its value.
pub(crate) type Pin = (&'static [&'static str], Value);

/// Every key [`pin_security_keys`] sets for a launch with Claude's sandbox (`sandboxed`) or
/// without, in forced ask or not.
pub(crate) fn security_pins(sandboxed: bool, forced_ask: bool) -> Vec<Pin> {
    let mode = super::claude_mode(forced_ask);
    vec![
        (&["disableAllHooks"], Value::Bool(false)),
        (&["permissions", "defaultMode"], Value::from(mode)),
        (
            &["permissions", "disableBypassPermissionsMode"],
            Value::from("disable"),
        ),
        (&["sandbox", "enabled"], Value::Bool(sandboxed)),
        (
            &["sandbox", "autoAllowBashIfSandboxed"],
            Value::Bool(sandboxed && !forced_ask),
        ),
        (&["sandbox", "allowUnsandboxedCommands"], Value::Bool(false)),
        (&["sandbox", "failIfUnavailable"], Value::Bool(true)),
        (&["sandbox", "allowAppleEvents"], Value::Bool(false)),
        (
            &["sandbox", "enableWeakerNestedSandbox"],
            Value::Bool(false),
        ),
        (
            &["sandbox", "enableWeakerNetworkIsolation"],
            Value::Bool(false),
        ),
        (
            &["sandbox", "network", "allowLocalBinding"],
            Value::Bool(false),
        ),
        (
            &["sandbox", "network", "allowAllUnixSockets"],
            Value::Bool(false),
        ),
    ]
}

/// Sets each of [`security_pins`] in `settings` (the flag settings), over whatever the hook or
/// the host's other keys put there; an object on a key's path that is not one is replaced.
pub(crate) fn pin_security_keys(
    settings: &mut Map<String, Value>,
    sandboxed: bool,
    forced_ask: bool,
) {
    for (path, value) in security_pins(sandboxed, forced_ask) {
        set_path(settings, path, value);
    }
}

/// Sets `value` at `path` in `map`, making each object on the way.
fn set_path(map: &mut Map<String, Value>, path: &[&str], value: Value) {
    match path {
        [] => {}
        [key] => {
            map.insert((*key).to_owned(), value);
        }
        [key, rest @ ..] => {
            let mut inner = take_object(map, key);
            set_path(&mut inner, rest, value);
            map.insert((*key).to_owned(), Value::Object(inner));
        }
    }
}

#[cfg(test)]
#[path = "pinned_tests.rs"]
mod tests;
