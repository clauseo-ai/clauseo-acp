//! The idle stop (R20b; decision record 0052). An agent with nothing pending for the idle limit
//! (`HostConfig::with_idle_limit`, 300 s) is stopped with the shutdown ladder, and its session
//! detaches `IdleStopped` (`agents/ended.rs`). Its next question relaunches it through the launch
//! path, which resumes the stored session (`session.rs`).
//!
//! - Nothing pending ([`pending`]): no turn (current, or its end not durable yet), no queued
//!   prompt, no turn held for the sign-in, no pending permission or elicitation, no launch or
//!   load, no config or mode call in flight, and no case-law relay call in flight (the relay
//!   tells the actor when each forwarded message starts and ends, [`relay_call`]).
//! - Each agent's idle clock (`Mark::since`) starts when the last pending item ends: after every
//!   input the actor handles, [`tick`] clears it for an agent with something pending, and starts
//!   it at the host clock's time for one with nothing. Any event from the agent (an update or a
//!   request, in a turn or between turns) clears it too ([`active`]), so an agent still at
//!   work after its turn's end is stopped a full limit after its last event, not before.
//! - One deadline: the earliest idle agent's, armed on the host clock; none while no agent is
//!   idle. When it fires, [`on_due`] looks at every agent again, inside the actor, and stops only
//!   those that still have nothing pending and whose clock reached the limit: work handled in
//!   the same tick, before the fire (a `SendPrompt`), keeps its agent.
//! - The cap ([`make_room`]): on a machine of 8 GB or less (or of unknown memory), a launch that
//!   finds `live_agents` (2) agents live idle-stops the one idle longest with nothing pending
//!   first, if it has been idle at least `cap_min_idle` (60 s). When no live agent qualifies
//!   (every one is busy, or went idle less than `cap_min_idle` ago) the launch goes ahead and
//!   the host logs a warning.

use std::collections::BTreeSet;
use std::time::Duration;

use clauseo_protocol::ThreadId;
use futures::future::AbortHandle;

use super::super::Actor;
use super::super::commands::arriving_for;
use super::super::input::Input;
use super::agents::{self, Stage};
use super::{TurnState, host_stopping, queue, state};
use crate::config::{IdleStop, SessionKey};
use crate::connectors::RelayCall;
use crate::log_file::LIFECYCLE;

/// What keeps an agent from idling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pending {
    /// A current turn, or one whose end is not durable yet.
    Turn,
    /// A prompt waits in the queue (held or not), or a steer is out.
    Queued,
    /// A turn held across an expired sign-in (`sign_in.rs`, `queue/sign_in.rs`).
    Held,
    /// A permission request or an elicitation waits for the lawyer or the agent.
    Ask,
    /// The agent is launching or loading its session, or work waits for it.
    Launch,
    /// A config or mode call, or a case-law relay call, is in flight.
    Call,
    /// A command that gives the agent work (a prompt, a config option, a mode) waits for its
    /// receipt check: it arrived and has not run yet.
    Arriving,
}

/// An agent's idle state.
#[derive(Debug, Default)]
pub(super) struct Mark {
    /// When it last had nothing pending, on the host's monotonic clock; `None` while something
    /// is pending.
    pub(super) since: Option<Duration>,
    /// Its config and mode calls, and its case-law relay calls, in flight.
    calls: u32,
}

/// The idle stop's host-wide state.
#[derive(Debug, Default)]
pub(crate) struct IdleClock {
    config: IdleStop,
    /// The threads whose agent is live, from its launch until its `Ended`.
    live: BTreeSet<ThreadId>,
    /// The one deadline armed, if any.
    armed: Option<Armed>,
    generation: u64,
}

/// The deadline armed: when it falls on the monotonic clock, its generation and its handle.
#[derive(Debug)]
struct Armed {
    at: Duration,
    generation: u64,
    handle: AbortHandle,
}

impl IdleClock {
    pub(crate) fn new(config: IdleStop) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// When the deadline armed falls, for tests.
    #[cfg(test)]
    pub(super) fn armed_at(&self) -> Option<Duration> {
        self.armed.as_ref().map(|armed| armed.at)
    }
}

