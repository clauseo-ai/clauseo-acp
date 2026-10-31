//! Store failures on the turn path ("Store failures" in the brief).
//!
//! A failed commit of a running turn cancels the turn through the Cancel path: the cancel
//! commit (logged again once the stream drains if the failure refused it), then the prompt's
//! answer or the cancel fallback, then steps 6 to 8 with `TurnFailed { code }`.
//!
//! A turn's last commit, the one holding its `TurnEnded`, leaves a closing record on the
//! thread until it returns `Ok`. When it (or a commit ahead of it) fails, the end is tried once
//! more as soon as the stream drains: expiries, cancelled tool calls, `TurnFailed`, `TurnEnded`.
//! If that fails too, the record stays and the thread refuses new prompts; start-up
//! reconciliation ends the turn at the next start, because the open turn is still the last
//! `UserMessage` in the log.

use clauseo_protocol::{
    DetachReason, ErrorCode, ExpiryReason, StopReason, StreamId, ThreadId, TurnFailed, TurnId,
};

use super::super::Actor;
use super::close::{Expiries, TurnClose};
use super::state::STORE_DETAIL;
use super::{Closing, Phase, Then, cancel, current, state, steps};
use crate::error::Error;
use crate::pipeline::Failure;

/// Whether `thread`'s stream is blocked by a failed commit; its unblock comes once every
/// commit in flight has returned.
pub(super) fn blocked(actor: &Actor, thread: ThreadId) -> bool {
    actor.pipeline.blocked(StreamId::Thread(thread)).is_some()
}

/// An agent-driven commit of `thread` failed: a running turn is cancelled, and once the
/// stream's commits have all returned, [`on_unblocked`] moves the turn on.
pub(super) fn on_store_failure(actor: &mut Actor, thread: ThreadId, failure: &Failure) {
    let Some(current) = current(actor, thread) else {
        tracing::warn!(%thread, error = %failure.error, "an agent-driven commit failed");
        return;
    };
    if current.store_failed.is_some() {
        return;
    }
    tracing::warn!(%thread, error = %failure.error, "a turn's commit failed; cancelling it");
    current.store_failed = Some(failure.error.code());
    if current.live() {
        cancel::cancel_running(actor, thread, None);
    }
}

/// `thread`'s stream drained after a failure; its sent view is the log's again.
///
/// A failed end is retried once. A turn whose prompt is in flight logs its cancel commit if the
/// failure refused it, and keeps waiting for the prompt's answer or the cancel fallback. A turn
/// whose checkpoint runs ends when the checkpoint answers. A turn that never started ends now.
pub(super) fn on_unblocked(actor: &mut Actor, thread: ThreadId) {
    retry_closing(actor, thread);
    let Some(current) = current(actor, thread) else {
        return;
    };
    let Some(code) = current.store_failed else {
        return;
    };
    let (turn, phase, cancelled) = (current.turn, current.phase, current.cancelled);
    match phase {
        Phase::Prompting | Phase::Stopping if cancelled => {
            // Logs nothing when the cancel commit was durable: the view no longer holds
            // anything for it to close.
            if let Err(error) = cancel::log_cancel(actor, thread, turn, None) {
                tracing::warn!(%thread, %error, "a cancel was not logged");
            }
        }
        Phase::Prompting | Phase::Stopping => cancel::cancel_running(actor, thread, None),
        Phase::Checkpointing | Phase::Ending => {}
        Phase::Accepting | Phase::Launching | Phase::Queued | Phase::Starting => {
            let failed = TurnFailed {
                turn,
                code,
                detail: Some(STORE_DETAIL.to_owned()),
            };
            steps::end_unstarted(actor, thread, turn, Some(failed), StopReason::Failed, None);
        }
    }
}

/// The commit ending `turn` was sent (`Ok`) or could not be (`Err`): keep the closing record
/// until it is durable. A send refused by a blocked stream is retried at the unblock; any
/// other refusal now.
pub(super) fn closing_sent(
    actor: &mut Actor,
    thread: ThreadId,
    close: &TurnClose,
    sent: Result<(), Error>,
) {
    let (turn, expiry, detach) = (close.turn, close.expiry(), close.detach.clone());
    let failed = sent.err().map(|error| {
        tracing::warn!(%thread, %error, "the end of a turn was not logged");
        error.code()
    });
    let Some(state) = state(actor, thread) else {
        return;
    };
    state.closing = Some(Closing {
        turn,
        failed,
        retried: false,
        expiry,
        detach,
    });
    if failed.is_some() && !blocked(actor, thread) {
        retry_closing(actor, thread);
    }
}

/// The commit ending `turn` is durable.
pub(super) fn on_closed(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if let Some(state) = state(actor, thread)
        && state
            .closing
            .as_ref()
            .is_some_and(|closing| closing.turn == turn)
    {
        state.closing = None;
    }
}

/// The commit ending `turn` failed; the unblock that follows retries it.
pub(super) fn on_closing_failed(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    failure: &Failure,
) {
    if let Some(closing) = state(actor, thread)
        .and_then(|state| state.closing.as_mut())
        .filter(|closing| closing.turn == turn)
    {
        closing.failed.get_or_insert(failure.error.code());
    }
}

/// The one retry of a failed end: expiries, cancelled tool calls, `TurnFailed { code }`,
/// `TurnEnded { Failed }`, and the detach last.
fn retry_closing(actor: &mut Actor, thread: ThreadId) {
    let Some(closing) = state(actor, thread)
        .and_then(|state| state.closing.as_mut())
        .filter(|closing| !closing.retried)
    else {
        return;
    };
    let Some(code) = closing.failed else {
        return;
    };
    closing.retried = true;
    let (turn, expiry, detach) = (closing.turn, closing.expiry, closing.detach.clone());
    let sent = retry_commit(actor, thread, turn, code, expiry, detach);
    if let Err(error) = sent {
        tracing::warn!(%thread, %error, "a turn's end failed twice; the next start ends it");
    }
}

fn retry_commit(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    code: ErrorCode,
    expiry: ExpiryReason,
    detach: Option<DetachReason>,
) -> Result<(), Error> {
    let mut close = TurnClose::new(turn, StopReason::Failed);
    close.expiries = Expiries::Pending(expiry);
    close.cancel_tools = true;
    close.failure = Some((code, Some("the commit ending the turn failed".to_owned())));
    close.detach = detach;
    close.send(actor, thread, Then::Closing(turn))
}
