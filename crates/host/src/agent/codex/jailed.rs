//! A jailed Codex's modes (R17 PR-D2c, the orchestrator's rulings B and 1). In the jail Codex
//! always runs codex-acp 2.2.2's [`JAILED_MODE`] preset (sandbox `dangerFullAccess`, approval
//! `never`): codex-acp sends its preset's approval policy with every turn
//! (`dist/index.js:35598-35615`), so it never asks and nests no sandbox of its own. Its
//! `ReadOnly` preset would do both (approval `on-request`, its own `readOnly` sandbox,
//! `:34263-34274`), so it is never run. A jailed Codex's two modes, [`MODES`], are the host's
//! policies, under the same ids the app sees:
//!
//! - [`READ_ONLY`]: the jail's read-only variant (`Policy::matter_read_only`): the matter is
//!   read, never written; the state, temp and notes folders keep their writes.
//! - [`JAILED_MODE`] ("Works on its own in the Safe workspace"): the normal jailed policy.
//!
//! A launch's policy never widens in place, so a switch between them relaunches the agent at
//! the turn boundary, its session resumed (`actor/turn/forced_mode.rs`).
//!
//! - Only [`MODES`] are offered: the session's modes and its mode config option lose every
//!   other value ([`keep_modes`], [`keep_options`]).
//! - `SetMode`, or `SetConfigOption` on the mode option, naming another mode is refused with
//!   `sandbox.ask_mode_unavailable` ([`check`]).
//! - A thread whose saved mode is any other (an asking mode, from before the jail), or cannot
//!   be read, launches read-only, never in the normal policy ([`start_mode`]).
//!
//! Tier C (no jail) is unchanged: [`super::ASK_MODE`], every mode offered.

use clauseo_acp::SessionModes;
use clauseo_protocol::{ConfigCategory, ConfigOption, ConfigOptionKind, ErrorCode};

use super::JAILED_MODE;
use crate::error::Error;

/// The read-only mode's id, codex-acp 2.2.2's read-only preset's (`src/AgentMode.ts`,
/// `dist/index.js:34243-34354`): here the jail's read-only policy, never that preset.
pub(crate) const READ_ONLY: &str = "read-only";

/// Every mode a jailed Codex has, in the order it shows them.
pub(crate) const MODES: [&str; 2] = [READ_ONLY, JAILED_MODE];

/// The id of codex-acp's config option for the mode, as ACP names it; an option of the mode
/// category counts too, whatever its id.
pub(crate) const MODE_OPTION: &str = "mode";

/// Whether a jailed Codex may be in mode `id`: only [`MODES`], by exact id.
pub(crate) fn allowed(id: &str) -> bool {
    MODES.contains(&id)
}

/// The mode, so the policy, of a jailed Codex launch: `asked` (the lawyer's switch it carries)
/// when one of [`MODES`], else [`start_mode`] of the thread's saved mode.
pub(crate) fn launch_policy(
    asked: Option<&str>,
    saved: Option<&str>,
    had_session: bool,
) -> &'static str {
    match asked.and_then(|asked| MODES.into_iter().find(|mode| *mode == asked)) {
        Some(mode) => mode,
        None => start_mode(saved, had_session),
    }
}

/// The mode a jailed Codex session starts in, from the thread's saved mode (its header's last
/// mode), `None` when none can be read: a saved mode of [`MODES`] stays; any other starts
/// [`READ_ONLY`]; with none saved, a thread that never had a session starts in the preset and
/// one that did starts [`READ_ONLY`], since what it was in cannot be told. Never an asking mode,
/// and never the preset in place of an asking one.
pub(crate) fn start_mode(saved: Option<&str>, had_session: bool) -> &'static str {
    match saved {
        Some(saved) => MODES
            .into_iter()
            .find(|mode| *mode == saved)
            .unwrap_or(READ_ONLY),
        None if had_session => READ_ONLY,
        None => JAILED_MODE,
    }
}

/// A jailed Codex's mode `id`, refused with `sandbox.ask_mode_unavailable` unless [`allowed`].
pub(crate) fn check(id: &str) -> Result<(), Error> {
    if allowed(id) {
        return Ok(());
    }
    tracing::info!(mode = id, "a jailed Codex was refused a mode that asks");
    Err(Error::refused(
        ErrorCode::SandboxAskModeUnavailable,
        format!(
            "Codex in the sandbox can't ask before changes, so it takes only \"{READ_ONLY}\" or \
             \"{JAILED_MODE}\", not \"{id}\""
        ),
    ))
}

/// A jailed Codex launch's start mode `mode` (`INITIAL_AGENT_MODE`, `codex::launch_mode`),
/// refused with `sandbox.ask_mode_unavailable` unless it is the jailed preset: no launcher is
/// made for a jailed Codex in any other mode (`launch_checks/sandbox/policy.rs`).
pub(crate) fn check_launch(mode: &str) -> Result<(), Error> {
    if mode == JAILED_MODE {
        return Ok(());
    }
    tracing::error!(
        mode,
        "a jailed Codex launch would not start in the jailed preset"
    );
    Err(Error::refused(
        ErrorCode::SandboxAskModeUnavailable,
        "The sandbox couldn't start Codex: it would not run in the sandbox's mode.",
    ))
}

/// Whether config option `id` sets the mode: [`MODE_OPTION`], or an option of the mode category
/// in `known` (the thread's options as logged).
pub(crate) fn is_mode_option(id: &str, known: &[ConfigOption]) -> bool {
    id == MODE_OPTION
        || known
            .iter()
            .any(|option| option.id == id && option.category == ConfigCategory::Mode)
}

/// The session's modes, but only [`MODES`].
pub(crate) fn keep_modes(modes: &mut SessionModes) {
    modes.available.retain(|mode| allowed(&mode.id));
}

/// Every mode option of `options` (see [`is_mode_option`]) keeps only [`MODES`]' values, its
/// current value set to `current` when given (the mode the session was switched to).
pub(crate) fn keep_options(options: &mut [ConfigOption], current: Option<&str>) {
    for option in options
        .iter_mut()
        .filter(|option| option.id == MODE_OPTION || option.category == ConfigCategory::Mode)
    {
        if let ConfigOptionKind::Select {
            current_value,
            options: choices,
        } = &mut option.kind
        {
            choices.retain(|choice| allowed(&choice.value));
            if let Some(current) = current {
                *current_value = current.into();
            }
        }
    }
}

#[cfg(test)]
mod tests;
