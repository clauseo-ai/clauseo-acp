//! Which policy setting weakens a Claude launch. Policy outranks Clauseo's flag settings (user,
//! then flag, then policy; Claude Code 2.1.293 `BSr`), so a policy's value for a key Clauseo
//! pins (`agent::loads::pinned`) wins over the pin, and its arrays join the launch's. A setting
//! found here refuses the launch, named with where it was found, so the firm's IT can act; a
//! value that only tightens the launch passes.
//!
//! Covered, in every policy document (admin files and drop-ins, macOS managed preferences,
//! HKLM, HKCU, the server-managed `remote-settings.json`):
//! - every pinned key but `disableAllHooks` (`disableBundledSkills` among them: a policy value
//!   outranks the flag pin, `S4`): a value that is neither the pin nor its tighter side
//!   ([`tight`]); for `permissions.defaultMode`, a mode not known to be as strict as the pin
//!   ([`STRICT_MODES`]). The hooks keys keep their own rule (`AdminPolicy::hooks_off`: a
//!   refusal when the launch rests on the guard), but in the user tier ([`weakening`]'s
//!   `hooks_too`), which has always refused them.
//! - `sandbox.excludedCommands` that is not empty: its commands run outside the sandbox.
//! - `skipDangerousModePermissionPrompt` set to anything but `false`.
//! - `allowManagedPermissionRulesOnly` set to anything but `false`.
//! - `env` naming one of [`ENV_REFUSED`], with any value and in any ASCII case: the shell
//!   Claude runs commands in on Windows, a claude.ai Project's id, and the switches of the
//!   `SendUserMessage` tool. A policy's `env` goes over the launch's environment when Claude Code
//!   starts (2.1.293, read from its source, not run live). The view names no setting for these
//!   ([`policy_setting`]); the unnamed words stand.
//! - Clauseo's own launch values ([`launch_pins`]): in `env`, a value other than the launch's
//!   for [`ENV_PINNED`] (fixed values) or [`ENV_OWN`] (`PATH` and `GIT_ATTR_SOURCE`, the
//!   launch's own values, [`OwnEnv`]), an empty one or `null` included (a policy cannot unset a
//!   variable, only empty it), in any ASCII case; and a flag setting of [`launch_settings`] set
//!   to another value. Claude Code applies a policy's `env` last, over the flag settings' and
//!   the process's. `PATH` is named to the app (`env.PATH`); the other variables are not.
//! - `tools` set to anything: it would replace the launch's tool list. `disallowedTools` is
//!   left: a policy's entries only add denies.
//!
//! The sandbox's keys count only for a launch with Claude's sandbox, or under a document that
//! turns it on: without one (Windows) they change nothing.
//!
//! Knowingly left: the arrays a firm adds (`permissions.allow`, `additionalDirectories`,
//! `sandbox.filesystem.allowWrite`, `sandbox.network.allowedDomains`, `allowUnixSockets`,
//! `allowMachLookup`), its own `hooks` and the rest of its `env`, and the single values
//! Clauseo does not pin (`apiKeyHelper`, `statusLine`, `otelHeadersHelper`, `awsAuthRefresh`,
//! `sandbox.network.httpProxyPort`, `socksProxyPort`, `strictAllowlist`): none outranks a pin,
//! and each is the firm's own choice in a place only IT can write.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use clauseo_protocol::PolicySetting;
use serde_json::{Map, Value};

use crate::agent::loads::pinned::Pin;

/// A setting that weakens a launch: its key's path and where it was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Weakening {
    /// The key's path, outermost first.
    pub(crate) key: &'static [&'static str],
    /// The policy document it is in (`managed-settings.json`, `HKCU\…`, …).
    pub(crate) source: &'static str,
}

impl Weakening {
    /// The key, dotted: `sandbox.enabled`.
    pub(crate) fn dotted(&self) -> String {
        self.key.join(".")
    }
}

