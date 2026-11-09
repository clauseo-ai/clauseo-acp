//! The modes the host forces on a session: forced ask's (`agent/forced_ask.rs`, a Claude agent
//! in an unprotected folder) and a jailed Codex's (`agent/codex/jailed.rs`, R17 PR-D2c).
//!
//! - `SetMode` and `SetConfigOption` on the mode are gated before they reach the agent
//!   ([`refusal`]).
//! - A jailed Codex's session starts in the mode [`codex_start`] picks, and its modes and config
//!   options offer only `codex::jailed::MODES` ([`codex_start`], [`kept`]).
//! - A mode the agent moves to itself outside its set is logged as it came, then
//!   `session/set_mode` puts the session back (`default` for forced ask, read-only for a jailed
//!   Codex), and that is logged once it answers. Meanwhile forced ask's guard hook still asks
//!   before every tool, so the race costs no unasked change.

use clauseo_acp::SessionStart;
use clauseo_protocol::{
    AgentChoiceId, CommandId, CompactString, ConfigOption, ConfigOptionsReplaced, ConfigValue,
    ErrorCode, ModeChanged, StreamId, ThreadId,
};

use super::super::Actor;
use super::agents::{Agent, Waiting, call};
use super::inputs::AgentReply;
use super::updates::{agent, commit};
use crate::agent::codex::{self, jailed};
use crate::agent::{forced_ask, loads};
use crate::error::Error;

/// `thread`'s agent, read only.
fn held(actor: &Actor, thread: ThreadId) -> Option<&Agent> {
    actor.threads.get(&thread)?.turn.agent.as_deref()
}

/// Whether `agent` is a Codex agent in the jail.
fn is_jailed(agent: &Agent) -> bool {
    agent.codex.as_ref().is_some_and(|codex| codex.jailed)
}

/// The mode `agent` is put back in when it moved itself to `mode`: `None` when `mode` is in its
/// set, or nothing forces its modes.
fn reset_to(agent: &Agent, mode: &str) -> Option<&'static str> {
    if agent.forced_ask && !forced_ask::mode_allowed(mode) {
        Some(loads::CLAUDE_MODE)
    } else if is_jailed(agent) && !jailed::allowed(mode) {
        Some(jailed::READ_ONLY)
    } else {
        None
    }
}

/// Why `waiting` may not reach `thread`'s agent, with the command it fails: a mode outside
/// forced ask's (`command.invalid`), or outside a jailed Codex's (`sandbox.ask_mode_unavailable`).
pub(super) fn refusal(
    actor: &Actor,
    thread: ThreadId,
    waiting: &Waiting,
) -> Option<(CommandId, Error)> {
    let agent = held(actor, thread)?;
    let (ctx, mode) = match waiting {
        Waiting::Turn(_) => return None,
        Waiting::Mode(ctx, mode) => (ctx, Some(mode.as_str())),
        Waiting::Config(ctx, option, value) => {
            let mode = match value {
                ConfigValue::Select(choice) => Some(choice.as_str()),
                ConfigValue::Boolean(_) => None,
            };
            let sets_mode = if is_jailed(agent) {
                jailed::is_mode_option(option.as_str(), &known_options(actor, thread))
            } else {
                !forced_ask::config_allowed(option.as_str(), mode)
            };
            if !sets_mode {
                return None;
            }
            (ctx, mode)
        }
    };
    if agent.forced_ask && !mode.is_some_and(forced_ask::mode_allowed) {
        let error = Error::refused(
            ErrorCode::CommandInvalid,
            format!(
                "mode {} would approve changes without asking, and this matter's folder is \
                 unprotected: only \"default\" and \"plan\" may be set",
                mode.unwrap_or("(none)")
            ),
        );
        return Some((ctx.id, error));
    }
    if is_jailed(agent) {
        return jailed::check(mode.unwrap_or_default())
            .err()
            .map(|error| (ctx.id, error));
    }
    None
}

/// The thread's config options as last logged.
fn known_options(actor: &Actor, thread: ThreadId) -> Vec<ConfigOption> {
    header_of(actor, thread)
        .map(|header| header.config_options.clone())
        .unwrap_or_default()
}

