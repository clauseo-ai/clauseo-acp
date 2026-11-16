//! A change to the fence between matters reaching a live agent. An agent's fence is made once,
//! at its launch (`agent::matter_fence`), so an agent whose launch would fence other matters
//! now is stopped as an idle agent is: its session detaches `IdleStopped`, which the app shows
//! as resting, and its next prompt relaunches it with the new rules and resumes the session.
//!
//! - After each input, each live agent's launch key (`FenceKey`: the matters' ids and folders,
//!   the conversation's invites) is compared with the key a launch would take now
//!   (`turn::outside::fence`). A different key stops it once [`super::stoppable`]: nothing pending,
//!   so a turn, a queued prompt, a permission or a launch keeps it until it ends (a busy agent
//!   keeps its old rules until then; the host's auto-deny reads the new ones at once). A turn
//!   that ends with prompts queued stops a stale agent before the next one starts
//!   ([`stop_stale_at_turn_end`]): the relaunch sends the queue. A change
//!   put back before then stops nothing. A stopping agent is not stoppable, so quick changes
//!   make one stop, and the agent relaunched takes the newest key.
//! - A read taken away while a turn runs (an invite withdrawn, or the invited matter forgotten)
//!   doesn't wait: the turn is stopped at once, as the lawyer's Stop does
//!   (`cancel::cancel_current`), with one notice naming the matter
//!   (`NoticeKind::MatterReadStopped`), so the agent reads nothing more there; the stops above
//!   then relaunch it. A queue the stop holds waits for the lawyer, and its next prompt meets the
//!   new rules (`agents::ensure`). A read added mid-turn waits for the next prompt, as before.
//! - Shape shared with the Tools change (`idle/tools.rs`); its stop waits out a grace, this one
//!   doesn't.
//! - The log names the thread only.

use clauseo_protocol::{AgentKind, MatterId, Notice, NoticeKind, NoticeLevel, ThreadId};

use super::super::super::Actor;
use super::super::outside::fence::{key_of, matters_hash, read_matters};
use super::super::{TurnPhase, agents, cancel, current};
use crate::log_file::{Cause, LIFECYCLE};

/// Stops each live agent whose fence is out of date and that has nothing pending.
pub(super) fn stop_stale(actor: &mut Actor) {
    let live: Vec<ThreadId> = actor.turn_path.idle.live.iter().copied().collect();
    if live.is_empty() {
        return;
    }
    let matters = matters_hash(actor);
    for thread in live {
        stop_turn_if_narrowed(actor, thread);
        if stale(actor, thread, matters) && super::stoppable(actor, thread) == Some(true) {
            tracing::info!(target: LIFECYCLE, %thread, "agent stopped for a matter fence change");
            agents::idle_stop(actor, thread);
        }
    }
}

/// A turn of `thread` closed and its queue holds more: an agent whose fence is out of date is
/// stopped now, before the queue moves on, so a queued prompt (one sent after a withdrawn
/// invite) runs on the relaunched agent with the new rules. The head waits for the stop
/// (`agents::after_stop`), and the relaunch takes it. Called before `queue::move_on`.
pub(in crate::actor::turn) fn stop_stale_at_turn_end(actor: &mut Actor, thread: ThreadId) {
    let waits = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| !entry.turn.queue.is_empty());
    let attached = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.agent.as_deref())
        .is_some_and(|agent| agent.stage == super::super::agents::Stage::Attached);
    if waits && attached && stale(actor, thread, matters_hash(actor)) {
        tracing::info!(
            target: LIFECYCLE,
            %thread,
            "agent stopped at its turn's end for a matter fence change"
        );
        agents::idle_stop(actor, thread);
    }
}

