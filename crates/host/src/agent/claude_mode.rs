//! The permission mode a Claude launch runs in (R17 PR-D2d; Rohan, 2026-11-09): decided at each
//! launch, from that launch alone, never stored.
//!
//! - **Jailed and not forced ask** (tiers A and B, the Safe workspace): Claude Code's [`AUTO`]
//!   mode, so Claude works on its own inside the matter. The jail is what fences it there:
//!   every fence Clauseo puts on a Claude session (the data folder's deny rules, the pre-write
//!   guard's hook, `disallowedTools` and the web rules, the proxy's variables) still holds in
//!   [`AUTO`] and in [`UNAVAILABLE`], measured with the pinned Claude Code (decision record
//!   0051, "R17 PR-D2d"). A shell command Claude's deny rules do not parse goes to auto's
//!   classifier, not to a permission request, so the host's auto-deny never sees it; only the
//!   jail, which hides the data folder whole, stops it. That is why no unjailed launch may run
//!   in [`AUTO`].
//! - **Unjailed (tier C) or forced ask** (an unprotected matter folder): [`CLAUDE_MODE`]
//!   (`default`), asking before each change, as before.
//!
//! A refused [`AUTO`] never fails the launch: the session runs in its [`Fallback`]'s mode, and the
//! thread is told once ([`Fallback::notice`]).
//!
//! [`CLAUDE_MODE`]: crate::agent::loads::CLAUDE_MODE

use clauseo_protocol::{Notice, NoticeKind, NoticeLevel};

use crate::agent::loads::CLAUDE_MODE;

/// Claude Code's auto mode: no prompt for an edit or a command in the matter; a classifier
/// model reviews each action, and after 3 refusals in a row or 20 in all Claude asks again
/// (an ordinary permission request, CLI 2.1.293).
pub(crate) const AUTO: &str = "auto";

/// The mode a jailed launch falls back to when nobody chose to turn [`AUTO`] off (the plan, a
/// server switch, the model): edits in the matter without asking, commands still ask. It is also
/// claude-agent-acp 0.89.1's own fallback for a model without auto (`session-mode.js:46-53`).
pub(crate) const UNAVAILABLE: &str = "acceptEdits";

/// What Claude Code 2.1.293 says, in a refused `session/set_mode`'s `data.details`, when a
/// `disableAutoMode` setting turned auto off: from the firm's managed policy and from the user
/// settings alike ("Cannot set permission mode to auto: auto mode disabled by settings").
const TURNED_OFF: &str = "auto mode disabled by settings";

/// The mode a Claude launch asks for: [`AUTO`] when `jailed` and not `forced_ask`, else
/// [`CLAUDE_MODE`]. The one place that picks it.
///
/// No unjailed launch runs in [`AUTO`] (or [`UNAVAILABLE`], its fallback): this module's tests
/// and `tests/host/claude_auto.rs` pin that for every launch kind. The one exception planned is
/// R17 PR-D2f's "protection switched off" (auto without the jail, when the lawyer turns
/// protection off and the firm allows it): it adds its own input here and its own row to both
/// tables.
pub(crate) const fn launch_mode(jailed: bool, forced_ask: bool) -> &'static str {
    if jailed && !forced_ask {
        AUTO
    } else {
        CLAUDE_MODE
    }
}

/// Why a jailed launch runs in another mode than [`AUTO`], and so which one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Fallback {
    /// Nobody at the firm chose it: the plan, a server switch, or the model. [`UNAVAILABLE`].
    Unavailable,
    /// A `disableAutoMode` setting, the firm's or the lawyer's own. [`CLAUDE_MODE`], asking
    /// before each change, since someone turned auto off on purpose.
    TurnedOff,
}

impl Fallback {
    /// The fallback for a refused `session/set_mode` to [`AUTO`]: [`Fallback::TurnedOff`] when
    /// Claude Code says a setting turned auto off ([`TURNED_OFF`]), else
    /// [`Fallback::Unavailable`].
    pub(crate) fn of_refusal(error: &clauseo_acp::Error) -> Self {
        if let clauseo_acp::Error::Rpc {
            details: Some(details),
            ..
        } = error
            && details.contains(TURNED_OFF)
        {
            Self::TurnedOff
        } else {
            Self::Unavailable
        }
    }

    /// The fallback a jailed launch that asked for [`AUTO`] shows by running in `mode`, if any.
    pub(crate) fn of_mode(mode: &str) -> Option<Self> {
        match mode {
            UNAVAILABLE => Some(Self::Unavailable),
            CLAUDE_MODE => Some(Self::TurnedOff),
            _ => None,
        }
    }

    /// The mode the session runs in instead.
    pub(crate) const fn mode(self) -> &'static str {
        match self {
            Self::Unavailable => UNAVAILABLE,
            Self::TurnedOff => CLAUDE_MODE,
        }
    }

    /// The cause as a kind fit for a log line (no value).
    pub(crate) const fn kind(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::TurnedOff => "turned_off",
        }
    }

    /// The thread's notice. The app words it by kind; `text` is the fallback older apps show
    /// (provisional words; the app thread owns them).
    pub(crate) fn notice(self) -> Notice {
        let (text, kind) = match self {
            Self::Unavailable => (
                "Claude's auto mode isn't available for this sign-in, so Claude will ask before \
                 running programs. It still changes files in this matter without asking.",
                NoticeKind::ClaudeAutoUnavailable,
            ),
            Self::TurnedOff => (
                "Claude asks before each change here, because auto mode is turned off on this \
                 computer.",
                NoticeKind::ClaudeAutoTurnedOff,
            ),
        };
        Notice {
            level: NoticeLevel::Info,
            text: text.to_owned(),
            kind: Some(kind),
        }
    }
}

#[cfg(test)]
mod tests;
