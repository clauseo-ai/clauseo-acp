//! An unjailed Codex's (tier C) own moves to a mode that approves without asking
//! (`codex::approves_unasked`), R17 PR-D2c rounds 5 to 7.
//!
//! - The lawyer's mode command becomes the agent's pick when it is sent ([`picked`]); the
//!   agent's report of that mode stands while the command is in flight. The pick holds once
//!   the command succeeded with that mode ([`answered`]); a failed `SetMode` or
//!   `SetConfigOption`, or one answered with another value, drops it, and a report of it made
//!   meanwhile is switched back like any other move.
//! - Any other move there ([`moved`]), from `current_mode_update` or a mode option's value in
//!   `config_option_update`, is switched back to `codex::ASK_MODE`; one switch back at a time.
//!   A mode reported while one is in flight is checked when it is answered: one that approves
//!   without asking gets one more switch back, and after that the agent is stopped.
//! - A switch back that fails or goes unanswered stops the agent ([`stop_unasked`]): its turn is
//!   cancelled, its tree killed at once (no graces), its session ends the normal way through the
//!   launch gate, and one notice (`NoticeKind::ModeSetBackFailed`) says so.
//! - Its mode log lines come once per kind per turn ([`first`]), with a count at the turn's
//!   end ([`flush`]), the mode id capped at [`MODE_LOG_CHARS`] characters ([`capped`]).

use std::borrow::Cow;
use std::time::Duration;

use clauseo_protocol::{CommandId, CompactString, Notice, NoticeKind, NoticeLevel, ThreadId};

use super::super::super::Actor;
use super::super::agents::{self, Stage};
use super::super::updates::{agent, commit};
use super::super::{cancel, current};
use crate::agent::codex;
use crate::error::Error;
use crate::log_file::Cause;

/// How long the host waits for the agent's answer to a switch back before it counts as failed.
pub(in super::super) const SET_BACK_WAIT: Duration = Duration::from_secs(10);

/// The most characters of a mode id a log line carries; a longer one ends in an ellipsis.
const MODE_LOG_CHARS: usize = 64;

/// The notice's fallback text (provisional, app thread): the app words it by kind.
pub(in super::super) const STOPPED: &str = "ChatGPT switched itself to working without asking \
     you, and Clauseo couldn't switch it back, so Clauseo stopped it. Start ChatGPT again to \
     carry on.";

/// A Codex agent's mode state at tier C, and its mode log lines' counts (`CodexAgent::modes`).
#[derive(Debug, Default)]
pub(in super::super) struct CodexModes {
    /// The lawyer's last mode command for it, from when it was sent.
    pick: Option<Pick>,
    /// A switch back to the ask mode is in flight.
    resetting: bool,
    /// The last mode the agent reported while a switch back was in flight, checked once that
    /// switch back is answered ([`reset_answered`]).
    during: Option<CompactString>,
    /// The switch back in flight is the second for one move: a third stops the agent.
    again: bool,
    /// This turn's mode log lines, by [`Kind`].
    seen: Seen,
}

/// This turn's mode log lines, one count per [`Kind`].
#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    moved: u32,
    set_back: u32,
    started: u32,
}

impl Seen {
    const fn count(&mut self, kind: Kind) -> &mut u32 {
        match kind {
            Kind::Moved => &mut self.moved,
            Kind::SetBack => &mut self.set_back,
            Kind::Started => &mut self.started,
        }
    }
}

/// The lawyer's mode command.
#[derive(Debug)]
struct Pick {
    /// The command, which its answer names.
    command: CommandId,
    mode: CompactString,
    /// It succeeded.
    answered: bool,
    /// The agent reported its mode while it was in flight.
    reported: bool,
}

/// A mode log line's kind, logged once per turn.
#[derive(Debug, Clone, Copy)]
pub(in super::super) enum Kind {
    /// The agent moved itself to a mode that approves without asking.
    Moved,
    /// The host switches a session back.
    SetBack,
    /// A session started in a mode that approves without asking.
    Started,
}

