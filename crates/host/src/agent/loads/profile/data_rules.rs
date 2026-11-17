//! The data folder's and the other matters' rules in a Claude launch's flag settings, and the
//! size past which their Windows shell rules shed the variable forms. Split from `profile.rs`
//! for its size.

use std::ffi::OsString;

use serde_json::{Map, Value};

use super::{push_new, take_array, take_object};

/// The size past which a launch's settings shed the data folder's variable forms from its shell
/// rules (`data_fence::windows_shell_deny_rules_without_variables`): about 1.8 MiB, under the
/// 2 MiB settings file Claude Code reads (`agent::launch_settings`), which refuses a larger one.
pub(crate) const SHED_AT: usize = 1_887_436;

/// `permissions.deny` gains the data folder's rules (`agent/data_fence.rs`; on Windows also its
/// `Bash` and `PowerShell` rules, `data_fence::windows_shell_deny_rules`, without the variable
/// forms when all of them would take the settings past [`SHED_AT`], counting the allow rules;
/// the other matters get none, their `Read` and `Edit` denies being what Claude Code checks a
/// command's file paths against, `data_fence::claude_deny_rules`), and `permissions.allow` the
/// Read of the built-in skills' folder and the own folders' listing and reading
/// (`data_fence::claude_allow_rules`), each after any the hook put there. The own folders' glob
/// denies go in only without Claude's shell sandbox (`sandboxed`, `keys.sandbox`: on Windows,
/// or with the sandbox `Off`). A hook value at `permissions`, `deny` or `allow` of another
/// type is replaced. With no spelling (a unit test's keys) nothing changes.
pub(super) fn deny_data_folder(
    settings: &mut Map<String, Value>,
    data: &crate::agent::data_fence::LaunchDeny,
    sandboxed: bool,
) {
    if data.spellings.is_empty() {
        return;
    }
    let mut permissions = take_object(settings, "permissions");
    let mut deny = take_array(&mut permissions, "deny");
    for rule in crate::agent::data_fence::claude_deny_rules(data, sandboxed) {
        push_new(&mut deny, &rule);
    }
    let mut allow = take_array(&mut permissions, "allow");
    for rule in crate::agent::data_fence::claude_allow_rules(data) {
        push_new(&mut allow, &rule);
    }
    if cfg!(windows) {
        let var = |name: &str| std::env::var_os(name);
        let spent = json_len(&*settings) + json_len(&deny) + json_len(&allow);
        let shell = shell_rules_within(data, &var, spent, SHED_AT);
        // A set, not `push_new`: a busy data folder gives a few thousand of these.
        let mut known: std::collections::HashSet<String> = deny
            .iter()
            .filter_map(|rule| rule.as_str().map(str::to_owned))
            .collect();
        for rule in shell {
            if known.insert(rule.clone()) {
                deny.push(Value::String(rule));
            }
        }
    }
    permissions.insert("deny".to_owned(), Value::Array(deny));
    permissions.insert("allow".to_owned(), Value::Array(allow));
    settings.insert("permissions".to_owned(), Value::Object(permissions));
}

/// The data folder's Windows shell rules for a launch whose other settings take `spent` bytes as
/// JSON: all of them, or without the variable forms
/// (`data_fence::windows_shell_deny_rules_without_variables`) when all of them would take the
/// settings past `budget` ([`SHED_AT`]), logged with fixed fields only.
pub(crate) fn shell_rules_within(
    data: &crate::agent::data_fence::LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
    spent: usize,
    budget: usize,
) -> Vec<String> {
    let shell = crate::agent::data_fence::windows_shell_deny_rules(data, var);
    let total = spent + json_len(&shell);
    if total <= budget {
        return shell;
    }
    tracing::warn!(
        settings_bytes = total,
        budget,
        shed = "vars",
        "Claude's settings outgrew their budget; the data folder's variable forms are shed"
    );
    crate::agent::data_fence::windows_shell_deny_rules_without_variables(data, var)
}

/// `value`'s length as JSON (0 if it can't be written, which a map of JSON values never is).
fn json_len(value: &impl serde::Serialize) -> usize {
    serde_json::to_string(value).map_or(0, |text| text.len())
}
