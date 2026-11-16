//! The read-back of a Claude session's mode after the host's `session/set_mode`: an empty
//! answer does not prove the mode changed, so the host takes the mode as set only when the agent
//! itself reports it. claude-agent-acp 0.89.1's `setSessionMode` sends a `current_mode_update`
//! when the effective mode differs, then always a `config_option_update` with the mode option,
//! before it answers (`session-mode.js:159-166`, `acp-agent.js:6714-6722`). The mode the agent
//! reports is either ([`reported`]); the last one after the switch went out counts.
//!
//! - At launch the updates are held until the attach, so `session/claude_mode.rs` reads them
//!   from the held list ([`last_reported`]).
//! - During the session they are logged as they come, so the agent keeps a count and the last
//!   mode it reported ([`Reports`], noted before the rate cap, `updates.rs`); a call takes a
//!   [`Mark`] as it goes out and awaits the link's fence before it answers, so every update the
//!   agent sent first is noted by then. The lawyer's `SetMode` (`answers.rs`), the switch back
//!   (`forced_mode.rs`) and the model switch's revert (`model_switch.rs`) each [`check`] it;
//!   a mode that is not the one asked for fails with `agent.mode_not_set` ([`not_set`]) and
//!   [`stop`]s the conversation, so the next message launches the agent again.
//!
//! Only a Claude agent is read back: Codex's adapter is not known to report its mode.

use clauseo_acp::{AcpEvent, AgentUpdate};
use clauseo_protocol::{
    AgentKind, CompactString, ConfigCategory, ConfigOptionKind, ErrorCode, ThreadId,
};

use super::super::super::Actor;
use super::super::agents;
use super::super::end::end_turn;
use super::super::updates::agent;
use super::super::{End, TurnPhase, current};
use crate::error::Error;

/// The modes an agent reported during its session: how many reports, and the last mode.
#[derive(Debug, Default)]
pub(crate) struct Reports {
    count: u64,
    last: Option<CompactString>,
}

/// Where an agent's [`Reports`] stood when a call went out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mark(u64);

impl Reports {
    /// Notes the mode `event` reports, if any.
    pub(crate) fn note(&mut self, event: &AcpEvent) {
        let AcpEvent::Update { update, .. } = event else {
            return;
        };
        if let Some(mode) = reported(update) {
            self.count += 1;
            self.last = Some(CompactString::from(mode));
        }
    }

    /// Where the reports stand now.
    pub(crate) fn mark(&self) -> Mark {
        Mark(self.count)
    }

    /// The last mode reported since `mark`, if any was.
    pub(crate) fn since(&self, mark: Mark) -> Option<&str> {
        (self.count > mark.0)
            .then_some(self.last.as_deref())
            .flatten()
    }
}

/// The mode `update` reports: a `current_mode_update`'s, or the current value of the
/// mode-category select option in a `config_option_update`.
pub(crate) fn reported(update: &AgentUpdate) -> Option<&str> {
    if let AgentUpdate::CurrentMode { mode_id } = update {
        return Some(mode_id.as_str());
    }
    let AgentUpdate::ConfigOptions(options) = update else {
        return None;
    };
    options
        .iter()
        .filter(|option| option.category == ConfigCategory::Mode)
        .find_map(|option| match &option.kind {
            ConfigOptionKind::Select { current_value, .. } => Some(current_value.as_str()),
            ConfigOptionKind::Boolean { .. } | ConfigOptionKind::Unknown { .. } => None,
        })
}

/// The last mode `events` report, if any.
pub(crate) fn last_reported(events: &[AcpEvent]) -> Option<&str> {
    events.iter().rev().find_map(|event| {
        let AcpEvent::Update { update, .. } = event else {
            return None;
        };
        reported(update)
    })
}

/// The failure when the agent reported `reported` (or nothing) after a switch to `wanted`, with
/// its one warn line. The modes are the agent's ids, cut short; nothing else is named.
pub(crate) fn not_set(thread: ThreadId, wanted: &str, reported: Option<&str>) -> Error {
    let reported: Option<String> = reported.map(|mode| mode.chars().take(40).collect());
    tracing::warn!(
        %thread,
        wanted,
        reported = reported.as_deref().unwrap_or("nothing"),
        "Claude Code did not confirm Clauseo's mode: the conversation is stopped"
    );
    let said = reported.map_or_else(|| "no mode".to_owned(), |mode| format!("\"{mode}\""));
    Error::refused(
        ErrorCode::AgentModeNotSet,
        format!(
            "Claude Code did not confirm the \"{wanted}\" mode Clauseo set (it reported {said}), \
             so Clauseo sent no message and stopped the conversation"
        ),
    )
}

/// A mid-session switch of `thread`'s agent to `wanted`, sent at `mark`, answered well: `Ok`
/// when the agent is not Claude's or reported `wanted` last since, else [`not_set`].
pub(crate) fn check(
    actor: &mut Actor,
    thread: ThreadId,
    wanted: &str,
    mark: Mark,
) -> Result<(), Error> {
    let claude = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Claude));
    let Some(agent) = agent(actor, thread) else {
        return Ok(());
    };
    // A forced-ask agent is always Claude's (`agents.rs`).
    if !claude && !agent.forced_ask {
        return Ok(());
    }
    match agent.modes.since(mark) {
        Some(mode) if mode == wanted => Ok(()),
        reported => {
            let reported = reported.map(str::to_owned);
            Err(not_set(thread, wanted, reported.as_deref()))
        }
    }
}

/// Stops `thread`'s conversation for `error`: a turn whose prompt runs fails with it (or ends
/// cancelled when the lawyer cancelled it), and the agent is stopped, so the next message
/// launches it again. A turn in any other phase meets no agent at its prompt and launches one.
pub(crate) fn stop(actor: &mut Actor, thread: ThreadId, error: &Error) {
    let prompting = current(actor, thread)
        .filter(|current| current.phase == TurnPhase::Prompting)
        .map(|current| current.cancelled);
    if let Some(cancelled) = prompting {
        let end = if cancelled {
            End::cancelled(None)
        } else {
            End::failed(error.code(), Some(error.to_string()), None)
        };
        end_turn(actor, thread, end);
    }
    agents::stop(actor, thread);
}

#[cfg(test)]
mod tests;