/// The setting the app names for `key`, a key [`weakening`] can return: a fixed match, so no
/// text from a policy reaches the view. `None` for a key not on the list, [`ENV_REFUSED`]'s
/// included: the view names no setting for them, and the unnamed words stand.
pub(crate) fn policy_setting(key: &[&str]) -> Option<PolicySetting> {
    Some(match key {
        ["disableAllHooks"] => PolicySetting::DisableAllHooks,
        ["allowManagedHooksOnly"] => PolicySetting::AllowManagedHooksOnly,
        ["allowManagedPermissionRulesOnly"] => PolicySetting::AllowManagedPermissionRulesOnly,
        ["skipDangerousModePermissionPrompt"] => PolicySetting::SkipDangerousModePermissionPrompt,
        ["disableBundledSkills"] => PolicySetting::DisableBundledSkills,
        ["permissions", "defaultMode"] => PolicySetting::PermissionsDefaultMode,
        ["permissions", "disableBypassPermissionsMode"] => {
            PolicySetting::PermissionsDisableBypassPermissionsMode
        }
        ["sandbox", "enabled"] => PolicySetting::SandboxEnabled,
        ["sandbox", "autoAllowBashIfSandboxed"] => PolicySetting::SandboxAutoAllowBashIfSandboxed,
        ["sandbox", "allowUnsandboxedCommands"] => PolicySetting::SandboxAllowUnsandboxedCommands,
        ["sandbox", "failIfUnavailable"] => PolicySetting::SandboxFailIfUnavailable,
        ["sandbox", "allowAppleEvents"] => PolicySetting::SandboxAllowAppleEvents,
        ["sandbox", "enableWeakerNestedSandbox"] => PolicySetting::SandboxEnableWeakerNestedSandbox,
        ["sandbox", "enableWeakerNetworkIsolation"] => {
            PolicySetting::SandboxEnableWeakerNetworkIsolation
        }
        ["sandbox", "network", "allowLocalBinding"] => {
            PolicySetting::SandboxNetworkAllowLocalBinding
        }
        ["sandbox", "network", "allowAllUnixSockets"] => {
            PolicySetting::SandboxNetworkAllowAllUnixSockets
        }
        ["sandbox", "excludedCommands"] => PolicySetting::SandboxExcludedCommands,
        ["includeGitInstructions"] => PolicySetting::IncludeGitInstructions,
        ["crossSessionInbound"] => PolicySetting::CrossSessionInbound,
        ["disableRemoteControl"] => PolicySetting::DisableRemoteControl,
        ["tools"] => PolicySetting::Tools,
        ["env", "PATH"] => PolicySetting::EnvPath,
        _ => return None,
    })
}

/// The modes as strict as each pinned one, or stricter: `default` asks, `plan` only reads.
/// Any other mode, `dontAsk` included (the guard counts it as unasked, `pre_write_guard.rs`),
/// is weaker or not known to be stricter.
pub(crate) const STRICT_MODES: [&str; 2] = ["default", "plan"];

/// Clauseo's launch variables with a fixed value, which a policy's `env` may not change. They
/// must match launch-tool-list's `agent::loads::CLAUDE_LAUNCH_VARS` (scheduler off, no
/// repository status, no messaging between sessions, no marketplace download) until it merges;
/// then this list takes them from there.
pub(crate) const ENV_PINNED: [(&[&str], &str); 4] = [
    (&["env", "CLAUDE_CODE_DISABLE_CRON"], "1"),
    (&["env", "CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS"], "1"),
    (&["env", "CLAUDE_CODE_HARBOR_KITE"], "0"),
    (
        &[
            "env",
            "CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL",
        ],
        "1",
    ),
];

/// Launch variables whose value is the launch's own ([`OwnEnv`]): `PATH`, and launch-no-git's
/// `GIT_ATTR_SOURCE` (the name must match that branch's until it merges).
pub(crate) const ENV_OWN: [&[&str]; 2] = [&["env", "PATH"], &["env", "GIT_ATTR_SOURCE"]];

