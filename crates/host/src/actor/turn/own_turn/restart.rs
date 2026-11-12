//! Start-up repair of an agent-started turn the host stopped before it started: its
//! `TurnQueued` (behind its own thread as it opened, or behind another thread's writer) is
//! durable, but no `TurnStarted` or `TurnEnded` is, so the view would show it `Starting` (or
//! `Queued`) forever. It has no prompt to queue again, and the agent that ran it is gone, so
//! start-up reconciliation (`reconcile.rs`) ends it `Cancelled` before it started: no step, and
//! the status settles. A turn with a prompt is not one: it stays queued (`queue::restore`).

use clauseo_protocol::{DomainEvent, Envelope, StopReason, ThreadStatus, TurnEnded, TurnId};
use clauseo_reducer::ThreadState;

use super::super::super::Actor;
use super::super::out::Out;
use crate::error::Error;

/// Whether the loaded `state` shows a turn on its way with none running: a wait an
/// agent-started turn may have left. The whole log then says whether it did ([`unstarted`]).
pub(in crate::actor::turn) fn may_wait(state: &ThreadState) -> bool {
    state.running().is_none()
        && matches!(
            state.status(),
            ThreadStatus::Starting | ThreadStatus::Queued { .. }
        )
}

/// The agent-started turn of `log` that waits with no start or end: the last turn with a
/// `TurnQueued` and no `UserMessage`, `TurnStarted` or `TurnEnded`.
fn unstarted(log: &[Envelope]) -> Option<TurnId> {
    let mut waiting = None;
    for envelope in log {
        let event = &envelope.event;
        let settled = if let DomainEvent::TurnQueued(queued) = event {
            waiting = Some(queued.turn());
            None
        } else if let DomainEvent::UserMessage(message) = event {
            Some(message.turn)
        } else if let DomainEvent::TurnStarted(started) = event {
            Some(started.turn)
        } else if let DomainEvent::TurnEnded(ended) = event {
            Some(ended.turn)
        } else {
            None
        };
        if settled.is_some() && settled == waiting {
            waiting = None;
        }
    }
    let prompted = |turn: TurnId| {
        log.iter().any(|envelope| {
            matches!(&envelope.event, DomainEvent::UserMessage(message) if message.turn == turn)
        })
    };
    waiting.filter(|turn| !prompted(*turn))
}

/// Pushes into `out` the `TurnEnded { Cancelled }` of the agent-started turn `log` leaves
/// waiting, if any.
pub(in crate::actor::turn) fn end_unstarted(
    actor: &mut Actor,
    out: &mut Out,
    log: &[Envelope],
) -> Result<(), Error> {
    let Some(turn) = unstarted(log) else {
        return Ok(());
    };
    tracing::info!(target: crate::log_file::LIFECYCLE, %turn, "an agent-started turn that never started ended at start-up");
    out.push(
        actor,
        TurnEnded {
            turn,
            stop: StopReason::Cancelled,
            usage: None,
            checkpoint_after: None,
        },
    )?;
    Ok(())
}
