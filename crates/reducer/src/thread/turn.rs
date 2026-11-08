//! Turns, steps and the thread's status.

use clauseo_protocol::{
    DetachReason, EntryId, EventId, FilesChanged, LimitStopEntry, MarkerKind, Seq, StepEntry,
    StepUndone, StopReason, ThreadStatus, Timestamp, TurnEnded, TurnFailed, TurnId, TurnQueued,
    TurnStoppedAtLimit, UserMessage, UserMessageEntry, UserMessageStatus, ViewEntry,
};

use super::queue::{mark_prompt, mark_put_back, settle_prompt};
use super::{insert_marker, insert_new, set, set_status, set_vec, update_header};
use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::order::insert_entry;
use crate::split::push_user_upsert;
use crate::state::{ThreadState, TurnFailure, TurnFiles};

/// The status while a turn runs: `NeedsYou` while the lawyer is asked, else `Running`.
pub(super) fn running_status(state: &ThreadState) -> ThreadStatus {
    if state.index.waiting > 0 {
        ThreadStatus::NeedsYou
    } else {
        ThreadStatus::Running
    }
}

/// The status once nothing waits for the lawyer: `Running` while a turn runs, else `Idle`.
pub(super) fn settled_status(state: &ThreadState) -> ThreadStatus {
    if state.index.waiting > 0 {
        ThreadStatus::NeedsYou
    } else if state.running.is_some() {
        ThreadStatus::Running
    } else {
        ThreadStatus::Idle
    }
}

/// The agent's session detached at `ts`. A thread idle-stopped while its next prompt waited
/// for the agent (the prompt came during the stop, so the thread is `Starting`) stays
/// `Starting`, and its resume starts now: `resuming_since` is the detach's `ts`. A restart
/// sets `restarting_since` and changes nothing else; any other detach ends the agent session,
/// so it expires pending denial cards and ends a restart.
pub(super) fn detached(
    state: &mut ThreadState,
    reason: &DetachReason,
    ts: Timestamp,
    out: &mut Applied,
) {
    let status = match reason {
        DetachReason::Exited { .. } | DetachReason::Unknown { .. } => ThreadStatus::AgentExited,
        DetachReason::Closed => ThreadStatus::Idle,
        DetachReason::IdleStopped => ThreadStatus::IdleStopped,
        // The host stops the agent to start it again with a wider policy (R18): the status,
        // a running turn and its cards stay, and the header shows the restart until the next
        // attach.
        DetachReason::Restarting => {
            if state
                .header
                .as_ref()
                .is_some_and(|h| h.restarting_since.is_none())
            {
                state.restarting = super::denials::restarted_turn(state);
            }
            update_header(state, "agent_session_detached", out, |h| {
                h.restarting_since.is_none() && set(&mut h.restarting_since, Some(ts))
            });
            return;
        }
    };
    super::denials::expire_pending(state, out);
    state.restarting = None;
    update_header(state, "agent_session_detached", out, |h| {
        set(&mut h.restarting_since, None)
    });
    if state.running.is_some() {
        state.after_turn = Some(status);
        return;
    }
    if status == ThreadStatus::IdleStopped && state.status() == ThreadStatus::Starting {
        update_header(state, "agent_session_detached", out, |h| {
            h.resuming_since.is_none() && set(&mut h.resuming_since, Some(ts))
        });
        return;
    }
    match state.status() {
        ThreadStatus::PausedAtLimit { .. } | ThreadStatus::Archived => {}
        ThreadStatus::Idle
        | ThreadStatus::Starting
        | ThreadStatus::Running
        | ThreadStatus::NeedsYou
        | ThreadStatus::Queued { .. }
        | ThreadStatus::AgentExited
        | ThreadStatus::IdleStopped => set_status(state, "agent_session_detached", out, status),
    }
}

pub(super) fn user_message(
    state: &mut ThreadState,
    seq: Seq,
    ts: Timestamp,
    event: &UserMessage,
    out: &mut Applied,
) {
    let target = EntryId::UserMessage(event.message);
    if state.index.users.contains_key(&event.message) {
        out.anomalies.push(Anomaly::Duplicate {
            event: "user_message",
            target,
        });
        return;
    }
    let entry = UserMessageEntry {
        order: seq,
        message: event.message,
        turn: event.turn,
        content: event.content.clone(),
        delivery: event.delivery,
        by: event.by,
        status: UserMessageStatus::Sent,
        edited: false,
        steer: None,
        at_head: false,
        ts,
    };
    push_user_upsert(&mut out.changes, &entry);
    state.index.users.insert(event.message, seq);
    // A steer into the running turn is not a prompt of its own turn.
    if state.running != Some(event.turn) {
        state.index.set_unstarted(event.turn, seq);
    }
    insert_entry(state, seq, ViewEntry::UserMessage(entry), out);
    if state.running.is_none() {
        // A prompt that moves an idle-stopped thread to `Starting` begins a resume, timed by
        // this event. `after_turn` can hold a stop only while a turn runs, so it is not
        // reachable here, but it counts the same.
        let resumes = state.status() == ThreadStatus::IdleStopped
            || state.after_turn == Some(ThreadStatus::IdleStopped);
        state.turn = Some(event.turn);
        state.after_turn = None;
        update_header(state, "user_message", out, |h| {
            let status = set(&mut h.status, ThreadStatus::Starting);
            let since = resumes && set(&mut h.resuming_since, Some(ts));
            status | since
        });
    }
}

