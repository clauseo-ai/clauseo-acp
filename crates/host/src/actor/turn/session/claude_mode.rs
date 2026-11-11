//! A Claude session's mode at its start: Clauseo's (`agent::loads::claude_mode`: `auto`, or
//! `default` in forced ask), else, outside forced ask, `acceptEdits`
//! (`agent::loads::CLAUDE_FALLBACK_MODE`) when auto is not available, with one notice for the
//! lawyer per thread and host run ([`notice`]). Auto is not available when:
//!
//! - the session's `availableModes` lacks `auto` ([`claude_start`]);
//! - `session/set_mode auto` answers an error (`session.rs` `checks`): Claude Code refuses auto
//!   when the account's plan, the settings or fast mode rule it out ("Cannot set permission mode
//!   to auto: …", CLI 2.1.293), and claude-agent-acp 0.89.1 passes that back as the RPC error
//!   (`session-mode.js:143-152`);
//! - the agent reports a `current_mode_update` away from `auto` among the updates held between
//!   that switch and the attach ([`prepared`]): claude-agent-acp 0.89.1 itself moves a session
//!   whose model cannot run auto to `acceptEdits` and says so (`session-mode.js:98-102`,
//!   `:161-162`).
//!
//! claude-agent-acp 0.89.1 lists `auto` whatever the account (`session-mode.js:233-258`), and a
//! CLI that drops to `default` on its own (its remote kill switch) is never relayed, so the host
//! cannot see that case.

use clauseo_acp::{AcpEvent, AgentUpdate, SessionStart};
use clauseo_protocol::{AgentChoiceId, ErrorCode, Notice, NoticeKind, NoticeLevel, ThreadId};

use super::super::super::Actor;
use super::super::agents::launch_failed;
use super::super::picks::Pick;
use super::super::state;
use super::{Switch, attach, before_attach};
use crate::agent::loads::{CLAUDE_FALLBACK_MODE, CLAUDE_MODE, claude_mode};
use crate::error::Error;
use crate::log_file::Cause;

/// The host English of the notice a thread gets when Claude's `auto` mode is not available
/// (`NoticeKind::AutoModeUnavailable`), for a view that does not know the kind.
pub(crate) const AUTO_UNAVAILABLE: &str = "Claude's automatic mode isn't available for this \
     account, so Claude asks before running commands.";

/// Where a Claude agent stands with `auto`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Auto {
    /// Not switched by the host, or switched and kept: `auto` is the mode it goes back to.
    #[default]
    Wanted,
    /// `session/set_mode auto` went out before the attach; the agent's held updates from index
    /// `held` on are its answer.
    Set { held: usize },
    /// Not available: the session runs in `acceptEdits` and goes back to it.
    Unavailable,
}

/// A Claude agent's session started: when it is not in its mode (`auto`, or `default` in forced
/// ask), `session/set_mode` goes first, before any prompt, through `prepare`, since
/// claude-agent-acp 0.89.1 starts it in the user's own `permissions.defaultMode` (or a resumed
/// transcript's mode) and `_meta` cannot override that (see `agent/loads.rs`). Outside forced
/// ask a session that offers no `auto` runs in `acceptEdits` instead, and a refused switch to
/// `auto` falls back to it (`session.rs` `checks`). Any other failed switch fails the launch, so
/// no prompt runs in the user's mode. One that reports no modes, or offers no mode the host may
/// run it in, fails the launch ([`no_mode`]). On load and resume this also resets a mode the
/// lawyer reached in the session.
pub(super) fn claude_start(
    actor: &mut Actor,
    thread: ThreadId,
    (start, loaded): (SessionStart, bool),
    picks: Vec<Pick>,
) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    let forced_ask = agent.forced_ask;
    let wanted = claude_mode(forced_ask);
    let Some(modes) = start.modes.as_ref() else {
        return no_mode(actor, thread, wanted, "reported no modes");
    };
    let offered = |id: &str| modes.available.iter().any(|mode| mode.id == id);
    let target = if !forced_ask && (agent.auto == Auto::Unavailable || !offered(wanted)) {
        if agent.auto != Auto::Unavailable {
            tracing::info!(%thread, "Claude offered no auto mode; it runs in acceptEdits");
        }
        agent.auto = Auto::Unavailable;
        CLAUDE_FALLBACK_MODE
    } else {
        wanted
    };
    if modes.current == target {
        if picks.is_empty() {
            return attach(actor, thread, start, loaded);
        }
        return before_attach(actor, thread, (start, loaded), (None, None, picks));
    }
    let (true, Ok(mode)) = (offered(target), AgentChoiceId::new(target)) else {
        return no_mode(actor, thread, target, "offered no such mode");
    };
    let fallback = (target == CLAUDE_MODE && offered(CLAUDE_FALLBACK_MODE))
        .then(|| AgentChoiceId::new(CLAUDE_FALLBACK_MODE).ok())
        .flatten();
    if target == CLAUDE_MODE {
        agent.auto = Auto::Set {
            held: agent.replayed.len(),
        };
    }
    tracing::info!(%thread, from = %modes.current, to = target, "switching the Claude session to Clauseo's mode");
    let switch = Switch { mode, fallback };
    before_attach(actor, thread, (start, loaded), (Some(switch), None, picks));
}

/// Fails the launch of a Claude session whose mode the host cannot set to `mode`: `why` says
/// what the agent's answer lacked.
fn no_mode(actor: &mut Actor, thread: ThreadId, mode: &str, why: &str) {
    let error = Error::refused(
        ErrorCode::AgentProtocolError,
        format!(
            "Claude Code {why}, so Clauseo cannot run it in its \"{mode}\" mode and sent no message"
        ),
    );
    tracing::warn!(
        %thread,
        error = %Cause(&error),
        "the Claude session cannot run in Clauseo's mode"
    );
    launch_failed(actor, thread, &error);
}

/// `prepare` ended well. A switch to `auto` that fell back (`fell_back`), or whose held updates
/// since it went out move the session to another mode, leaves auto unavailable; one that held
/// keeps `auto` wanted. A held move to `acceptEdits` is then kept as it is logged; a move to
/// any other mode is switched back to `acceptEdits` (`forced_mode.rs`).
pub(super) fn prepared(actor: &mut Actor, thread: ThreadId, fell_back: bool) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    let Auto::Set { held } = agent.auto else {
        return;
    };
    let moved = agent
        .replayed
        .get(held..)
        .unwrap_or_default()
        .iter()
        .any(|event| {
            matches!(
                event,
                AcpEvent::Update {
                    update: AgentUpdate::CurrentMode { mode_id },
                    ..
                } if mode_id != CLAUDE_MODE
            )
        });
    agent.auto = if fell_back || moved {
        tracing::info!(%thread, fell_back, moved, "Claude's auto mode is not available; it runs in acceptEdits");
        Auto::Unavailable
    } else {
        Auto::Wanted
    };
}

/// The notice that auto is not available, for the attach commit: once per thread for the host's
/// run, and only while the agent runs without auto.
pub(super) fn notice(actor: &mut Actor, thread: ThreadId) -> Option<Notice> {
    let state = state(actor, thread)?;
    let unavailable = state
        .agent
        .as_deref()
        .is_some_and(|agent| agent.auto == Auto::Unavailable);
    if !unavailable || state.auto_told {
        return None;
    }
    state.auto_told = true;
    Some(Notice {
        level: NoticeLevel::Info,
        text: AUTO_UNAVAILABLE.to_owned(),
        kind: Some(NoticeKind::AutoModeUnavailable),
    })
}