impl Kind {
    const ALL: [Self; 3] = [Self::Moved, Self::SetBack, Self::Started];

    pub(in super::super) const fn name(self) -> &'static str {
        match self {
            Self::Moved => "agent_moved_to_full_access",
            Self::SetBack => "mode_set_back",
            Self::Started => "agent_started_in_full_access",
        }
    }
}

/// `thread`'s Codex agent's mode state.
fn modes(actor: &mut Actor, thread: ThreadId) -> Option<&mut CodexModes> {
    agent(actor, thread)
        .and_then(|agent| agent.codex.as_mut())
        .map(|codex| &mut codex.modes)
}

/// Whether the `kind` line is the first this turn, counting it. Any agent but Codex: always.
pub(in super::super) fn first(actor: &mut Actor, thread: ThreadId, kind: Kind) -> bool {
    let Some(modes) = modes(actor, thread) else {
        return true;
    };
    let count = modes.seen.count(kind);
    *count = count.saturating_add(1);
    *count == 1
}

/// The turn ended: each kind logged more than once this turn is logged with its count.
pub(in super::super) fn flush(actor: &mut Actor, thread: ThreadId) {
    let Some(modes) = modes(actor, thread) else {
        return;
    };
    let mut seen = std::mem::take(&mut modes.seen);
    for kind in Kind::ALL {
        let count = *seen.count(kind);
        if count > 1 {
            tracing::info!(kind = kind.name(), count, "mode lines this turn");
        }
    }
}

/// `mode` for a log line: at most [`MODE_LOG_CHARS`] characters, then an ellipsis.
pub(in super::super) fn capped(mode: &str) -> Cow<'_, str> {
    if mode.chars().nth(MODE_LOG_CHARS).is_none() {
        return Cow::Borrowed(mode);
    }
    let mut cut: String = mode.chars().take(MODE_LOG_CHARS).collect();
    cut.push('…');
    Cow::Owned(cut)
}

/// The lawyer's mode command `command` for `thread`'s unjailed Codex is sent: `mode` is its
/// pick.
pub(in super::super) fn picked(
    actor: &mut Actor,
    thread: ThreadId,
    command: CommandId,
    mode: CompactString,
) {
    if let Some(modes) = modes(actor, thread) {
        modes.pick = Some(Pick {
            command,
            mode,
            answered: false,
            reported: false,
        });
    }
}

/// The lawyer's mode command `command` answered with `mode`: the mode it now runs, or `None`
/// when it failed or its answer names none. The pick holds when that is its mode, else it is
/// dropped (a failed `SetMode`, a failed `SetConfigOption`, or one answered with another
/// value). A dropped pick's mode comes back when the agent reported it meanwhile and it
/// approves without asking, to be switched back.
pub(in super::super) fn answered(
    actor: &mut Actor,
    thread: ThreadId,
    command: CommandId,
    mode: Option<&str>,
) -> Option<CompactString> {
    let modes = modes(actor, thread)?;
    let pick = modes.pick.as_mut().filter(|pick| pick.command == command)?;
    if mode == Some(pick.mode.as_str()) {
        pick.answered = true;
        return None;
    }
    let pick = modes.pick.take()?;
    if !pick.reported || !codex::approves_unasked(&pick.mode) {
        return None;
    }
    if modes.resetting {
        modes.during = Some(pick.mode);
        return None;
    }
    if first(actor, thread, Kind::Moved) {
        tracing::info!(
            kind = Kind::Moved.name(),
            mode = %capped(&pick.mode),
            "an unjailed Codex reported the mode of a lawyer's command that did not hold"
        );
    }
    Some(pick.mode)
}