/// A turn waits. Behind another thread's writer it gives `Queued { behind }`, also as the turn's
/// second `TurnQueued`, and when no turn runs it becomes the thread's last turn, so its
/// `TurnEnded` before it starts (a cancel) settles the status; its prompt has left the thread's
/// queue (`Sent`). Behind its own thread its prompt shows `Queued` (and `at_head` when the turn
/// was put back at the head, `queued_order.rs`), and it leaves the status
/// while a turn runs; when none runs, it settles (to `Idle` unless the lawyer is asked).
pub(super) fn turn_queued(state: &mut ThreadState, event: &TurnQueued, out: &mut Applied) {
    let behind = event.behind();
    let status = if behind == state.thread {
        UserMessageStatus::Queued
    } else {
        UserMessageStatus::Sent
    };
    if behind == state.thread && event.is_at_head() {
        mark_put_back(state, event.turn(), out);
    } else {
        mark_prompt(state, event.turn(), status, out);
    }
    if behind != state.thread {
        if state.running.is_none() {
            state.turn = Some(event.turn());
        }
        set_status(state, "turn_queued", out, ThreadStatus::Queued { behind });
        return;
    }
    if state.running.is_none() {
        let status = settled_status(state);
        set_status(state, "turn_queued", out, status);
    }
}

/// A turn starts: the cards still pending from earlier turns expire (`denials::expire_pending`).
pub(super) fn turn_started(state: &mut ThreadState, turn: TurnId, out: &mut Applied) {
    super::denials::expire_pending(state, out);
    settle_prompt(state, turn, UserMessageStatus::Sent, out);
    state.running = Some(turn);
    state.turn = Some(turn);
    state.after_turn = None;
    let status = running_status(state);
    set_status(state, "turn_started", out, status);
}

/// Keeps the turn's files for its step, or updates the step when the turn has ended.
pub(super) fn files_changed(state: &mut ThreadState, event: &FilesChanged, out: &mut Applied) {
    if let Some(order) = state.index.steps.get(&event.turn).copied() {
        update_step(state, order, out, |step| {
            set_vec(&mut step.files, &event.files) | set_vec(&mut step.skipped, &event.skipped)
        });
        return;
    }
    let files = TurnFiles {
        files: event.files.clone(),
        skipped: event.skipped.clone(),
    };
    state.files.insert(event.turn, files);
}

/// Keeps the failure's code for the turn's step, or updates the step when the turn has ended.
/// A failure of the restarted turn also ends the restart (`denials::end_restart`).
pub(super) fn turn_failed(state: &mut ThreadState, event: &TurnFailed, out: &mut Applied) {
    super::denials::end_restart(state, event.turn, "turn_failed", out);
    if let Some(order) = state.index.steps.get(&event.turn).copied() {
        update_step(state, order, out, |step| {
            set(&mut step.failure, Some(event.code)) | set(&mut step.failure_at, event.at.clone())
        });
        return;
    }
    let failure = TurnFailure {
        code: event.code,
        at: event.at.clone(),
    };
    state.failures.insert(event.turn, failure);
}

/// The at-limit card, and `PausedAtLimit` when the turn is the thread's last one, even after
/// its `TurnEnded`.
pub(super) fn stopped_at_limit(
    state: &mut ThreadState,
    seq: Seq,
    ts: Timestamp,
    event: &TurnStoppedAtLimit,
    out: &mut Applied,
) {
    if state.index.limit_stops.contains_key(&event.turn) {
        out.anomalies.push(Anomaly::Duplicate {
            event: "turn_stopped_at_limit",
            target: EntryId::LimitStop(event.turn),
        });
        return;
    }
    state.index.limit_stops.insert(event.turn, seq);
    let entry = LimitStopEntry {
        order: seq,
        turn: event.turn,
        window: event.window.clone(),
        resets_at: event.resets_at,
        reason: event.reason,
        ts,
    };
    insert_new(state, seq, ViewEntry::LimitStop(entry), out);
    if state.turn == Some(event.turn) {
        let status = ThreadStatus::PausedAtLimit {
            resets_at: event.resets_at,
        };
        set_status(state, "turn_stopped_at_limit", out, status);
    }
}

