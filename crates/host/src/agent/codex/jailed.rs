//! A jailed Codex's modes (R17 PR-D2c, the orchestrator's rulings B and 1). In the jail Codex
//! always runs codex-acp 2.2.2's [`JAILED_MODE`] preset (sandbox `dangerFullAccess`, approval
//! `never`): codex-acp sends its preset's approval policy with every turn
//! (`dist/index.js:35598-35615`), so it never asks and nests no sandbox of its own. Its
//! `ReadOnly` preset would do both (approval `on-request`, its own `readOnly` sandbox,
//! `:34263-34274`), so it is never run. A jailed Codex's two modes, [`MODES`], are the host's
//! policies, under the same ids the app sees:
//!
//! - [`READ_ONLY`]: the jail's read-only variant (`Policy::matter_read_only`): the matter is
//!   read, never written; the state, temp and notes folders keep their writes. Not offered on
//!   Windows until stage C's PR6 enforces it there ([`READ_ONLY_ENFORCED`]).
//! - [`JAILED_MODE`] ("Works on its own in the Safe workspace"): the normal jailed policy.
//!
//! A launch's policy never widens in place, so a switch between them relaunches the agent at
//! the turn boundary, its session resumed (`actor/turn/forced_mode.rs`).
//!
//! - Only [`MODES`] are offered: the session's modes and its mode config option lose every
//!   other value ([`keep_modes`], [`keep_options`]).
//! - `SetMode`, or `SetConfigOption` on the mode option, naming another mode is refused with
//!   `sandbox.ask_mode_unavailable` ([`check`]).
//! - A thread whose saved mode is any other (an asking mode, from before the jail) launches in
//!   [`SAVED_ASK_FALLBACK`]; one with no saved mode in the jailed preset ([`start_mode`]).
//!
//! Tier C (no jail) is unchanged: [`super::ASK_MODE`], every mode offered.

use clauseo_acp::SessionModes;
use clauseo_protocol::{ConfigCategory, ConfigOption, ConfigOptionKind, ErrorCode};

use super::JAILED_MODE;
use crate::error::Error;

/// The read-only mode's id, codex-acp 2.2.2's read-only preset's (`src/AgentMode.ts`,
/// `dist/index.js:34243-34354`): here the jail's read-only policy, never that preset.
pub(crate) const READ_ONLY: &str = "read-only";

/// Whether the jail enforces [`READ_ONLY`] on this system, so it is offered: not on Windows
/// yet. TODO(PR6): stage C's PR6 enforces it on Windows; this becomes `true` then.
pub(crate) const READ_ONLY_ENFORCED: bool = !cfg!(windows);

/// Every mode a jailed Codex has on this system, in the order it shows them: [`READ_ONLY`] only
/// where the jail enforces it ([`READ_ONLY_ENFORCED`]).
pub(crate) const MODES: &[&str] = if READ_ONLY_ENFORCED {
    &[READ_ONLY, JAILED_MODE]
} else {
    &[JAILED_MODE]
};

/// What a thread whose saved mode is not one of [`MODES`] (an asking mode, from before the jail)
/// starts in. Rohan's ruling (2026-11-09): no Codex mode switch at launch, so the jailed
/// preset a new thread starts in, fenced by the jail; with a switch it would be [`READ_ONLY`].
pub(crate) const SAVED_ASK_FALLBACK: &str = JAILED_MODE;

/// The id of codex-acp's config option for the mode, as ACP names it; an option of the mode
/// category counts too, whatever its id.
pub(crate) const MODE_OPTION: &str = "mode";

/// Whether a jailed Codex may be in mode `id`: only [`MODES`], by exact id.
pub(crate) fn allowed(id: &str) -> bool {
    MODES.contains(&id)
}

/// The mode, so the policy, of a jailed Codex launch: `asked` (the lawyer's switch it carries)
/// when one of [`MODES`], else [`start_mode`] of the thread's saved mode.
pub(crate) fn launch_policy(asked: Option<&str>, saved: Option<&str>) -> &'static str {
    match asked.and_then(offered) {
        Some(mode) => mode,
        None => start_mode(saved),
    }
}

/// The mode a jailed Codex session starts in, from the thread's saved mode (its header's last
/// mode), `None` when none can be read: a saved mode of [`MODES`] stays; any other starts in
/// [`SAVED_ASK_FALLBACK`] (when offered here); with none saved, the jailed preset, as a new
/// thread starts. Never an asking mode.
pub(crate) fn start_mode(saved: Option<&str>) -> &'static str {
    match saved {
        Some(saved) => offered(saved)
            .or_else(|| offered(SAVED_ASK_FALLBACK))
            .unwrap_or(JAILED_MODE),
        None => JAILED_MODE,
    }
}

/// `id` as one of [`MODES`], when it is one.
fn offered(id: &str) -> Option<&'static str> {
    MODES.iter().copied().find(|mode| *mode == id)
}

/// A jailed Codex's mode `id`, refused unless [`allowed`]: [`READ_ONLY`] where the jail does
/// not enforce it ([`READ_ONLY_ENFORCED`], TODO(PR6)) with `command.invalid`, as a mode the
/// session does not offer; any other with `sandbox.ask_mode_unavailable`. The words name no
/// mode id.
pub(crate) fn check(id: &str) -> Result<(), Error> {
    if allowed(id) {
        return Ok(());
    }
    if id == READ_ONLY {
        tracing::info!(
            "a jailed Codex was refused Read-only, which the jail does not enforce here"
        );
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            "Read-only isn't available for ChatGPT in the Safe workspace on this computer yet, \
             so its mode stayed as it was.",
        ));
    }
    tracing::info!(mode = id, "a jailed Codex was refused a mode that asks");
    Err(Error::refused(
        ErrorCode::SandboxAskModeUnavailable,
        "ChatGPT can't ask before each change in the Safe workspace, so its mode stayed as it was.",
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
