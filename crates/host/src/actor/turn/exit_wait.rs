//! A turn whose agent's connection is gone waits, at its close, for the agent's exit report
//! (R17 PR-E): the after-turn checkpoint's answer is held until the agent's `Ended` comes, or for
//! [`EXIT_WAIT`] on the host clock, so the exit's reason joins the turn's end. Windows reports an
//! exit up to about a second after the pipes close; without the wait, the sandbox runner's
//! Escapee exit (`agents/ended.rs`) would come after `TurnEnded` and the turn would stay
//! `failed` / `agent.exited` instead of `left_safe_workspace`.
//!
//! - Only while the exit is due: the agent's connection closed, or it is stopping because it was
//!   (`agents::exit_due`). An agent that is alive, or one the host asked to stop, is not waited
//!   for: its exit can't be the Escapee's stop.
//! - Once per turn; never while the host shuts down, and shutdown ends every wait at once.
//! - Past the bound the turn ends as before; a later exit logs its detach after `TurnEnded`, and
//!   an Escapee one its `warn` (between turns: log only).

use std::time::Duration;

use clauseo_protocol::{ThreadId, TurnId};
use futures::future::AbortHandle;

use super::super::Actor;
use super::{TimerKind, TurnPhase, TurnTimer, agents, current, end, host_stopping};
use crate::error::Error;
use crate::matter_worker::TurnCheckpoint;

/// How long a turn's close waits for its agent's exit report.
pub(super) const EXIT_WAIT: Duration = Duration::from_secs(2);

/// A turn's wait for its agent's exit report.
#[derive(Debug, Default)]
pub(super) struct ExitWait {
    /// The wait was used: the end goes on whatever comes.
    done: bool,
    /// The bound, while armed.
    timer: Option<AbortHandle>,
    /// The after-turn checkpoint's answer, held until the exit report or the bound.
    held: Option<Result<TurnCheckpoint, Error>>,
}

#[cfg(test)]
impl ExitWait {
    /// Whether it holds the after-turn checkpoint's answer.
    pub(super) const fn holds_checkpoint(&self) -> bool {
        self.held.is_some()
    }
}

/// The after-turn checkpoint of `thread`'s `turn` answered with `result`: held, and `None`
/// returned, while the agent's exit is due and not reported; otherwise `result`, for the end
/// to go on now.
pub(super) fn hold(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    result: Result<TurnCheckpoint, Error>,
) -> Option<Result<TurnCheckpoint, Error>> {
    let due = !host_stopping(actor) && agents::exit_due(actor, thread);
    let Some(current) = current(actor, thread) else {
        return Some(result);
    };
    if current.exit_wait.done || !due {
        return Some(result);
    }
    current.exit_wait.done = true;
    current.exit_wait.held = Some(result);
    let timer = TurnTimer {
        thread,
        kind: TimerKind::ExitWait(turn),
    };
    let timer = actor.arm_timer(EXIT_WAIT, timer);
    match current_wait(actor, thread, Some(turn)) {
        Some(wait) => wait.timer = Some(timer),
        None => timer.abort(),
    }
    None
}

/// `thread`'s agent ended: a turn waiting for it ends now, its exit joined to the end.
pub(super) fn agent_ended(actor: &mut Actor, thread: ThreadId) {
    release(actor, thread, None);
}

/// The bound of `thread`'s `turn`'s wait is over: the end goes on without the exit report.
pub(super) fn on_timer(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    release(actor, thread, Some(turn));
}

/// Shutdown: every turn waiting for an exit report ends now.
pub(super) fn stop_all(actor: &mut Actor) {
    let waiting: Vec<ThreadId> = actor
        .threads
        .iter()
        .filter(|(_, entry)| {
            let current = entry.turn.current.as_deref();
            current.is_some_and(|current| current.exit_wait.held.is_some())
        })
        .map(|(thread, _)| *thread)
        .collect();
    for thread in waiting {
        release(actor, thread, None);
    }
}

/// The ending turn's wait (of `turn`, when given).
fn current_wait(
    actor: &mut Actor,
    thread: ThreadId,
    turn: Option<TurnId>,
) -> Option<&mut ExitWait> {
    current(actor, thread)
        .filter(|current| current.phase == TurnPhase::Ending)
        .filter(|current| turn.is_none_or(|turn| turn == current.turn))
        .map(|current| &mut current.exit_wait)
}

/// Ends the wait of `thread`'s turn (of `turn`, when given), if it holds the checkpoint's
/// answer: the end goes on with it.
fn release(actor: &mut Actor, thread: ThreadId, turn: Option<TurnId>) {
    let Some(wait) = current_wait(actor, thread, turn) else {
        return;
    };
    let Some(result) = wait.held.take() else {
        return;
    };
    if let Some(timer) = wait.timer.take() {
        timer.abort();
    }
    let Some(turn) = current(actor, thread).map(|current| current.turn) else {
        return;
    };
    end::on_after_turn(actor, thread, turn, result);
}
