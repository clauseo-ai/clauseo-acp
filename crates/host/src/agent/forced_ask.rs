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
//! - no `allowedTools` in the session's `_meta` (`agent/loads.rs`);
//! - the guard hook runs before every tool with [`ASK_ALL_FLAG`] and answers `ask` for all but a
//!   fixed set of local reads (`pre_write_guard/ask.rs`), the fence a mode switch cannot race.
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

/// The Claude Code permission modes that approve changes without asking, as claude-agent-acp
/// names them; a permission option whose id is one of them switches the session to it (the plan
/// exit's "Yes, and auto-accept edits" is `acceptEdits`).
const SKIPPING_MODES: [&str; 4] = ["acceptEdits", "bypassPermissions", "dontAsk", "auto"];

/// Whether a forced-ask session may be set to mode `id`: only [`ASK_MODES`], by exact id.
pub(crate) fn mode_allowed(id: &str) -> bool {
    ASK_MODES.contains(&id)
}

/// Whether a forced-ask session's permission request keeps `option`: not an `allow_always`
/// one, and not one whose id names a mode outside [`ASK_MODES`].
pub(crate) fn keep_option(option: &PermissionOption) -> bool {
    let id = option.option_id.as_str();
    option.kind != PermissionKind::AllowAlways
        && !SKIPPING_MODES
            .iter()
            .any(|mode| mode.eq_ignore_ascii_case(id))
}

/// The argument after the guard's `--matter <folder>` that makes it answer `ask` for every tool
/// but the local reads. An argument, not a variable: a variable would reach every child of the
/// agent's shell.
pub(crate) const ASK_ALL_FLAG: &str = "--ask-all";

/// The hook matcher for every tool, MCP tools included.
pub(crate) const EVERY_TOOL: &str = "*";

#[cfg(test)]
mod tests;
