//! Text coalescing on the host clock: the 16 ms display batch, which shows text through
//! `apply_text_delta` with the stream's pending head as `pending_seq`, and the 250 ms durable
//! batch (or 8 KiB), which commits one `TextAppended` per message in first-delta order. The
//! display batch leads: text that comes after a quiet 16 ms is shown when it arrives, and text
//! inside the window is shown by one timer at the window's end.

use std::time::Duration;

use clauseo_protocol::{StreamId, TextAppended, ThreadId};
use clauseo_reducer::{TextDelta, ViewChange, apply_text_delta};

use super::super::Actor;
use super::super::streams::StreamSlot;
use super::out::Out;
use super::{Then, TimerKind, TurnTimer, end, lost, state};
use crate::coalesce::{DisplayText, TextIn};
use crate::error::Error;

/// One text delta of the live turn. A message's first delta closes the held batch first, so
/// its provisional order is the seq its `TextAppended` gets.
pub(super) fn on_delta(actor: &mut Actor, thread: ThreadId, delta: TextIn) -> Result<(), Error> {
    let closes = state(actor, thread)
        .is_some_and(|state| state.text.has_durable() && state.text.is_new_message(delta.message));
    if closes {
        Out::new(actor, thread, None)?.send(actor, Then::Nothing)?;
    }
    let now = actor.now().mono;
    let state = state(actor, thread).ok_or_else(|| lost("a thread"))?;
    let pushed = state.text.push(now, delta);
    if let Some(at) = pushed.display_at {
        arm(actor, thread, at.saturating_sub(now), TimerKind::Display);
    }
    if let Some(at) = pushed.durable_at {
        arm(actor, thread, at.saturating_sub(now), TimerKind::Durable);
    }
    // A full durable flush shows everything held, so the display flush below is not needed then.
    if pushed.full {
        Out::new(actor, thread, None)?.send(actor, Then::Nothing)?;
    } else if pushed.display_now {
        show_display(actor, thread, now);
        end::check_abort(actor, thread);
    }
    Ok(())
}

fn arm(actor: &Actor, thread: ThreadId, delay: Duration, kind: TimerKind) {
    // A timer that fires after its batch was flushed finds nothing held and does nothing.
    let _handle = actor.arm_timer(delay, TurnTimer { thread, kind });
}

/// The display batch's timer fired. A timer that a later flush has overtaken finds its window
/// still open and shows nothing; the timer of the current window does the flush.
pub(super) fn on_display_timer(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    if state(actor, thread).is_some_and(|state| state.text.display_due(now)) {
        show_display(actor, thread, now);
    }
    end::check_abort(actor, thread);
}

/// The display flush at `now` for a display timer or a leading delta. A failure is logged; the
/// text not shown stays held.
fn show_display(actor: &mut Actor, thread: ThreadId, now: Duration) {
    if let Err(error) = show(actor, thread, false, now) {
        tracing::error!(%thread, %error, "text could not be shown");
    }
}

/// The durable batch's timer fired.
pub(super) fn on_durable_timer(actor: &mut Actor, thread: ThreadId) {
    let held = state(actor, thread).is_some_and(|state| state.text.has_durable());
    if !held {
        return;
    }
    let sent = Out::new(actor, thread, None).and_then(|out| out.send(actor, Then::Nothing));
    if let Err(error) = sent {
        tracing::warn!(%thread, %error, "held text was not committed");
    }
}

/// The durable flush: shows what the display has not shown, then takes every held message as
/// a `TextAppended`, in first-delta order.
pub(super) fn take_durable(
    actor: &mut Actor,
    thread: ThreadId,
) -> Result<Vec<TextAppended>, Error> {
    let now = actor.now().mono;
    Ok(show(actor, thread, true, now)?.unwrap_or_default())
}

/// Runs the display flush (and with `durable` the durable one) of `thread` at `now`, publishing
/// the view changes once. A refused delta (`PendingSeqBehind`, a host bug) does not stop the
/// flush: the text still goes durable, and the turn ends with `TurnFailed { internal }`.
fn show(
    actor: &mut Actor,
    thread: ThreadId,
    durable: bool,
    now: Duration,
) -> Result<Option<Vec<TextAppended>>, Error> {
    let stream = StreamId::Thread(thread);
    let pending = actor
        .pipeline
        .pending_head(stream)
        .ok_or_else(|| lost("the pending head"))?;
    let Actor {
        threads,
        streams,
        encoder,
        ..
    } = actor;
    let text = &mut threads
        .get_mut(&thread)
        .ok_or_else(|| lost("a thread"))?
        .turn
        .text;
    let Some(StreamSlot::Loaded(loaded)) = streams.get_mut(&stream) else {
        return Err(lost("a loaded thread stream"));
    };
    let state = loaded
        .state
        .as_thread_mut()
        .ok_or_else(|| lost("a thread state"))?;
    let mut changes: Vec<ViewChange> = Vec::new();
    let mut refused: Option<clauseo_reducer::Error> = None;
    let mut apply = |shown: DisplayText<'_>| -> Result<(), Error> {
        let delta = TextDelta {
            turn: shown.turn,
            message: shown.message,
            kind: shown.kind,
            text: shown.text,
            pending_seq: pending,
        };
        match apply_text_delta(state, &delta) {
            Ok(applied) => {
                for anomaly in &applied.anomalies {
                    tracing::warn!(%stream, ?anomaly, "reducer anomaly");
                }
                changes.extend(applied.changes);
            }
            Err(error) => {
                tracing::error!(%stream, %error, "a text delta was refused");
                refused.get_or_insert(error);
            }
        }
        Ok(())
    };
    let appended = if durable {
        Some(text.flush_durable(now, &mut apply)?)
    } else {
        text.flush_display(now, &mut apply)?;
        None
    };
    if !changes.is_empty() {
        let encoded = loaded.view.publish(changes, encoder)?;
        for event in &encoded {
            loaded.views.retain_mut(|sub| sub.offer(event));
        }
    }
    if let Some(error) = refused {
        // Acted on after the commit being built is sent (`end::check_abort`).
        end::mark_failed(actor, thread, &Error::Reducer(error));
    }
    Ok(appended)
}