/// The thread's header: from the loaded stream, else as the actor kept it at the last unload.
fn header_of(actor: &Actor, thread: ThreadId) -> Option<&clauseo_protocol::HeaderEntry> {
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(actor.threads.get(&thread)?.header.as_ref())
}

/// A Codex session's start: the mode it must run in, and whether it must be switched there
/// first. Tier C: [`codex::ASK_MODE`]. Jailed: `jailed::start_mode` of the thread's saved mode
/// (its header's), checked, and the start's modes and options cut to `jailed::MODES` with the
/// mode option at that mode. A mode the agent does not offer fails with
/// `agent.protocol_error`.
pub(super) fn codex_start(
    actor: &Actor,
    thread: ThreadId,
    start: &mut SessionStart,
) -> Result<(AgentChoiceId, bool), Error> {
    let mode = if held(actor, thread).is_some_and(is_jailed) {
        let saved = header_of(actor, thread).and_then(|header| header.mode_id.as_deref());
        let had_session = actor
            .threads
            .get(&thread)
            .is_some_and(|entry| entry.row.acp_session_id.is_some());
        let mode = jailed::start_mode(saved, had_session);
        jailed::check(mode)?;
        if let Some(modes) = &mut start.modes {
            jailed::keep_modes(modes);
        }
        jailed::keep_options(&mut start.config_options, Some(mode));
        mode
    } else {
        codex::ASK_MODE
    };
    let offered = start
        .modes
        .as_ref()
        .is_some_and(|modes| modes.available.iter().any(|offered| offered.id == mode));
    if !offered {
        return Err(Error::refused(
            ErrorCode::AgentProtocolError,
            format!(
                "Codex offered no \"{mode}\" mode, which Clauseo runs it in, so no message was sent"
            ),
        ));
    }
    let id = AgentChoiceId::new(mode).map_err(|error| Error::Internal(error.to_string()))?;
    let switch = start
        .modes
        .as_ref()
        .is_some_and(|modes| modes.current != mode);
    Ok((id, switch))
}

/// The agent's config options for `thread`'s log: a jailed Codex's mode option keeps only
/// `jailed::MODES`.
pub(super) fn kept(
    actor: &Actor,
    thread: ThreadId,
    mut options: Vec<ConfigOption>,
) -> ConfigOptionsReplaced {
    if held(actor, thread).is_some_and(is_jailed) {
        jailed::keep_options(&mut options, None);
    }
    ConfigOptionsReplaced { options }
}

/// `mode` came from the agent: switch it back when its forced set does not hold it.
pub(super) fn reset_if_forced(actor: &mut Actor, thread: ThreadId, mode: &str) {
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    let Some(target) = reset_to(agent, mode) else {
        return;
    };
    let (Some(session), Ok(id)) = (agent.session.clone(), AgentChoiceId::new(target)) else {
        return;
    };
    let (agent_id, client) = (agent.id, agent.link.client.clone());
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    tracing::info!(
        matter = ?matter,
        mode,
        target,
        "a session moved to a mode outside the modes Clauseo allows it; switching it back"
    );
    call(actor, thread, agent_id, async move {
        let result = client.set_mode(&session, id).await;
        AgentReply::ModeReset(CompactString::from(target), result)
    });
}

/// The switch back to `mode` answered: it is logged; a failure is a warning (forced ask's hook
/// still asks).
pub(super) fn on_mode_reset(
    actor: &mut Actor,
    thread: ThreadId,
    same: bool,
    mode_id: CompactString,
    result: Result<(), clauseo_acp::Error>,
) {
    if !same {
        return;
    }
    let logged = result
        .map_err(Error::Acp)
        .and_then(|()| commit(actor, thread, [ModeChanged { mode_id }.into()], Vec::new()));
    if let Err(error) = logged {
        tracing::warn!(
            %thread,
            error = %crate::log_file::Cause(&error),
            "a session was not switched back to its allowed mode"
        );
    }
}