/// `waiting` back to run as usual (`agents::ensure`), unless it is a turn whose agent is attached
/// with a stale fence ([`fence_stale`]): that agent stops first and the turn waits for the
/// relaunch, so the prompt never runs on the old rules.
pub(in crate::actor::turn) fn stop_stale_first(
    actor: &mut Actor,
    thread: ThreadId,
    waiting: agents::Waiting,
) -> Option<agents::Waiting> {
    let turn = matches!(waiting, agents::Waiting::Turn(_));
    if !turn || !fence_stale(actor, thread) {
        return Some(waiting);
    }
    let agent = actor
        .threads
        .get_mut(&thread)
        .and_then(|entry| entry.turn.agent.as_deref_mut())
        .filter(|agent| agent.stage == agents::Stage::Attached && !agent.link.closed());
    let Some(agent) = agent else {
        return Some(waiting);
    };
    agent.after_stop.push(waiting);
    tracing::info!(target: LIFECYCLE, %thread, "agent stopped before a prompt for a matter fence change");
    agents::idle_stop(actor, thread);
    None
}

/// Whether `thread`'s live launch took another fence key than a launch would take now, for a
/// prompt about to reach it (`agents::ensure`): a read invite withdrawn or a matter added while
/// the prompt waited on its command stops the agent first, and the prompt waits for the
/// relaunch, so it never runs on the old rules.
fn fence_stale(actor: &Actor, thread: ThreadId) -> bool {
    stale(actor, thread, matters_hash(actor))
}

/// Whether `thread`'s live launch took another fence key than a launch would take now.
fn stale(actor: &Actor, thread: ThreadId, matters: u64) -> bool {
    let launched = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.run.as_ref())
        .map(|run| run.data_deny.matters.key);
    launched.is_some_and(|launched| launched != key_of(actor, thread, matters))
}

/// `thread`'s live launch let it read a matter it may no longer read (the invite withdrawn, or
/// the matter forgotten) and a turn runs that the lawyer's Stop would cancel: the turn is
/// cancelled that way now (`cancel::cancel_current`), with one notice naming the matter: one
/// checkpointing, starting or prompting on that attached agent. A cancelled turn, or one
/// already ending, is left alone, so the notice goes once a turn; a turn not yet past its launch
/// (accepting, queued, or waiting on a relaunch) meets the new rules at `agents::ensure`.
fn stop_turn_if_narrowed(actor: &mut Actor, thread: ThreadId) {
    let Some((matter, name)) = narrowed(actor, thread) else {
        return;
    };
    // The prompt is with the stale agent, or about to go to it: a turn whose agent is stopping
    // (or not up) relaunches it first, and `agents::ensure` stops a stale one before a prompt.
    let attached = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.agent.as_deref())
        .is_some_and(|agent| agent.stage == agents::Stage::Attached);
    let running = attached
        && current(actor, thread).is_some_and(|current| {
            !current.cancelled
                && matches!(
                    current.phase,
                    TurnPhase::Checkpointing | TurnPhase::Starting | TurnPhase::Prompting
                )
        });
    if !running {
        return;
    }
    tracing::info!(target: LIFECYCLE, %thread, "a turn stopped: a matter it could read was taken away");
    let kind = actor
        .threads
        .get(&thread)
        .and_then(|entry| AgentKind::of(&entry.row.agent));
    let agent = kind.map_or("the AI", clauseo_profile::instructions::plan_name);
    let notice = Notice {
        level: NoticeLevel::Warning,
        text: format!("Stopped this answer so {agent} can no longer read another matter's files."),
        kind: Some(NoticeKind::MatterReadStopped { matter, name }),
    };
    if let Err(error) = super::super::updates::commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "the read-stopped notice was not logged");
    }
    cancel::cancel_current(actor, thread, cancel::why::FENCE);
}

/// The first matter `thread`'s live launch let it read that it may no longer read: not invited
/// now, or no longer on the list (forgotten), with its name as the launch knew it.
fn narrowed(actor: &Actor, thread: ThreadId) -> Option<(MatterId, String)> {
    let run = actor.threads.get(&thread)?.turn.run.as_ref()?;
    let invited = read_matters(actor, thread);
    run.data_deny
        .matters
        .others
        .iter()
        .find(|other| {
            other.invited && !(invited.contains(&other.id) && actor.matters.contains_key(&other.id))
        })
        .map(|other| (other.id, other.name.clone()))
}
