//! The late window of a cancelled agent-started turn. No prompt answers such a turn, so nothing
//! tells the host when the agent has stopped: the output it sends after `session/cancel` (late
//! edits, tool calls, text) would otherwise open a turn of its own again or, after a Send now,
//! be taken for the next prompt's. A cancel of an agent-started turn, in any phase, opens the
//! window: the output waiting for a turn is dropped, and every answer, thought or tool update
//! with no prompt in flight is dropped while it lasts, counted only. It closes on the agent's
//! stop signal (the cost-bearing usage update its SDK's `result` sends) or after [`QUIET`] with
//! no such output, at the latest [`CAP`] after the cancel. While it is open no agent-started turn
//! opens, and the next prompt starts only once it closes or [`QUIET`] after the cancel.

use std::time::Duration;

use clauseo_acp::AgentUpdate;
use clauseo_protocol::ThreadId;
use futures::future::AbortHandle;

use super::super::super::Actor;
use super::super::{TimerKind, TurnTimer, queue, state};
use crate::log_file::LIFECYCLE;

/// How long the agent must send nothing after a cancel of its own turn for it to count as
/// stopped; the longest a prompt sent now waits for that.
pub(in crate::actor::turn) const QUIET: Duration = Duration::from_secs(3);
/// The longest the window stays open while the agent goes on sending.
pub(in crate::actor::turn) const CAP: Duration = Duration::from_secs(30);

/// An open late window.
#[derive(Debug)]
pub(crate) struct Late {
    /// When the cancel opened it, on the monotonic clock.
    since: Duration,
    /// The last output it dropped (or `since`), on the monotonic clock.
    last: Duration,
    /// How many updates and requests it dropped.
    dropped: usize,
    /// The timer that closes it, or lets the next prompt start.
    timer: Option<AbortHandle>,
}

/// A cancel of `thread`'s agent-started turn opens the window, or moves an open one on; the
/// output waiting for a turn is that turn's and is dropped.
pub(super) fn open(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    let Some(held) = state(actor, thread).map(|state| &mut state.own_held) else {
        return;
    };
    let waiting = std::mem::take(&mut held.updates).len();
    if let Some(late) = held.late.as_mut() {
        late.last = now;
        late.dropped += waiting;
        return;
    }
    held.late = Some(Late {
        since: now,
        last: now,
        dropped: waiting,
        timer: None,
    });
    arm(actor, thread, QUIET);
}

/// Whether `thread`'s late window is open.
pub(super) fn is_open(actor: &mut Actor, thread: ThreadId) -> bool {
    state(actor, thread).is_some_and(|state| state.own_held.late.is_some())
}

/// Whether the window holds the next prompt back: it is open, and [`QUIET`] has not passed
/// since the cancel.
pub(in crate::actor::turn) fn holds_starts(actor: &mut Actor, thread: ThreadId) -> bool {
    let now = actor.now().mono;
    state(actor, thread)
        .and_then(|state| state.own_held.late.as_ref())
        .is_some_and(|late| now.saturating_sub(late.since) < QUIET)
}

/// Whether an `Idle` update is turn output the open window drops: answer or thought text, a
/// tool call or its update, a plan, or the stop signal, which closes the window. Anything else
/// (modes, commands, a title, usage with no cost) is the session's and goes on.
pub(super) fn drops(actor: &mut Actor, thread: ThreadId, update: &AgentUpdate) -> bool {
    let output = match update {
        AgentUpdate::MessageChunk { role, .. } => *role != clauseo_acp::MessageRole::User,
        AgentUpdate::ToolCall(_) | AgentUpdate::ToolCallUpdate { .. } | AgentUpdate::Plan(_) => {
            true
        }
        AgentUpdate::Usage { cost, .. } => cost.is_some(),
        AgentUpdate::Commands(_)
        | AgentUpdate::CurrentMode { .. }
        | AgentUpdate::ConfigOptions(_)
        | AgentUpdate::SessionTitle { .. }
        | AgentUpdate::Notice { .. }
        | AgentUpdate::CompactionStarted { .. }
        | AgentUpdate::CompactionEnded { .. }
        | AgentUpdate::CompactionSummaryChunk { .. } => false,
    };
    if !output || !note(actor, thread) {
        return false;
    }
    if super::ends_turn(update) {
        close(actor, thread, "signal");
    }
    true
}

/// Counts one dropped update or request in the open window; false when none is open.
pub(super) fn note(actor: &mut Actor, thread: ThreadId) -> bool {
    let now = actor.now().mono;
    let Some(late) = state(actor, thread).and_then(|state| state.own_held.late.as_mut()) else {
        return false;
    };
    late.last = now;
    late.dropped += 1;
    true
}

/// The window's timer fired: it closes after [`QUIET`] with no output or at [`CAP`]; else it
/// waits on, and the next prompt, held back only for [`QUIET`] after the cancel, may start.
pub(in crate::actor::turn) fn on_timer(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    let Some((since, last)) = state(actor, thread)
        .and_then(|state| state.own_held.late.as_ref())
        .map(|late| (late.since, late.last))
    else {
        return;
    };
    let quiet_at = last.saturating_add(QUIET);
    let cap_at = since.saturating_add(CAP);
    if now >= quiet_at {
        close(actor, thread, "quiet");
    } else if now >= cap_at {
        close(actor, thread, "cap");
    } else {
        arm(actor, thread, quiet_at.min(cap_at).saturating_sub(now));
        queue::move_on(actor, thread);
    }
}

/// The window closes: a lifecycle line counts what it dropped, and the queue moves on.
fn close(actor: &mut Actor, thread: ThreadId, by: &'static str) {
    let Some(late) = state(actor, thread).and_then(|state| state.own_held.late.take()) else {
        return;
    };
    if let Some(timer) = late.timer {
        timer.abort();
    }
    let dropped = late.dropped;
    tracing::info!(target: LIFECYCLE, %thread, dropped, by, "an agent-started turn's late output was dropped");
    queue::move_on(actor, thread);
    super::open_if_idle(actor, thread);
}

/// What `thread`'s open late window dropped so far, or `None` when none is open.
#[cfg(test)]
pub(in crate::actor::turn) fn dropped(actor: &mut Actor, thread: ThreadId) -> Option<usize> {
    state(actor, thread)
        .and_then(|state| state.own_held.late.as_ref())
        .map(|late| late.dropped)
}

fn arm(actor: &mut Actor, thread: ThreadId, after: Duration) {
    let timer = actor.arm_timer(
        after,
        TurnTimer {
            thread,
            kind: TimerKind::OwnLate,
        },
    );
    match state(actor, thread).and_then(|state| state.own_held.late.as_mut()) {
        Some(late) => {
            if let Some(old) = late.timer.replace(timer) {
                old.abort();
            }
        }
        None => timer.abort(),
    }
}