/// What keeps `state`'s agent from idling, or `None` when nothing does.
pub(super) fn pending(state: &TurnState) -> Option<Pending> {
    let launching = state.preparing.is_some()
        || state.agent.as_deref().is_some_and(|agent| {
            matches!(agent.stage, Stage::Initializing | Stage::Loading)
                || agent.preparing
                || !agent.waiting.is_empty()
                || !agent.after_stop.is_empty()
        });
    if launching {
        return Some(Pending::Launch);
    }
    if state.current.is_some() || state.closing.is_some() {
        return Some(Pending::Turn);
    }
    if state.stopped_for_sign_in.is_some() {
        return Some(Pending::Held);
    }
    if let Some(waits) = queue::waits(&state.queue) {
        return Some(waits);
    }
    let agent = state.agent.as_deref()?;
    let asks = !(agent.permissions.is_empty()
        && agent.elicitations.is_empty()
        && agent.url_elicitations.is_empty());
    if asks {
        return Some(Pending::Ask);
    }
    (agent.idle.calls > 0).then_some(Pending::Call)
}

/// What keeps `thread`'s agent from idling, its state's and a command arriving for it.
pub(super) fn pending_in(actor: &Actor, thread: ThreadId) -> Option<Pending> {
    let state = &actor.threads.get(&thread)?.turn;
    pending(state).or_else(|| arriving_for(actor, thread).then_some(Pending::Arriving))
}

/// Whether `thread`'s agent may be idle-stopped now: not stopping or closed already, its
/// connection up, nothing pending; `None` when it has no agent. A launching agent is kept by
/// [`pending`] (`Pending::Launch`) alone, so each item of the hard rule holds on its own.
fn stoppable(actor: &Actor, thread: ThreadId) -> Option<bool> {
    let agent = actor.threads.get(&thread)?.turn.agent.as_deref()?;
    let up = !matches!(agent.stage, Stage::Stopping | Stage::Closed) && !agent.link.closed();
    Some(up && pending_in(actor, thread).is_none())
}

/// `thread`'s agent was launched: it is live.
pub(super) fn launched(actor: &mut Actor, thread: ThreadId) {
    actor.turn_path.idle.live.insert(thread);
}

/// `thread`'s agent ended: it is live no more.
pub(super) fn ended(actor: &mut Actor, thread: ThreadId) {
    actor.turn_path.idle.live.remove(&thread);
}

/// A config or mode call to `thread`'s agent went out.
pub(super) fn call_started(actor: &mut Actor, thread: ThreadId) {
    if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) {
        agent.idle.calls += 1;
    }
}

/// A config or mode call to `thread`'s agent came back.
pub(super) fn call_ended(actor: &mut Actor, thread: ThreadId) {
    if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) {
        agent.idle.calls = agent.idle.calls.saturating_sub(1);
    }
}

/// A case-law relay message of `session` started or ended (`RelayEvent::Call`): it counts as
/// a call in flight of the thread's agent, in a turn or between turns. A stopped session's
/// relay (not the thread's current run) counts for nothing.
pub(crate) fn relay_call(actor: &mut Actor, session: SessionKey, call: RelayCall) {
    let Some(state) = state(actor, session.thread) else {
        return;
    };
    if state.run.as_ref().map(|run| run.key) != Some(session) {
        return;
    }
    let Some(agent) = state.agent.as_deref_mut() else {
        return;
    };
    match call {
        RelayCall::Started => agent.idle.calls += 1,
        RelayCall::Ended => agent.idle.calls = agent.idle.calls.saturating_sub(1),
    }
    agent.idle.since = None;
}

/// The agent sent an event (an update or a request), in a turn or between turns: its idle
/// clock starts again from the tick after this input.
pub(super) fn active(mark: &mut Mark) {
    mark.since = None;
}

/// After each input: every live agent's idle clock brought up to date at `now`, and the one
/// deadline armed for the earliest, or disarmed when no agent is idle (or the host is stopping).
pub(crate) fn tick(actor: &mut Actor) {
    let now = actor.now().mono;
    let earliest = if host_stopping(actor) {
        None
    } else {
        refresh(actor, now)
    };
    let config = actor.turn_path.idle.config;
    arm(
        actor,
        now,
        earliest.and_then(|since| config.deadline(since)),
    );
}

/// Starts or clears each live agent's idle clock at `now`; the earliest start among idle agents.
/// Forgets a live thread whose agent is gone.
fn refresh(actor: &mut Actor, now: Duration) -> Option<Duration> {
    let looked: Vec<(ThreadId, Option<bool>)> = actor
        .turn_path
        .idle
        .live
        .iter()
        .map(|thread| (*thread, stoppable(actor, *thread)))
        .collect();
    let mut earliest: Option<Duration> = None;
    for (thread, idle) in looked {
        let agent = actor
            .threads
            .get_mut(&thread)
            .and_then(|entry| entry.turn.agent.as_deref_mut());
        let (Some(idle), Some(agent)) = (idle, agent) else {
            actor.turn_path.idle.live.remove(&thread);
            continue;
        };
        if idle {
            let since = *agent.idle.since.get_or_insert(now);
            earliest = Some(earliest.map_or(since, |earliest| earliest.min(since)));
        } else {
            agent.idle.since = None;
        }
    }
    earliest
}