/// Ends the running turn: its step, then the status. A `TurnEnded { Failed }` of a turn that
/// never started (its launch or its before-turn checkpoint failed) ends the same way: its prompt
/// stays and it gets a step with its failure, and, as the thread's last turn while no turn runs,
/// it settles the status (0002). Any other `TurnEnded` of a turn that is not running (a prompt
/// removed or cancelled before its turn) makes no step and marks its user message `Removed`; it
/// leaves the status, but for `Starting` from that turn's own prompt, or `Queued` from its wait
/// for another thread's writer, while no turn runs, which settles (to `Idle` unless the lawyer
/// is asked). A second `TurnEnded` of a turn with a step is [`Anomaly::Duplicate`]. Pending
/// denial cards survive it, but the restarted turn's `TurnEnded` while a restart is shown ends
/// the restart and expires them (`denials::end_restart`).
pub(super) fn turn_ended(
    state: &mut ThreadState,
    seq: Seq,
    ts: Timestamp,
    event: &TurnEnded,
    out: &mut Applied,
) {
    if state.index.steps.contains_key(&event.turn) {
        out.anomalies.push(Anomaly::Duplicate {
            event: "turn_ended",
            target: EntryId::Step(event.turn),
        });
        return;
    }
    super::denials::end_restart(state, event.turn, "turn_ended", out);
    let running = state.running == Some(event.turn);
    if !running && event.stop != StopReason::Failed {
        state.files.remove(&event.turn);
        state.failures.remove(&event.turn);
        settle_prompt(state, event.turn, UserMessageStatus::Removed, out);
        if state.running.is_none()
            && state.turn == Some(event.turn)
            && matches!(
                state.status(),
                ThreadStatus::Starting | ThreadStatus::Queued { .. }
            )
        {
            let status = settled_status(state);
            set_status(state, "turn_ended", out, status);
        }
        return;
    }
    if running {
        state.running = None;
    } else {
        // A turn that failed before it started: its prompt was sent, and stays.
        settle_prompt(state, event.turn, UserMessageStatus::Sent, out);
    }
    let (failure, failure_at) = state
        .failures
        .remove(&event.turn)
        .map_or((None, None), |f| (Some(f.code), f.at));
    let files = state.files.remove(&event.turn);
    let (files, skipped) = files.map_or_else(Default::default, |f| (f.files, f.skipped));
    let entry = StepEntry {
        order: seq,
        turn: event.turn,
        stop: event.stop,
        failure,
        failure_at,
        files,
        skipped,
        checkpoint_after: event.checkpoint_after,
        undone: false,
        usage: event.usage,
        ts,
    };
    state.index.steps.insert(event.turn, seq);
    insert_new(state, seq, ViewEntry::Step(entry), out);
    if !running && (state.running.is_some() || state.turn != Some(event.turn)) {
        // Not the thread's last turn: the status belongs to another.
        return;
    }
    let after = state.after_turn.take();
    let status = match state.status() {
        paused @ ThreadStatus::PausedAtLimit { .. } => paused,
        ThreadStatus::Idle
        | ThreadStatus::Starting
        | ThreadStatus::Running
        | ThreadStatus::NeedsYou
        | ThreadStatus::Queued { .. }
        | ThreadStatus::AgentExited
        | ThreadStatus::IdleStopped
        | ThreadStatus::Archived => after.unwrap_or_else(|| settled_status(state)),
    };
    set_status(state, "turn_ended", out, status);
}

pub(super) fn step_undone(
    state: &mut ThreadState,
    seq: Seq,
    event_id: EventId,
    ts: Timestamp,
    event: &StepUndone,
    out: &mut Applied,
) {
    let Some(order) = state.index.steps.get(&event.turn).copied() else {
        out.anomalies.push(Anomaly::UnknownReference {
            event: "step_undone",
            target: EntryId::Step(event.turn),
        });
        return;
    };
    update_step(state, order, out, |step| set(&mut step.undone, true));
    let kind = MarkerKind::StepUndone {
        turn: event.turn,
        checkpoint: event.checkpoint,
        restored: event.restored.clone(),
        kept: event.kept.clone(),
    };
    insert_marker(state, seq, event_id, ts, kind, out);
}

fn update_step(
    state: &mut ThreadState,
    order: Seq,
    out: &mut Applied,
    change: impl FnOnce(&mut StepEntry) -> bool,
) {
    if let Some(ViewEntry::Step(step)) = state.entries.get_mut(&order)
        && change(step)
    {
        out.changes
            .push(ViewChange::Upsert(Box::new(ViewEntry::Step(step.clone()))));
    }
}
