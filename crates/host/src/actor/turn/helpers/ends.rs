//! How a conversation's helpers end when their run is cut short: a Stop, a turn's close (by
//! itself, a failure, the agent gone), a detach, a failed write's retry, and a past run's
//! helpers left open in the log (`helpers.rs` for the rest).

use clauseo_protocol::{
    HelperId, HelperOutcome, StopReason, StreamId, TextOrBlob, ThreadId, ViewEntry,
};
use clauseo_reducer::ThreadState;

use super::super::super::Actor;
use super::super::close::{Expiries, TurnClose};
use super::super::out::Out;
use super::super::{Then, state};
use super::existing;
use crate::build;
use crate::error::Error;
use crate::log_file::Cause;

/// Pushes `HelperEnded { outcome }` for every open helper of the thread into `out`, and closes
/// them. A helper whose own end (its handback, with its report) a failed write lost gets that
/// end again instead. The kept end stays until the view shows it or a turn's end is durable,
/// so a retry whose write fails too writes it once more. A Codex helper whose start still waits
/// for its model starts first, in the same commit (`codex::start_waiting`), and a Codex helper's
/// end carries the report it had written so far (`codex::State::take_report`), so a stopped
/// helper keeps it.
fn end_open(
    actor: &mut Actor,
    thread: ThreadId,
    outcome: HelperOutcome,
    out: &mut Out,
) -> Result<(), Error> {
    end_open_of(actor, thread, outcome, out, Which::All)
}

/// Which open helpers [`end_open_of`] ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    All,
    /// Not those working on in the background.
    Foreground,
}

/// [`end_open`] for the open helpers `which` picks.
fn end_open_of(
    actor: &mut Actor,
    thread: ThreadId,
    outcome: HelperOutcome,
    out: &mut Out,
    which: Which,
) -> Result<(), Error> {
    super::codex::start_waiting(actor, thread, out)?;
    let Some(helpers) = existing(actor, thread) else {
        return Ok(());
    };
    let mut ended: Vec<(HelperId, HelperOutcome, Option<String>)> = Vec::new();
    for (acp_id, known) in &mut helpers.known {
        if !known.open || (which == Which::Foreground && known.background) {
            continue;
        }
        known.open = false;
        helpers.last_words.remove(&known.id);
        // A Codex helper's report so far is its summary (none for any other helper).
        let report = helpers.codex.take_report(acp_id);
        let (outcome, summary) = helpers
            .sent_ends
            .get(&known.id)
            .cloned()
            .unwrap_or((outcome, report));
        ended.push((known.id, outcome, summary));
    }
    helpers.forget_ended_steps();
    // In start order, so the log reads as the helpers ran.
    ended.sort_unstable_by_key(|(helper, _, _)| *helper);
    for (helper, outcome, summary) in ended {
        let summary = summary.map(TextOrBlob::Inline);
        let built = build::helper_ended(helper, outcome, summary);
        out.push(actor, built.value)?;
        out.draft.blobs(built.blobs);
    }
    Ok(())
}

/// The thread's stream drained after a failed write (`failure.rs`): a helper this run marked
/// ended is open again when the log does not hold its end, so the retried cancel or close ends
/// it (with the end it had sent, when it was its own). An own end the log holds is forgotten.
/// Reads the view only for a thread that has helpers. The caller makes sure no commit of the
/// thread is in flight, so the view is the log.
pub(in crate::actor::turn) fn reopen_from_view(actor: &mut Actor, thread: ThreadId) {
    if existing(actor, thread).is_none() {
        return;
    }
    let open: Vec<HelperId> = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .map(open_in_view)
        .unwrap_or_default();
    let Some(helpers) = existing(actor, thread) else {
        return;
    };
    for known in helpers.known.values_mut() {
        if !known.open && open.contains(&known.id) {
            known.open = true;
        }
    }
    helpers.sent_ends.retain(|helper, _| open.contains(helper));
}

/// The helpers `state` holds started and not ended.
fn open_in_view(state: &ThreadState) -> Vec<HelperId> {
    if state.open_helpers() == 0 {
        return Vec::new();
    }
    state
        .entries()
        .values()
        .filter_map(|entry| {
            if let ViewEntry::Helper(helper) = entry
                && helper.outcome.is_none()
            {
                Some(helper.helper)
            } else {
                None
            }
        })
        .collect()
}

/// A cancel (the lawyer's Stop, a send-now, a failure, the host stopping): every open helper
/// of the conversation ends `Stopped`. Claude's engine stops its helpers on a cancel, foreground
/// and background alike (recorded: no model or tool call of a helper after a cancel), so none
/// outlives it. codex-acp's cancel stops only the lead (`codex.rs`): a Codex helper still open
/// marks the agent to stop once the turn has closed (`idle::stop_for_helpers`).
pub(in crate::actor::turn) fn on_cancel(
    actor: &mut Actor,
    thread: ThreadId,
    out: &mut Out,
) -> Result<(), Error> {
    if existing(actor, thread).is_some_and(|helpers| super::codex::any_open(helpers)) {
        super::super::idle::stop_for_helpers(actor, thread);
    }
    end_open(actor, thread, HelperOutcome::Stopped, out)
}

/// What a turn that ends by itself (a stop reason from the agent, no failure) writes for a
/// foreground helper still open: it never handed back a report. The one home of that word.
const ENDED_BY_ITSELF: HelperOutcome = HelperOutcome::Failed;

