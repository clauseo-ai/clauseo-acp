//! The start's bound: an agent silent for [`START_WAIT`] while it starts (`initialize` through
//! `session/new`, `session/load` or `session/resume`, and a Codex or Claude session's checks
//! before its attach) has failed to start; each update or answer it sends counts the wait from
//! then, and time the computer spent asleep is left out ([`TICK`], [`SLACK`]). It is stopped
//! through the shutdown ladder and what waits for it fails `agent.launch_failed`, its question
//! kept; the offline probe may make that `agent.start_offline` or `agent.start_unreachable`
//! (`offline/start.rs`). Without the bound a start that never answers left the turn starting
//! until the lawyer pressed Stop.
//!
//! The limit: the slowest start measured in the repo is Codex's cold start on a Windows VM,
//! 3.7 s (`agents/src/signin/codex.rs`); claude-agent-acp 0.89.1's session-start phases total
//! milliseconds (`start_failure.rs`). A `session/load` replays the whole conversation and a
//! cold Windows start behind a scanner can be many times slower, so the bound is generous: as
//! long as the folder preflight waits for a lawyer's answer (`folder_access::PREFLIGHT_WAIT`).

use std::time::Duration;

use clauseo_protocol::{ErrorCode, ThreadId};

use super::super::{TimerKind, TurnTimer, state};
use super::{Stage, launch_failed};
use crate::actor::Actor;
use crate::error::Error;

/// How long a starting agent may stay silent: each update or answer it sends while it starts
/// counts the wait from then, so a long replay is never cut off, only an agent gone quiet.
pub(crate) const START_WAIT: Duration = Duration::from_secs(120);

/// The detail of a start whose agent was silent for [`START_WAIT`], for logs;
/// `offline/start.rs` reads it to ask the probe.
pub(crate) const START_TIMED_OUT: &str = "the agent said nothing for 120 s while it was starting";

/// How often the bound looks while the agent starts: often enough that a sleep shows as a late
/// look, whenever it falls.
pub(crate) const TICK: Duration = Duration::from_secs(5);

/// A look more than this late was held up by the computer's sleep, not by the agent: Windows'
/// clock runs on while the computer sleeps (a Mac's stops), so the time asleep is left out of
/// the agent's silence. Without it a lid closed mid-start failed the start on waking, or cut
/// the agent's waking time short when the sleep ended before the bound was due.
const SLACK: Duration = Duration::from_secs(5);

/// A starting agent's bound: when it was last heard from and when its timer is due, on the
/// host's monotonic clock. Kept in `TurnPath::start_bounds` by agent id until its timer fires
/// for an agent no longer starting.
#[derive(Debug, Clone, Copy)]
pub(in super::super) struct Bound {
    heard: Duration,
    due: Duration,
}

/// A new agent's id for `thread`'s launch, its bound armed.
pub(in super::super) fn launched(actor: &mut Actor, thread: ThreadId) -> u64 {
    let id = super::next_agent_id();
    let now = actor.now().mono;
    let due = now.saturating_add(TICK);
    actor
        .turn_path
        .start_bounds
        .insert(id, Bound { heard: now, due });
    arm_for(actor, thread, id, TICK);
    id
}

/// Arms `thread`'s agent `id`'s timer `delay` from now.
fn arm_for(actor: &Actor, thread: ThreadId, id: u64, delay: Duration) {
    #[cfg(test)]
    if actor.turn_path.no_start_bound {
        return;
    }
    let timer = TurnTimer {
        thread,
        kind: TimerKind::Start(id),
    };
    let _handle = actor.arm_timer(delay, timer);
}

/// `thread`'s agent said something (an update, a call's answer; `id` the agent a call went to):
/// while it is starting, the bound counts from now. A long `session/load` replay keeps it alive.
pub(in super::super) fn heard(actor: &mut Actor, thread: ThreadId, id: Option<u64>) {
    let now = actor.now().mono;
    let Some(agent) = starting(actor, thread).map(|agent| agent.id) else {
        return;
    };
    if id.is_none_or(|id| id == agent)
        && let Some(bound) = actor.turn_path.start_bounds.get_mut(&agent)
    {
        bound.heard = now;
    }
}

/// `thread`'s agent while it is starting.
fn starting(actor: &mut Actor, thread: ThreadId) -> Option<&mut super::Agent> {
    state(actor, thread)
        .and_then(|state| state.agent.as_deref_mut())
        .filter(|agent| matches!(agent.stage, Stage::Initializing | Stage::Loading))
}

/// The timer of `thread`'s agent `id` fired. An agent attached, stopping or gone meanwhile is
/// left as it is. A look held up by sleep leaves the time asleep out of the silence; an agent
/// silent for [`START_WAIT`] has failed to start; else the next look is due in [`TICK`], or at
/// the bound when that comes first.
pub(in super::super) fn on_timer(actor: &mut Actor, thread: ThreadId, id: u64) {
    let now = actor.now().mono;
    let still = starting(actor, thread).is_some_and(|agent| agent.id == id);
    let bound = actor.turn_path.start_bounds.get_mut(&id);
    let (true, Some(bound)) = (still, bound) else {
        actor.turn_path.start_bounds.remove(&id);
        return;
    };
    let late = now.saturating_sub(bound.due);
    if late > SLACK {
        tracing::info!(%thread, late_s = late.as_secs(), "the start's bound leaves out time asleep");
        bound.heard = bound.heard.saturating_add(late).min(now);
    }
    let silent = now.saturating_sub(bound.heard);
    if silent < START_WAIT {
        let wait = TICK.min(START_WAIT.saturating_sub(silent));
        bound.due = now.saturating_add(wait);
        arm_for(actor, thread, id, wait);
        return;
    }
    actor.turn_path.start_bounds.remove(&id);
    tracing::warn!(%thread, wait_s = START_WAIT.as_secs(), "the agent did not finish starting");
    launch_failed(
        actor,
        thread,
        &Error::refused(ErrorCode::AgentLaunchFailed, START_TIMED_OUT),
    );
}