/// Arms the deadline for `at` (disarms it for `None`), unless it is armed for `at` already.
fn arm(actor: &mut Actor, now: Duration, at: Option<Duration>) {
    let clock = &mut actor.turn_path.idle;
    if at.is_some() && clock.armed.as_ref().map(|armed| armed.at) == at {
        return;
    }
    if let Some(armed) = clock.armed.take() {
        armed.handle.abort();
    }
    let Some(at) = at else {
        return;
    };
    clock.generation += 1;
    let generation = clock.generation;
    let handle = actor.post_after(at.saturating_sub(now), Input::IdleDeadline { generation });
    actor.turn_path.idle.armed = Some(Armed {
        at,
        generation,
        handle,
    });
}

/// The deadline armed with `generation` fired. Every agent is looked at again here, at the
/// fire's time: one that has something pending now (a prompt handled before this input) is not
/// stopped, and its clock is cleared. The next deadline is armed by the tick after this input.
pub(crate) fn on_due(actor: &mut Actor, generation: u64) {
    let idle = &mut actor.turn_path.idle;
    if idle
        .armed
        .as_ref()
        .is_none_or(|armed| armed.generation != generation)
    {
        // A deadline disarmed after it had already posted.
        return;
    }
    idle.armed = None;
    if host_stopping(actor) {
        return;
    }
    let now = actor.now().mono;
    refresh(actor, now);
    let config = actor.turn_path.idle.config;
    let due: Vec<(ThreadId, Duration)> = actor
        .turn_path
        .idle
        .live
        .iter()
        .filter_map(|thread| {
            let since = actor
                .threads
                .get(thread)?
                .turn
                .agent
                .as_deref()?
                .idle
                .since?;
            let reached = config.deadline(since).is_some_and(|at| at <= now);
            reached.then(|| (*thread, now.saturating_sub(since)))
        })
        .collect();
    for (thread, idle_for) in due {
        stop(actor, thread, idle_for, "the idle limit");
    }
}

/// Idle-stops `thread`'s agent, idle for `idle_for`, for `why`.
fn stop(actor: &mut Actor, thread: ThreadId, idle_for: Duration, why: &'static str) {
    tracing::info!(
        target: LIFECYCLE,
        %thread,
        idle_ms = u64::try_from(idle_for.as_millis()).unwrap_or(u64::MAX),
        why,
        "agent idle-stopped"
    );
    agents::idle_stop(actor, thread);
}

/// Before `starting`'s agent launches: when the cap applies and `live_agents` agents of other
/// threads are live (or launching), the one idle longest with nothing pending, looked at now, is
/// idle-stopped, if it has been idle at least `cap_min_idle`. When none qualifies the launch
/// goes ahead past the cap, with a warning (provisional until G3 item 3's 8 GB numbers).
pub(super) fn make_room(actor: &mut Actor, starting: ThreadId) {
    let config = actor.turn_path.idle.config;
    if !config.capped() {
        return;
    }
    let live = actor
        .threads
        .iter()
        .filter(|(thread, entry)| {
            let state = &entry.turn;
            **thread != starting
                && (state.preparing.is_some()
                    || state
                        .agent
                        .as_deref()
                        .is_some_and(|agent| agent.stage != Stage::Stopping))
        })
        .count();
    if live < config.live_agents {
        return;
    }
    let now = actor.now().mono;
    let victim = actor
        .turn_path
        .idle
        .live
        .iter()
        .filter(|thread| **thread != starting)
        .filter_map(|thread| {
            let since = actor
                .threads
                .get(thread)?
                .turn
                .agent
                .as_deref()?
                .idle
                .since?;
            let long_enough = now.saturating_sub(since) >= config.cap_min_idle;
            (long_enough && stoppable(actor, *thread)?).then_some((since, *thread))
        })
        .min();
    if let Some((since, thread)) = victim {
        stop(actor, thread, now.saturating_sub(since), "the live cap");
    } else {
        tracing::warn!(
            thread = %starting,
            live,
            cap = config.live_agents,
            "no live agent is idle long enough to stop; one more starts past the live-agents cap"
        );
    }
}