/// The close of a turn (`close.rs`): with the agent gone (its detach) every open helper ends
/// `Stopped`; a failed turn's (the agent exited, its connection was lost) and a cancelled
/// turn's left `Stopped` too. A turn that ended by itself ends its open foreground helpers
/// [`ENDED_BY_ITSELF`] and leaves its background ones open: Claude Code ends the prompt while
/// they work on, and they end later on their handback (in the agent's own turn, or with none
/// live), on a Stop, on a detach, or at a load with no agent (`settle_loaded`). A turn that is
/// not the one the log has running (removed from the queue, never started) leaves them open.
/// Start-up's repair of a turn a crash or a quit left open (its expiries listed from the log)
/// ends `Stopped` every helper the log leaves open, so no card of the last run stays running.
pub(in crate::actor::turn) fn on_close(
    actor: &mut Actor,
    out: &mut Out,
    close: &TurnClose,
) -> Result<(), Error> {
    let thread = out.thread();
    if matches!(close.expiries, Expiries::Listed { .. }) {
        return end_logged_open(actor, thread, out);
    }
    if existing(actor, thread).is_none() {
        return Ok(());
    }
    let turn = close.turn;
    // Only the close of the turn the log has running closes helpers: a queued turn removed, or
    // one that never started, leaves another turn's helpers as they are. Read from the view, so
    // a close retried after a failed write (its turn no longer current) closes them too.
    let prompted = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .is_some_and(|state| state.running() == Some(turn));
    // The engine gone (its detach, or a failure: the agent exited, the connection was lost)
    // and a cancel stop every open helper, whoever cut the turn short.
    let stopped =
        close.detach.is_some() || close.failure.is_some() || close.stop == StopReason::Cancelled;
    let (outcome, which) = if close.detach.is_some() {
        (HelperOutcome::Stopped, Which::All)
    } else if !prompted {
        return Ok(());
    } else if stopped {
        (HelperOutcome::Stopped, Which::All)
    } else {
        (ENDED_BY_ITSELF, Which::Foreground)
    };
    end_open_of(actor, thread, outcome, out, which)
}

/// `HelperEnded { Stopped }` into `out` for every helper the thread's view holds with no end:
/// the helpers of a run that stopped without ending them. One walk of the view, at start-up only.
fn end_logged_open(actor: &mut Actor, thread: ThreadId, out: &mut Out) -> Result<(), Error> {
    let open: Vec<HelperId> = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .map(open_in_view)
        .unwrap_or_default();
    for helper in open {
        let ended = build::helper_ended(helper, HelperOutcome::Stopped, None).value;
        out.push(actor, ended)?;
    }
    Ok(())
}

/// A thread's stream loaded: the helpers its log leaves open end `Stopped` when no agent of this
/// run can still be running them (the run that started them crashed or quit, its agent gone).
/// A log that ends inside a turn is left to start-up's repair of that turn, which ends them; a
/// thread with a live agent, a current turn or a close in progress is left to those paths.
/// Start-up loads every thread while it reconciles, so this runs then too. Costs one count read
/// for a thread without open helpers; a second load finds none open.
pub(crate) fn settle_loaded(actor: &mut Actor, thread: ThreadId) {
    let stream = StreamId::Thread(thread);
    let (open, turn_open) = actor
        .loaded(stream)
        .and_then(|loaded| loaded.state.as_thread())
        .map_or((0, false), |state| {
            (state.open_helpers(), state.running().is_some())
        });
    if open == 0 || turn_open || actor.pipeline.in_flight(stream) {
        return;
    }
    let busy = state(actor, thread).is_none_or(|state| {
        state.agent.is_some() || state.current.is_some() || state.closing.is_some()
    });
    if busy {
        return;
    }
    let sent = Out::new(actor, thread, None).and_then(|mut out| {
        end_logged_open(actor, thread, &mut out)?;
        out.send(actor, Then::Nothing)
    });
    if let Err(error) = sent {
        tracing::warn!(%thread, error = %Cause(&error), "a past run's helpers were not ended");
    }
}

/// The agent session ended outside a turn: every open helper ends `Stopped`, and the thread
/// forgets its helpers, whose ids belonged to that session.
pub(in crate::actor::turn) fn on_detach(
    actor: &mut Actor,
    thread: ThreadId,
    out: &mut Out,
) -> Result<(), Error> {
    on_switch_detach(actor, thread, out)?;
    forget(actor, thread);
    Ok(())
}

/// A switch's commit logs the old session's detach (`switch/commit.rs`): every open helper ends
/// `Stopped` in that commit, as at any detach. The thread keeps its helpers until the commit is
/// durable ([`forget`]), so a refused commit leaves them for the drain's [`reopen_from_view`],
/// and the `Closed` detach logged after it ends them.
pub(in crate::actor::turn) fn on_switch_detach(
    actor: &mut Actor,
    thread: ThreadId,
    out: &mut Out,
) -> Result<(), Error> {
    end_open(actor, thread, HelperOutcome::Stopped, out)
}

/// The thread forgets its helpers: their session is gone, and the next one's ids are its own.
pub(in crate::actor::turn) fn forget(actor: &mut Actor, thread: ThreadId) {
    if let Some(state) = state(actor, thread) {
        state.helpers = None;
    }
}
