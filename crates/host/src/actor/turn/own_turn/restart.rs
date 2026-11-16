//! Start-up repair of an agent-started turn the host stopped before it started: its
//! `TurnQueued` (behind its own thread as it opened, or behind another thread's writer) is
//! durable, but no `TurnStarted` or `TurnEnded` is, so the view would show it `Starting` (or
//! `Queued`) forever. It has no prompt to queue again, and the agent that ran it is gone, so
//! start-up reconciliation (`reconcile.rs`) ends it `Cancelled` before it started: no step, and
//! the status settles. A turn with a prompt is not one: it stays queued (`queue::restore`). Nor
//! is a requeued steer's turn, whose `UserMessage` names the turn it was sent into: only its
//! `SteerResolved` names its own. So a turn `queue::restore` puts back is never ended here.

use std::collections::BTreeSet;

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

/// The agent-started turns of `log` that wait with no start or end, in the order they opened:
/// each turn with a `TurnQueued` and no `UserMessage`, `TurnStarted` or `TurnEnded`, and that
/// `queue::restore` does not put back (a requeued or sent-now steer's turn). Every one counts,
/// not only the last wait: a prompt queued behind such a turn logs a `TurnQueued` of its own
/// after it. There is at most one, but none is assumed.
fn unstarted(log: &[Envelope]) -> Vec<TurnId> {
    let mut waiting: Vec<TurnId> = Vec::new();
    let mut settled: BTreeSet<TurnId> = BTreeSet::new();
    for envelope in log {
        let event = &envelope.event;
        if let DomainEvent::TurnQueued(queued) = event {
            if !waiting.contains(&queued.turn()) {
                waiting.push(queued.turn());
            }
        } else if let DomainEvent::UserMessage(message) = event {
            settled.insert(message.turn);
        } else if let DomainEvent::TurnStarted(started) = event {
            settled.insert(started.turn);
        } else if let DomainEvent::TurnEnded(ended) = event {
            settled.insert(ended.turn);
        }
    }
    let restored = super::super::queue::restores(log);
    waiting.retain(|turn| !settled.contains(turn) && !restored.contains(turn));
    waiting
}

/// Pushes into `out` the `TurnEnded { Cancelled }` of each agent-started turn `log` leaves
/// waiting. It goes before `queue::restore`'s events, so the prompts queued behind it come back
/// queued with the turn ahead of them already ended.
pub(in crate::actor::turn) fn end_unstarted(
    actor: &mut Actor,
    out: &mut Out,
    log: &[Envelope],
) -> Result<(), Error> {
    for turn in unstarted(log) {
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
    }
    Ok(())
}