/// The flag settings every Claude launch sets, with Clauseo's values. They must match
/// launch-tool-list's `agent::loads::claude_flag_settings` until it merges.
pub(crate) fn launch_settings() -> [Pin; 3] {
    [
        (&["includeGitInstructions"], Value::Bool(false)),
        (&["crossSessionInbound"], Value::from("refuse")),
        (&["disableRemoteControl"], Value::Bool(true)),
    ]
}

/// The settings a policy may not hold at all: `tools` replaces the launch's tool list.
pub(crate) const ANY_VALUE: [&[&str]; 1] = [&["tools"]];

/// The launch's own values of [`ENV_OWN`]'s variables, from the environment it gave the agent
/// (`None` for one it does not set): what a policy's `env` must leave as it is. Kept on the
/// agent for the checks before each prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OwnEnv([Option<String>; 2]);

impl OwnEnv {
    /// The values in `env`, the agent's environment: a name in any ASCII case on Windows,
    /// where `Path` is `PATH`.
    pub(crate) fn of(env: &BTreeMap<OsString, OsString>) -> Self {
        Self(ENV_OWN.map(|key| {
            env.iter()
                .find(|(name, _)| key.last().is_some_and(|wanted| same_name(name, wanted)))
                .map(|(_, value)| value.to_string_lossy().into_owned())
        }))
    }
}

/// Whether the variable `name` is `wanted`, as this system compares names.
fn same_name(name: &OsStr, wanted: &str) -> bool {
    if cfg!(windows) {
        name.eq_ignore_ascii_case(wanted)
    } else {
        name == wanted
    }
}

/// The pins of Clauseo's own launch values, beside `pinned::security_pins`: [`ENV_PINNED`],
/// [`launch_settings`] and, once the launch's environment is known (`own`), [`ENV_OWN`]'s.
pub(crate) fn launch_pins(own: Option<&OwnEnv>) -> Vec<Pin> {
    let fixed = ENV_PINNED
        .into_iter()
        .map(|(key, value)| (key, Value::from(value)));
    let own = own.into_iter().flat_map(|own| {
        let values = own
            .0
            .clone()
            .map(|value| value.map_or(Value::Null, Value::from));
        ENV_OWN.into_iter().zip(values)
    });
    fixed.chain(launch_settings()).chain(own).collect()
}

/// The variables a policy's `env` may not set, to any value: which shell Claude's commands run
/// in on Windows; `CLAUDE_PROJECT_UUID`, a claude.ai Project's id (2.1.293's project context
/// block); and the two that each offer its `SendUserMessage` (`Brief`) tool. Clauseo sets none
/// of them, and an inherited one is scrubbed (`agent::spec`).
pub(crate) const ENV_REFUSED: [&[&str]; 5] = [
    &["env", "CLAUDE_CODE_USE_POWERSHELL_TOOL"],
    &["env", "CLAUDE_CODE_GIT_BASH_PATH"],
    &["env", "CLAUDE_PROJECT_UUID"],
    &["env", "CLAUDE_CODE_PEWTER_OWL_TOOL"],
    &["env", "CLAUDE_CODE_BRIEF"],
];

/// The hooks keys that, set to `true`, drop every hook but the policy's own.
const HOOKS_OFF: [&[&str]; 2] = [&["disableAllHooks"], &["allowManagedHooksOnly"]];

