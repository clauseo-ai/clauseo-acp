//! Forced ask: how a Claude agent runs in a matter whose folder Codex's sandbox can't work in
//! (`agent/folder_support.rs` answered `network` or `no_permissions` at this launch), or whose
//! check failed (an unknown folder fails closed: #370's security review). Nothing fences such a
//! folder, so Claude asks the lawyer before every change: Rohan, 2026-11-04 (private
//! `folder-rights.md`, decision 3; decision record 0030). Four layers, each enforced by
//! the host:
//!
//! - modes: only [`ASK_MODES`] may be set (`SetMode` refuses any other with `command.invalid`),
//!   and a mode the agent moves to itself is switched back to `default`;
//! - options: a permission request loses every option that would stop later asks
//!   ([`keep_option`]), so no "always" rule reaches the adapter;
//! - no `allowedTools` in the session's `_meta` (`agent/loads.rs`), and [`ASK_RULES`] as
//!   `permissions.ask` rules in its flag settings, the fence that holds when the hook fails;
//! - the guard hook runs before every tool with [`ASK_ALL_FLAG`] and answers `ask` for all but a
//!   fixed set of local reads (`pre_write_guard/ask.rs`), the fence a mode switch cannot race.
//!   A forced-ask Claude launch with no guard hook to give is refused (`agent.launch_failed`).
//!
//! A `PermissionRequest` hook already in the lawyer's own settings or plugins can still approve
//! here (decision record 0030 has why that is accepted).
//!
//! The decision is made per launch from that launch's own folder check, never from the row's
//! remembered answer; `session/load` comes with a launch, so it is checked again.

use clauseo_protocol::{FolderSupportKind, PermissionKind, PermissionOption};

use crate::agent::folder_support::Answer;

/// Whether a Claude launch whose folder check gave `answer` runs in forced ask: on a network
/// drive, on a drive without permissions, and when the check failed (unknown fails closed).
pub(crate) fn applies(answer: Answer) -> bool {
    matches!(
        answer,
        Answer::Failed
            | Answer::Known(Some(
                FolderSupportKind::Network | FolderSupportKind::NoPermissions
            ))
    )
}

/// The Claude Code permission modes that never approve a change by themselves: `default` asks
/// before each change, `plan` makes none.
pub(crate) const ASK_MODES: [&str; 2] = ["default", "plan"];

/// The ids of the plan exit's (`ExitPlanMode`) options that switch the session to a mode that
/// approves changes without asking: claude-agent-acp 0.89.1, `permissions/options/shared.js`
/// `PERMISSION_OPTION_ID`, offered by `permissions/options/tools.js`
/// `buildExitPlanModePermissionOptions`. Every one of them is `allow_always` there, so the
/// `allow_always` filter drops them already; the list is the second check. The options that stay
/// are `exit-plan-default` ("Yes, manually approve edits", `allow_once`) and `reject` ("No,
/// keep planning", `reject_once`).
const SKIPPING_OPTIONS: [&str; 6] = [
    "exit-plan-auto",
    "exit-plan-bypass",
    "exit-plan-accept-edits",
    "exit-plan-clear-auto",
    "exit-plan-clear-bypass",
    "exit-plan-clear-accept-edits",
];

/// Whether a forced-ask session may be set to mode `id`: only [`ASK_MODES`], by exact id.
pub(crate) fn mode_allowed(id: &str) -> bool {
    ASK_MODES.contains(&id)
}

/// Whether a forced-ask session's permission request keeps `option`: not an `allow_always`
/// one, and not one of [`SKIPPING_OPTIONS`].
pub(crate) fn keep_option(option: &PermissionOption) -> bool {
    let id = option.option_id.as_str();
    option.kind != PermissionKind::AllowAlways
        && !SKIPPING_OPTIONS
            .iter()
            .any(|skipping| skipping.eq_ignore_ascii_case(id))
}

/// claude-agent-acp's config option for the permission mode (`MODE_CONFIG_ID`, 0.89.1
/// `acp-agent.js`): `session/set_config_option` with it sets the mode as `session/set_mode`
/// does, so forced ask gates it the same way ([`config_allowed`]).
pub(crate) const MODE_CONFIG_OPTION: &str = "mode";

/// Whether a forced-ask session may set config option `option` to `value`: any option but
/// [`MODE_CONFIG_OPTION`], and that one only to one of [`ASK_MODES`]. A mode value that is no
/// id (`None`) is refused.
pub(crate) fn config_allowed(option: &str, value: Option<&str>) -> bool {
    option != MODE_CONFIG_OPTION || value.is_some_and(mode_allowed)
}

/// The `permissions.ask` rules a forced-ask session's flag settings (`options.settings`, the
/// `--settings` tier) carry: every tool the guard's `--ask-all` asks for that changes something
/// or reaches out. They hold when the guard hook times out, crashes or is blocked, where Claude
/// Code would go on to its normal permission check and the user's own `allow` rules would
/// approve unasked.
///
/// What Claude Code 2.1.293 honours (read from its binary, as the review read `T0o`): its rule
/// check (`KA`) takes a matching deny rule first, then a matching ask rule (`Tc`), and only then
/// the tool's own check and allow rules; the rules of every source (user, flag, …) are merged
/// before that (`RMe`), so a flag-settings ask beats a user-settings allow. A bare tool name
/// matches every call of it. Tool names match with globs (`IH` with `globMatching`, `Ere`), so
/// `mcp__*` matches every MCP server's tools, the form the CLI's own messages recommend "for
/// every MCP server's tools"; no per-server list is needed. `MultiEdit` is a legacy alias at
/// 2.1.293 (a warning at most), kept as the guard keeps it. One exception in `KA`: a whole-tool
/// `Bash` ask rule is skipped when the user's settings sandbox Bash with
/// `autoAllowBashIfSandboxed`, so the flag settings also set that to `false`
/// ([`NO_SANDBOX_AUTO_ALLOW`]).
pub(crate) const ASK_RULES: [&str; 9] = [
    "Bash",
    "PowerShell",
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
    "WebFetch",
    "WebSearch",
    "mcp__*",
];

/// The `sandbox` key a forced-ask session's flag settings set to `false` (see [`ASK_RULES`]).
pub(crate) const NO_SANDBOX_AUTO_ALLOW: &str = "autoAllowBashIfSandboxed";

/// The argument after the guard's `--matter <folder>` that makes it answer `ask` for every tool
/// but the local reads. An argument, not a variable: a variable would reach every child of the
/// agent's shell.
pub(crate) const ASK_ALL_FLAG: &str = "--ask-all";

/// The hook matcher for every tool, MCP tools included.
pub(crate) const EVERY_TOOL: &str = "*";

#[cfg(test)]
mod tests;
