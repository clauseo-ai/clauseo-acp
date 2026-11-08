//! Unprotected sessions once launches are refused (R17 PR-E, the orchestrator's ruling): when
//! the computer's protection turns to a cause that refuses launches (`protection::refusal`:
//! blocked, another account, a full or owned pool, a broken set-up, an unknown cause, a removal,
//! or `sandbox.required`), every live agent launched unprotected (`Agent::tier` `None`) stops:
//! one between turns at once, one with a turn when that turn ends, as it would have ended (no
//! `TurnFailed` for it). Its next turn then needs a launch, which the refusal meets, so the
//! lawyer sees the refusal on their next send, never mid-answer. Stopping a turn mid-way would
//! lose the lawyer's work without protecting anything: its session was already running
//! unprotected, as the settings let it, when it started. The stop is the idle stop
//! (`agents::idle_stop`, detach `IdleStopped`); an agent launched under tier A or B keeps going.

use clauseo_protocol::ThreadId;

use super::super::Actor;
use super::agents::{self, Stage};
use super::{current, state};

/// Whether `thread`'s agent was launched unprotected and is not stopping already.
fn unprotected(actor: &mut Actor, thread: ThreadId) -> bool {
    state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .is_some_and(|agent| agent.tier.is_none() && agent.stage != Stage::Stopping)
}

/// Stops `thread`'s unprotected agent, logged with ids only.
fn stop(actor: &mut Actor, thread: ThreadId) {
    tracing::info!(%thread, "launches are refused now: an unprotected agent stops");
    agents::idle_stop(actor, thread);
}

/// The protection view changed: while launches are refused, each unprotected agent between
/// turns stops now; one with a turn stops when the turn ends ([`turn_ended`]).
pub(crate) fn protection_changed(actor: &mut Actor) {
    if super::super::protection::refusal(actor).is_none() {
        return;
    }
    let threads: Vec<ThreadId> = actor.threads.keys().copied().collect();
    for thread in threads {
        if current(actor, thread).is_none() && unprotected(actor, thread) {
            stop(actor, thread);
        }
    }
}

/// `thread`'s turn ended: while launches are refused, its unprotected agent stops.
pub(super) fn turn_ended(actor: &mut Actor, thread: ThreadId) {
    if super::super::protection::refusal(actor).is_some() && unprotected(actor, thread) {
        stop(actor, thread);
    }
}
