//! Windows: Claude gets one shell tool, PowerShell, on every machine, Git or not (the
//! orchestrator's ruling of 2026-11-16; decision record 0030, "Claude's permission mode").
//!
//! Claude Code 2.1.293 decides its shell tools at start-up, read from its source:
//! - `bS()`: on Windows `CLAUDE_CODE_USE_POWERSHELL_TOOL` set to `1`, `true`, `yes` or `on` turns
//!   the `PowerShell` tool on and `0`, `false`, `no` or `off` turns it off; unset, it is on where
//!   no Git Bash is found and, where one is, follows a remote flag. Setting it makes the shell
//!   the same whether or not Git is installed.
//! - `sBr`: any `Bash` deny rule turns the `PowerShell` tool off too, but only where Git Bash
//!   exists, and not when the variable is on or a settings rule names `PowerShell`. So `"Bash"`
//!   denied with the variable at `1` leaves PowerShell on, and the Bash tool off, on any machine.
//! - `BG()` picks the PowerShell: `pwsh` on `PATH`, then `%ProgramFiles%\PowerShell\7`, then
//!   `WindowsApps` and `.dotnet\tools`, then `powershell` on `PATH`, then System32's 5.1. No
//!   variable moves it, so the shell is Windows PowerShell 5.1, or PowerShell 7 if installed.
//!
//! What the launch does, on Windows only:
//! - [`apply_env`] edits a Claude launch's merged environment: every inherited, settings-entry
//!   or hook value of [`POWERSHELL_TOOL_VAR`] or [`GIT_BASH_VAR`] (any case) is dropped and
//!   [`POWERSHELL_TOOL_VAR`] is set to [`ON`], so only Clauseo's own value counts.
//! - [`meta_env`] sets the same value in `_meta`'s `options.env` and the flag settings' `env`
//!   (`agent::loads::session_meta`): Claude Code applies a settings `env` over its process env
//!   before the shell check, so the value holds over the hook's `_meta`. The settings files a
//!   launch reads are checked by the launch check (#1167).
//! - [`deny_bash`] puts [`BASH_RULE`] in the flag settings' `permissions.deny`
//!   (`agent::loads::profile::claude_options`): Claude Code merges every source's deny rules and
//!   a deny beats every allow, so a machine with Git gets no Bash tool either.
//!
//! Git Bash is found at fixed Git paths, then through `where.exe git`, never from a `bash.exe` on
//! `PATH`. Clauseo's one hook, the pre-write guard, is exec form (`command` and `args`, spawned
//! with no shell), so it runs the same under either shell. macOS and Linux are left as they were.

use std::collections::BTreeMap;
use std::ffi::OsString;

use serde_json::{Map, Value};

use super::shell_path::Os;

/// The variable that turns Claude Code's `PowerShell` tool on or off on Windows.
pub(crate) const POWERSHELL_TOOL_VAR: &str = "CLAUDE_CODE_USE_POWERSHELL_TOOL";

/// The value Clauseo sets [`POWERSHELL_TOOL_VAR`] to.
pub(crate) const ON: &str = "1";

/// The variable that points Claude Code at a Git Bash; no Windows launch keeps one.
pub(crate) const GIT_BASH_VAR: &str = "CLAUDE_CODE_GIT_BASH_PATH";

/// The deny rule that takes the whole `Bash` tool away on Windows.
pub(crate) const BASH_RULE: &str = "Bash";

/// Whether `name` is [`POWERSHELL_TOOL_VAR`] or [`GIT_BASH_VAR`], in any case.
fn is_shell_var(name: &str) -> bool {
    name.eq_ignore_ascii_case(POWERSHELL_TOOL_VAR) || name.eq_ignore_ascii_case(GIT_BASH_VAR)
}

/// Edits a Claude launch's merged `env` on `os`: on Windows only, drops every
/// [`POWERSHELL_TOOL_VAR`] and [`GIT_BASH_VAR`] (any case) and sets [`POWERSHELL_TOOL_VAR`] to
/// [`ON`].
pub(crate) fn apply_env(env: &mut BTreeMap<OsString, OsString>, os: Os) {
    if os != Os::Windows {
        return;
    }
    env.retain(|name, _| !name.to_str().is_some_and(is_shell_var));
    env.insert(OsString::from(POWERSHELL_TOOL_VAR), OsString::from(ON));
}

/// On Windows (`windows`), sets [`POWERSHELL_TOOL_VAR`] to [`ON`] in `env` (a `_meta` or flag
/// settings `env` object), over any value of that name in any case, and drops [`GIT_BASH_VAR`].
pub(crate) fn meta_env(env: &mut Map<String, Value>, windows: bool) {
    if !windows {
        return;
    }
    env.retain(|name, _| !is_shell_var(name));
    env.insert(POWERSHELL_TOOL_VAR.to_owned(), Value::from(ON));
}

/// On Windows (`windows`), `permissions.deny` of the flag `settings` gains [`BASH_RULE`], after
/// any rules already there. A value at `permissions` or `deny` of another type is replaced.
pub(crate) fn deny_bash(settings: &mut Map<String, Value>, windows: bool) {
    if !windows {
        return;
    }
    let mut permissions = super::loads::take_object(settings, "permissions");
    let mut deny = match permissions.remove("deny") {
        Some(Value::Array(rules)) => rules,
        _ => Vec::new(),
    };
    if !deny.iter().any(|rule| rule == BASH_RULE) {
        deny.push(Value::from(BASH_RULE));
    }
    permissions.insert("deny".to_owned(), Value::Array(deny));
    settings.insert("permissions".to_owned(), Value::Object(permissions));
}

#[cfg(test)]
mod tests;
