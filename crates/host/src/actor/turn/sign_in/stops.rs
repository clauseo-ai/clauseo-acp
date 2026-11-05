//! `stop_agents`' answer (decision record 0048, P4d): the call answers once every process of
//! the agent id has exited, or once [`STOP_WAIT`] has passed on the host clock, whichever comes
//! first, and says which ([`AgentsStopped`]). It never hangs: the wait is a timer of its own,
//! beside the shutdown ladder's. An agent whose ladder ends with it unresponsive (it outlived
//! its kill) makes the answer `TimedOut` too, as it is not known to have exited.

use std::time::Duration;

use clauseo_protocol::{AgentId, ThreadId};
use futures::future::AbortHandle;

use super::super::super::Actor;
use super::super::super::input::Reply;
use super::super::{TimerKind, TurnTimer};
use crate::agent::login_link::AgentsStopped;

/// How long `stop_agents` waits for the agents' ends: the shutdown ladder's 7 s (2 s after
/// stdin closes, 3 s after SIGTERM, 2 s after the kill) and a margin for the descendants'
/// listing before it and the pipes after it.
pub(crate) const STOP_WAIT: Duration = Duration::from_secs(10);

/// The `stop_agents` calls waiting, and the number the next one gets.
#[derive(Debug, Default)]
pub(crate) struct StopWaits {
    next: u64,
    waits: Vec<StopWait>,
}

/// One `stop_agents` call waiting for its agents' ends.
#[derive(Debug)]
struct StopWait {
    id: u64,
    agent: AgentId,
    /// An agent of the id outlived its kill.
    unresponsive: bool,
    reply: Reply<AgentsStopped>,
    timer: AbortHandle,
}

/// A thread of `agent` whose agent has not ended yet: a process, or a launch being prepared.
fn live_thread(actor: &Actor, agent: &AgentId) -> Option<ThreadId> {
    actor
        .threads
        .iter()
        .find(|(_, entry)| {
            entry.row.agent == *agent
                && (entry.turn.agent.is_some() || entry.turn.preparing.is_some())
        })
        .map(|(thread, _)| *thread)
}

/// `stop_agents` has stopped every agent of `agent`: answers now when none is left, else once
/// the last one ends or [`STOP_WAIT`] has passed.
pub(super) fn wait(actor: &mut Actor, agent: AgentId, reply: Reply<AgentsStopped>) {
    let Some(thread) = live_thread(actor, &agent) else {
        let _gone = reply.send(Ok(AgentsStopped::Exited));
        return;
    };
    let stops = &mut actor.turn_path.stops;
    let id = stops.next;
    stops.next += 1;
    let timer = actor.arm_timer(
        STOP_WAIT,
        TurnTimer {
            thread,
            kind: TimerKind::StopWait(id),
        },
    );
    actor.turn_path.stops.waits.push(StopWait {
        id,
        agent,
        unresponsive: false,
        reply,
        timer,
    });
}

/// `thread`'s agent ended (`unresponsive`: it outlived its kill): each wait for its id
/// answers once no agent of the id is left.
pub(in crate::actor::turn) fn agent_ended(actor: &mut Actor, thread: ThreadId, unresponsive: bool) {
    let Some(agent) = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone())
    else {
        return;
    };
    let waits = &mut actor.turn_path.stops.waits;
    if !waits.iter().any(|wait| wait.agent == agent) {
        return;
    }
    for wait in waits.iter_mut().filter(|wait| wait.agent == agent) {
        wait.unresponsive |= unresponsive;
    }
    if live_thread(actor, &agent).is_some() {
        return;
    }
    let (done, left) = std::mem::take(&mut actor.turn_path.stops.waits)
        .into_iter()
        .partition(|wait| wait.agent == agent);
    actor.turn_path.stops.waits = left;
    for wait in done {
        wait.timer.abort();
        let stopped = if wait.unresponsive {
            AgentsStopped::TimedOut
        } else {
            AgentsStopped::Exited
        };
        tracing::info!(%agent, ?stopped, "the agents stopped for the login change");
        let _gone = wait.reply.send(Ok(stopped));
    }
}

/// The wait `id` ran out: it answers `TimedOut`. The agents' own stops go on.
pub(in crate::actor::turn) fn on_timer(actor: &mut Actor, id: u64) {
    let waits = &mut actor.turn_path.stops.waits;
    let Some(at) = waits.iter().position(|wait| wait.id == id) else {
        return;
    };
    let wait = waits.remove(at);
    tracing::warn!(agent = %wait.agent, "the agents did not all stop in time for the login change");
    let _gone = wait.reply.send(Ok(AgentsStopped::TimedOut));
}