/// The first setting of `settings`, a policy document from `source`, that weakens a launch
/// whose pins are `pins` (`pinned::security_pins`) and that carries Claude's sandbox or not
/// (`sandboxed`); with `hooks_too`, the hooks keys set to `true` count as well.
pub(crate) fn weakening(
    settings: &Map<String, Value>,
    source: &'static str,
    pins: &[Pin],
    sandboxed: bool,
    hooks_too: bool,
) -> Option<Weakening> {
    let sandbox_on = sandboxed || at(settings, &["sandbox", "enabled"]) == Some(&Value::Bool(true));
    let counts = |key: &[&str]| sandbox_on || key.first() != Some(&"sandbox");
    let pinned = pins.iter().filter(|(key, _)| *key != ["disableAllHooks"]);
    let found = pinned
        .filter(|(key, _)| counts(key))
        .find(|(key, pin)| pin_weakened(settings, key, pin))
        .map(|(key, _)| *key)
        .or_else(|| {
            let excluded: &'static [&'static str] = &["sandbox", "excludedCommands"];
            let listed = at(settings, excluded).is_some_and(|value| !empty(value));
            (counts(excluded) && listed).then_some(excluded)
        })
        .or_else(|| {
            let flags: [&'static [&'static str]; 2] = [
                &["skipDangerousModePermissionPrompt"],
                &["allowManagedPermissionRulesOnly"],
            ];
            flags
                .into_iter()
                .find(|key| at(settings, key).is_some_and(|value| *value != Value::Bool(false)))
        })
        .or_else(|| {
            let set = |key: &&[&str]| {
                key.last()
                    .is_some_and(|name| env_values(settings, name).next().is_some())
            };
            ENV_REFUSED.into_iter().find(set)
        })
        .or_else(|| {
            ANY_VALUE
                .into_iter()
                .find(|key| at(settings, key).is_some())
        })
        .or_else(|| {
            let hooks = HOOKS_OFF.into_iter().filter(|_| hooks_too);
            hooks
                .into_iter()
                .find(|key| at(settings, key) == Some(&Value::Bool(true)))
        });
    found.map(|key| Weakening { key, source })
}

/// Whether `settings` holds a value at the pinned `key` that weakens `pin`. In `env`: an entry
/// of that name in any ASCII case that is not the pin, an empty one or `null` included, or any
/// entry at all for a variable the launch does not set (a `null` pin).
fn pin_weakened(settings: &Map<String, Value>, key: &[&str], pin: &Value) -> bool {
    if let ["env", name] = key {
        return env_values(settings, name).any(|value| pin.is_null() || value != pin);
    }
    at(settings, key).is_some_and(|value| weakens(key, pin, value))
}

/// The values a policy's `env` gives the variable `name`, in any ASCII case.
fn env_values<'a>(
    settings: &'a Map<String, Value>,
    name: &'a str,
) -> impl Iterator<Item = &'a Value> + 'a {
    let env = settings.get("env").and_then(Value::as_object);
    env.into_iter()
        .flatten()
        .filter(move |(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

/// Whether a policy's `value` at the pinned `key` weakens the launch's `pin`.
fn weakens(key: &[&str], pin: &Value, value: &Value) -> bool {
    if value == pin {
        return false;
    }
    if key == ["permissions", "defaultMode"] {
        return !value
            .as_str()
            .is_some_and(|mode| STRICT_MODES.contains(&mode));
    }
    tight(key).is_none_or(|tight| *value != tight)
}

/// The tighter side of a pinned key whose pin depends on the launch, or that has a safe value:
/// a sandbox that is on and fails closed, everything else in the sandbox off, bypass disabled.
/// `None` for a key with no tighter side, where any other value weakens.
fn tight(key: &[&str]) -> Option<Value> {
    match key {
        ["sandbox", "enabled" | "failIfUnavailable"] => Some(Value::Bool(true)),
        ["sandbox", ..] => Some(Value::Bool(false)),
        ["permissions", "disableBypassPermissionsMode"] => Some(Value::from("disable")),
        _ => None,
    }
}

/// The value at `key` in `settings`, through nested objects.
fn at<'a>(settings: &'a Map<String, Value>, key: &[&str]) -> Option<&'a Value> {
    let (last, outer) = key.split_last()?;
    let mut map = settings;
    for part in outer {
        map = map.get(*part)?.as_object()?;
    }
    map.get(*last)
}

/// Whether a list setting's `value` adds nothing: `null`, or an empty array or object.
fn empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(items) => items.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
#[path = "weakens_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "weakens_launch_tests.rs"]
mod launch_tests;
