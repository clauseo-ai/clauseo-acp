//! Turns, steps and the thread's status.

use clauseo_protocol::{
    DetachReason, EntryId, EventId, FilesChanged, LimitStopEntry, MarkerKind, Seq, StepEntry,
    StepUndone, ThreadStatus, TurnEnded, TurnFailed, TurnId, TurnQueued, TurnStoppedAtLimit,
    UserMessage, UserMessageEntry, UserMessageStatus, ViewEntry,
};

use super::{insert_marker, insert_new, not_yet, set, set_status, set_vec};
use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::order::insert_entry;
use crate::split::push_user_upsert;
use crate::state::{ThreadState, TurnFiles};

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

pub(super) fn detached(state: &mut ThreadState, reason: &DetachReason, out: &mut Applied) {
    let status = match reason {
        DetachReason::Exited { .. } | DetachReason::Unknown { .. } => ThreadStatus::AgentExited,
        DetachReason::Closed => ThreadStatus::Idle,
        DetachReason::Restarting | DetachReason::IdleStopped => {
            not_yet(out, "agent_session_detached");
            return;
        }
    };
    if state.running.is_some() {
        state.after_turn = Some(status);
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
        status: UserMessageStatus::Sent,
        edited: false,
        steer: None,
    };
    push_user_upsert(&mut out.changes, &entry);
    state.index.users.insert(event.message, seq);
    insert_entry(state, seq, ViewEntry::UserMessage(entry), out);
    if state.running.is_none() {
        state.turn = Some(event.turn);
        state.after_turn = None;
        set_status(state, "user_message", out, ThreadStatus::Starting);
    }
}

/// A turn waits. Behind another thread's writer it gives `Queued { behind }`, also as the turn's
/// second `TurnQueued`, and when no turn runs it becomes the thread's last turn, so its
/// `TurnEnded` before it starts (a cancel) settles the status. Behind its own thread it leaves
/// the status while a turn runs; when none runs, it settles (to `Idle` unless the lawyer is
/// asked).
pub(super) fn turn_queued(state: &mut ThreadState, event: &TurnQueued, out: &mut Applied) {
    let behind = event.behind();
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

pub(super) fn turn_started(state: &mut ThreadState, turn: TurnId, out: &mut Applied) {
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
pub(super) fn turn_failed(state: &mut ThreadState, event: &TurnFailed, out: &mut Applied) {
    if let Some(order) = state.index.steps.get(&event.turn).copied() {
        update_step(state, order, out, |step| {
            set(&mut step.failure, Some(event.code))
        });
        return;
    }
    state.failures.insert(event.turn, event.code);
}

/// The at-limit card, and `PausedAtLimit` when the turn is the thread's last one, even after
/// its `TurnEnded`.
pub(super) fn stopped_at_limit(
    state: &mut ThreadState,
    seq: Seq,
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
    };
    insert_new(state, seq, ViewEntry::LimitStop(entry), out);
    if state.turn == Some(event.turn) {
        let status = ThreadStatus::PausedAtLimit {
            resets_at: event.resets_at,
        };
        set_status(state, "turn_stopped_at_limit", out, status);
    }
}

/// Ends the running turn: its step, then the status. A `TurnEnded` of a turn that is not
/// running (one that never started) makes no step and marks its user message `Removed`
/// (0002); it leaves the status, but for `Starting` from that turn's own prompt, or `Queued` from
/// its wait for another thread's writer, while no turn runs, which settles (to `Idle` unless the
/// lawyer is asked). A second `TurnEnded` of a turn with a step is [`Anomaly::Duplicate`].
pub(super) fn turn_ended(state: &mut ThreadState, seq: Seq, event: &TurnEnded, out: &mut Applied) {
    if state.index.steps.contains_key(&event.turn) {
        out.anomalies.push(Anomaly::Duplicate {
            event: "turn_ended",
            target: EntryId::Step(event.turn),
        });
        return;
    }
    if state.running != Some(event.turn) {
        state.files.remove(&event.turn);
        state.failures.remove(&event.turn);
        remove_prompts(state, event.turn, out);
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
    state.running = None;
    let files = state.files.remove(&event.turn);
    let (files, skipped) = files.map_or_else(Default::default, |f| (f.files, f.skipped));
    let entry = StepEntry {
        order: seq,
        turn: event.turn,
        stop: event.stop,
        failure: state.failures.remove(&event.turn),
        files,
        skipped,
        checkpoint_after: event.checkpoint_after,
        undone: false,
        usage: event.usage,
    };
    state.index.steps.insert(event.turn, seq);
    insert_new(state, seq, ViewEntry::Step(entry), out);
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

/// Marks the user messages of `turn`, which ended without starting, `Removed`. It walks every
/// entry: a turn that never started is rare (a prompt removed or cancelled before its turn), and
/// an index from turns to prompts would be one more to keep.
fn remove_prompts(state: &mut ThreadState, turn: TurnId, out: &mut Applied) {
    for entry in state.entries.values_mut() {
        if let ViewEntry::UserMessage(prompt) = entry
            && prompt.turn == turn
            && set(&mut prompt.status, UserMessageStatus::Removed)
        {
            push_user_upsert(&mut out.changes, prompt);
        }
    }
}

pub(super) fn step_undone(
    state: &mut ThreadState,
    seq: Seq,
    event_id: EventId,
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
    insert_marker(state, seq, event_id, kind, out);
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
