//! A Claude session's start mode (`agent/claude_mode.rs`): the mode its launch asks for,
//! switched to before the first prompt, with a refused auto falling back by its cause; and the
//! notice that tells the thread, once, that a jailed launch runs in a fallback.

use clauseo_acp::{AcpClient, AcpEvent, AgentUpdate, Origin, SessionStart};
use clauseo_protocol::{AgentChoiceId, CompactString, ErrorCode, Notice, ThreadId};

use super::super::agents::{call, launch_failed};
use super::super::{AgentReply, state};
use super::attach;
use crate::actor::Actor;
use crate::agent::claude_mode::{AUTO, Fallback};
use crate::error::Error;
use crate::log_file::Cause;

/// A Claude session is up. claude-agent-acp 0.89.1 starts it in the user's own
/// `permissions.defaultMode` (or a resumed transcript's mode), which `_meta` cannot override
/// (see `agent/loads.rs`), so `session/set_mode` to the launch's mode (`Agent::launch_mode`,
/// from [`launch_mode`](crate::agent::claude_mode::launch_mode)) goes first, before any prompt;
/// a session already in it attaches now. A refused switch to auto falls back by its cause
/// ([`switch`]); any other failed switch fails the launch, so no prompt runs in the user's mode.
/// One that reports no modes, or offers neither the launch's mode nor (for auto) the fallback,
/// fails the launch ([`no_mode`]): the host could not tell or change the mode it runs in. On load
/// this also resets a mode the session reached before (see [`crate::agent::loads::CLAUDE_MODE`]).
pub(super) fn start(actor: &mut Actor, thread: ThreadId, start: SessionStart, loaded: bool) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref()) else {
        return;
    };
    let mut mode = agent.launch_mode;
    let Some(modes) = start.modes.as_ref() else {
        return no_mode(actor, thread, "reported no modes", mode);
    };
    let offered = |id: &str| modes.available.iter().any(|offer| offer.id == id);
    if mode == AUTO && !offered(AUTO) {
        let fallback = Fallback::Unavailable;
        tracing::info!(
            %thread,
            cause = "not_offered",
            fallback = fallback.kind(),
            "Claude offered no auto mode; the session runs in its fallback"
        );
        mode = fallback.mode();
    }
    if modes.current == mode {
        return attach(actor, thread, start, loaded);
    }
    let (true, Ok(ask)) = (offered(mode), AgentChoiceId::new(mode)) else {
        return no_mode(actor, thread, &format!("offered no \"{mode}\" mode"), mode);
    };
    tracing::info!(
        %thread,
        from = %modes.current,
        to = mode,
        "switching the Claude session to its launch's mode"
    );
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.preparing = true;
    let (id, client, fence) = (agent.id, agent.link.client.clone(), agent.link.fence());
    call(actor, thread, id, async move {
        let result = switch(&client, thread, start, ask).await;
        fence.await;
        AgentReply::Prepared { loaded, result }
    });
}

/// `session/set_mode` to `ask`; the start comes back with the mode it now runs in. A refused
/// auto is never the launch's end: the session goes to the refusal's [`Fallback`] mode (the
/// cause is logged by kind), and only a failed fallback fails the launch.
async fn switch(
    client: &AcpClient,
    thread: ThreadId,
    mut start: SessionStart,
    ask: AgentChoiceId,
) -> Result<SessionStart, Error> {
    let mode = match client.set_mode(&start.session, ask.clone()).await {
        Ok(()) => ask,
        Err(error) if ask.as_str() == AUTO => {
            let fallback = Fallback::of_refusal(&error);
            tracing::info!(
                %thread,
                cause = fallback.kind(),
                fallback = fallback.mode(),
                "Claude refused auto mode; the session runs in its fallback"
            );
            let mode =
                AgentChoiceId::new(fallback.mode()).map_err(|_invalid| unset(fallback.mode()))?;
            let current = start.modes.as_ref().map(|modes| modes.current.as_str());
            if current != Some(fallback.mode()) {
                let offered = start.modes.as_ref().is_some_and(|modes| {
                    modes
                        .available
                        .iter()
                        .any(|offer| offer.id == fallback.mode())
                });
                if !offered {
                    return Err(unset(fallback.mode()));
                }
                client.set_mode(&start.session, mode.clone()).await?;
            }
            mode
        }
        Err(error) => return Err(Error::Acp(error)),
    };
    if let Some(modes) = &mut start.modes {
        modes.current = CompactString::from(mode.as_str());
    }
    Ok(start)
}

/// The error of a fallback the session does not offer.
fn unset(mode: &str) -> Error {
    Error::refused(
        ErrorCode::AgentProtocolError,
        format!(
            "Claude Code offered no \"{mode}\" mode, so Clauseo cannot run it and sent no message"
        ),
    )
}

/// Fails the launch of a Claude session whose mode the host cannot set to `mode`: `why` says what
/// the agent's answer lacked.
fn no_mode(actor: &mut Actor, thread: ThreadId, why: &str, mode: &str) {
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

/// The notice for `thread`'s Claude session that is attaching, if its launch asked for auto and
/// it runs in a fallback the thread was not told of yet (each [`Fallback`] once per thread for
/// the host's run). The mode it runs in is the last live `current_mode_update` held while it
/// started, else `start`'s: claude-agent-acp's own fallback for a model without auto answers the
/// switch with `current_mode_update` `acceptEdits` (logged here as the cause `model`).
pub(super) fn notice(actor: &mut Actor, thread: ThreadId, start: &SessionStart) -> Option<Notice> {
    let state = state(actor, thread)?;
    let agent = state.agent.as_deref()?;
    if agent.launch_mode != AUTO {
        return None;
    }
    let moved = agent.replayed.iter().rev().find_map(|event| {
        if let AcpEvent::Update {
            origin,
            update: AgentUpdate::CurrentMode { mode_id },
            ..
        } = event
            && *origin != Origin::Replay
        {
            Some(mode_id.as_str())
        } else {
            None
        }
    });
    let mode = moved.or_else(|| start.modes.as_ref().map(|modes| modes.current.as_str()))?;
    let fallback = Fallback::of_mode(mode)?;
    if moved.is_some() {
        tracing::info!(
            %thread,
            cause = "model",
            fallback = fallback.kind(),
            "Claude moved itself out of auto mode as it started"
        );
    }
    state.packs.told_claude(fallback).then(|| fallback.notice())
}