/// `thread`'s unjailed Codex reported `mode` (`from` the update it came in): whether to switch
/// it back, a mode that approves without asking that is not the lawyer's pick, with no switch
/// back in flight; one reported while a switch back is in flight is kept for its answer
/// ([`reset_answered`]). A `current_mode_update` of another mode ends a pick that succeeded.
pub(in super::super) fn moved(
    actor: &mut Actor,
    thread: ThreadId,
    mode: &str,
    from: &'static str,
) -> bool {
    let Some(modes) = modes(actor, thread) else {
        return false;
    };
    if let Some(pick) = &mut modes.pick {
        if pick.mode == mode {
            pick.reported |= !pick.answered;
            return false;
        }
        if pick.answered && from == "current_mode_update" {
            modes.pick = None;
        }
    }
    if modes.resetting {
        modes.during = Some(CompactString::from(mode));
        return false;
    }
    if !codex::approves_unasked(mode) {
        return false;
    }
    if first(actor, thread, Kind::Moved) {
        tracing::info!(
            kind = Kind::Moved.name(),
            from,
            mode = %capped(mode),
            "an unjailed Codex moved itself to a mode that approves without asking"
        );
    }
    true
}

/// A switch back of `thread`'s unjailed Codex is sent.
pub(in super::super) fn resetting(actor: &mut Actor, thread: ThreadId) {
    if let Some(modes) = modes(actor, thread) {
        modes.resetting = true;
    }
}

/// What follows an unjailed Codex's switch back once answered.
#[derive(Debug)]
pub(in super::super) enum After {
    /// It holds: log the ask mode.
    Done,
    /// It succeeded, but the agent reported this mode, which approves without asking, while it
    /// was in flight: log the ask mode and switch back once more.
    Again(CompactString),
    /// It failed or went unanswered, or the agent moved again during the second switch back
    /// for one move: stop the agent ([`stop_unasked`]).
    Stop,
}

/// A switch back of `thread`'s agent answered, `ok` when it succeeded: what follows when it
/// was an unjailed Codex's, `None` otherwise.
pub(in super::super) fn reset_answered(
    actor: &mut Actor,
    thread: ThreadId,
    ok: bool,
) -> Option<After> {
    let modes = modes(actor, thread)?;
    if !std::mem::take(&mut modes.resetting) {
        return None;
    }
    let during = modes.during.take();
    if !ok {
        return Some(After::Stop);
    }
    let Some(mode) = during.filter(|mode| codex::approves_unasked(mode)) else {
        modes.again = false;
        return Some(After::Done);
    };
    if std::mem::replace(&mut modes.again, true) {
        return Some(After::Stop);
    }
    if first(actor, thread, Kind::Moved) {
        tracing::info!(
            kind = Kind::Moved.name(),
            mode = %capped(&mode),
            "an unjailed Codex moved itself again while it was switched back"
        );
    }
    Some(After::Again(mode))
}

/// The switch back of `thread`'s unjailed Codex failed, went unanswered, or did not hold: one
/// notice, its turn cancelled, and the agent stopped at once, its tree killed with no graces
/// (`AgentLink::stop_now`); its session ends through the launch gate as any stop's.
pub(in super::super) fn stop_unasked(actor: &mut Actor, thread: ThreadId, error: &Error) {
    if agent(actor, thread).is_none_or(|agent| agent.stage == Stage::Stopping) {
        return;
    }
    tracing::warn!(
        %thread,
        error = %Cause(error),
        "an unjailed Codex was not switched back from a mode that approves without asking; \
         stopping it"
    );
    let notice = Notice {
        level: NoticeLevel::Warning,
        text: STOPPED.to_owned(),
        kind: Some(NoticeKind::ModeSetBackFailed),
    };
    if let Err(error) = commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "the stop's notice was not logged");
    }
    if current(actor, thread).is_some_and(|current| !current.cancelled) {
        cancel::cancel_current(actor, thread);
    }
    if let Some(agent) = agent(actor, thread) {
        agent.link.stop_now();
    }
    agents::stop(actor, thread);
}

#[cfg(test)]
mod tests;
