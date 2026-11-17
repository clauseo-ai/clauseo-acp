//! Step 4's restart of a thread's agent for a turn: when its matter moved, or when it is marked
//! to relaunch (a Tools change).

use std::path::PathBuf;

use clauseo_protocol::{ThreadId, TurnId};

use super::super::super::Actor;
use super::super::{TurnPhase, current, state};
use super::{Stage, StopCause, Waiting};
use crate::log_file::LIFECYCLE;

/// Step 4 for a turn whose agent was started in another folder than its matter's row now names
/// (the matter was relocated since): stops that agent and starts a new one in the new folder for
/// the turn, which goes back to step 2 and keeps the writer. `false` when the agent's folder is
/// the matter's, or there is no running agent.
pub(in super::super) fn restart_if_moved(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
) -> bool {
    let Ok(row) = actor.check_thread(thread).map(|entry| entry.row.matter) else {
        return false;
    };
    let Ok(folder) = actor
        .check_matter(row)
        .map(|entry| PathBuf::from(&entry.row.folder_path))
    else {
        return false;
    };
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return false;
    };
    if agent.folder == folder || agent.stage == Stage::Stopping {
        return false;
    }
    tracing::info!(%thread, "the matter moved; its agent starts again in the new folder");
    restart_for(actor, thread, turn);
    true
}

/// Step 4 for a prompted turn (typed, or taken from the queue) whose agent is marked to relaunch
/// (`idle::Relaunch`, a Tools change): with nothing else on that agent (no question or call
/// open, no output held for a turn of its own) it stops and a new one starts for the turn, as
/// [`restart_if_moved`] does, so the prompt goes with the new inputs. An agent-started turn
/// that has opened is never cut: it is the current turn then, not this one. A command Claude put
/// in the background (a steer during a Bash call) that has not reported yet is cut: the host
/// sees it only when it reports, so the relaunch ends it and its result never comes. A relaunch
/// within `cap_min_idle` of a turn that closed after a steer during a tool call is logged.
/// `false` when nothing was restarted.
pub(in super::super) fn restart_if_marked(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
) -> bool {
    let Some(state) = state(actor, thread) else {
        return false;
    };
    let prompted = state
        .current
        .as_ref()
        .is_some_and(|current| current.turn == turn && current.own.is_none());
    let held = super::super::own_turn::output_waits(&state.own_held);
    let Some(agent) = state.agent.as_deref() else {
        return false;
    };
    let open = !(agent.permissions.is_empty()
        && agent.elicitations.is_empty()
        && agent.url_elicitations.is_empty());
    let up = agent.stage == Stage::Attached && !agent.link.closed();
    let Some(why) = agent.idle.relaunch else {
        return false;
    };
    if !prompted || held || open || !up || super::super::idle::calls(agent) > 0 {
        return false;
    }
    let steered_end = agent.idle.steered_end;
    tracing::info!(target: LIFECYCLE, %thread, why = why.as_str(), "agent relaunched for a prompt");
    let now = actor.now().mono;
    let window = actor.turn_path.idle.window();
    if let Some(at) = steered_end
        && now.saturating_sub(at) <= window
    {
        let since_ms = u64::try_from(now.saturating_sub(at).as_millis()).unwrap_or(u64::MAX);
        tracing::info!(
            target: LIFECYCLE,
            %thread,
            since_ms,
            "agent relaunched soon after a steer during a tool call; a background command may end"
        );
    }
    restart_for(actor, thread, turn);
    true
}

/// Stops `thread`'s agent and starts a new one for `turn`, which goes back to step 2 and keeps
/// the writer (`Restarting`).
fn restart_for(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    agent.link.stop();
    agent.stage = Stage::Stopping;
    agent.stop_cause = StopCause::Restarting;
    let waiting = std::mem::take(&mut agent.waiting);
    agent.after_stop.extend(waiting);
    agent.after_stop.push(Waiting::Turn(turn));
    if let Some(current) = current(actor, thread) {
        current.phase = TurnPhase::Launching;
    }
}
