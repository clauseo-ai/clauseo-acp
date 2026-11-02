//! The thread's queue as the view shows it: where each prompt that has not started stands
//! (`Queued`, `Sent`, `Removed`), a queued prompt's edit, where a steered message went, and the
//! tool calls a steer let carry on in the background.
//!
//! The prompts of turns that have neither started nor ended are indexed by turn
//! (`Index::unstarted`), so marking one never walks the transcript.

use clauseo_protocol::{
    AgentCapabilities, AgentId, AgentKind, EntryId, QueuedPromptEdited, SteerOutcome,
    SteerResolved, Timestamp, ToolCallStatus, TurnId, UserMessageStatus, ViewEntry,
};

use super::{set, set_vec, update_header};
use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::split::push_user_upsert;
use crate::state::ThreadState;

/// Whether a steer joins the running turn, before the agent's first session attaches: only a
/// Claude agent's does (claude-agent-acp), and its first attach confirms it or not.
pub(super) fn steer_joins_by_kind(agent: &AgentId) -> bool {
    AgentKind::of(agent) == Some(AgentKind::Claude)
}

/// Whether a steer joins the running turn with these capabilities: it needs both flags.
fn steer_joins(capabilities: &AgentCapabilities) -> bool {
    capabilities
        .steering
        .is_some_and(|steering| steering.native && steering.idle_fallback_to_host)
}

/// An attached session's capabilities say whether a steer joins the running turn.
pub(super) fn session_attached(
    state: &mut ThreadState,
    capabilities: &AgentCapabilities,
    out: &mut Applied,
) {
    let joins = steer_joins(capabilities);
    update_header(state, "agent_session_attached", out, |h| {
        set(&mut h.steer_joins_turn, joins)
    });
}

/// Sets the status of `turn`'s prompt while its turn has not started, emitting its `Upsert`
/// when it changed. A turn with no such prompt changes nothing.
pub(super) fn mark_prompt(
    state: &mut ThreadState,
    turn: TurnId,
    status: UserMessageStatus,
    out: &mut Applied,
) {
    let Some(order) = state.index.unstarted.get(&turn).copied() else {
        return;
    };
    if let Some(ViewEntry::UserMessage(prompt)) = state.entries.get_mut(&order)
        && set(&mut prompt.status, status)
    {
        push_user_upsert(&mut out.changes, prompt);
    }
}

/// `turn` started or ended: its prompt is no longer waiting, marked `status` one last time.
pub(super) fn settle_prompt(
    state: &mut ThreadState,
    turn: TurnId,
    status: UserMessageStatus,
    out: &mut Applied,
) {
    mark_prompt(state, turn, status, out);
    state.index.unstarted.remove(&turn);
}

/// A queued prompt's content is replaced and it shows as edited.
pub(super) fn queued_prompt_edited(
    state: &mut ThreadState,
    event: &QueuedPromptEdited,
    out: &mut Applied,
) {
    let target = EntryId::UserMessage(event.message);
    let found = state.index.users.get(&event.message).copied();
    let Some(ViewEntry::UserMessage(prompt)) =
        found.and_then(|order| state.entries.get_mut(&order))
    else {
        out.anomalies.push(Anomaly::UnknownReference {
            event: "queued_prompt_edited",
            target,
        });
        return;
    };
    let changed = set_vec(&mut prompt.content, &event.content) | set(&mut prompt.edited, true);
    if changed {
        push_user_upsert(&mut out.changes, prompt);
    }
}

/// Where a steered message went: its outcome, and the turn that answers it. A message that moved
/// to a new turn waits as that turn's prompt; each tool call set aside is marked background.
pub(super) fn steer_resolved(state: &mut ThreadState, event: &SteerResolved, out: &mut Applied) {
    let target = EntryId::UserMessage(event.message);
    let Some(order) = state.index.users.get(&event.message).copied() else {
        out.anomalies.push(Anomaly::UnknownReference {
            event: "steer_resolved",
            target,
        });
        return;
    };
    let answering = match &event.outcome {
        SteerOutcome::Injected { turn }
        | SteerOutcome::Requeued { turn }
        | SteerOutcome::SentNow { turn } => Some(*turn),
        SteerOutcome::Unknown { .. } => None,
    };
    let moved = match &event.outcome {
        SteerOutcome::Requeued { turn } | SteerOutcome::SentNow { turn } => Some(*turn),
        SteerOutcome::Injected { .. } | SteerOutcome::Unknown { .. } => None,
    };
    if let Some(ViewEntry::UserMessage(prompt)) = state.entries.get_mut(&order) {
        let mut changed = set(&mut prompt.steer, Some(event.outcome.clone()));
        if let Some(turn) = answering {
            changed |= set(&mut prompt.turn, turn);
        }
        if changed {
            push_user_upsert(&mut out.changes, prompt);
        }
    }
    if let Some(turn) = moved
        && state.running != Some(turn)
        && !state.index.steps.contains_key(&turn)
    {
        state.index.unstarted.insert(turn, order);
    }
    for tool_call in &event.aside {
        set_aside(state, *tool_call, event.message, out);
    }
}

/// Marks a tool call that had not finished as carrying on in the background.
fn set_aside(
    state: &mut ThreadState,
    tool_call: clauseo_protocol::ToolCallId,
    steer: clauseo_protocol::MessageId,
    out: &mut Applied,
) {
    let found = state.index.tool_calls.get(&tool_call).copied();
    let Some(ViewEntry::ToolCall(entry)) = found.and_then(|order| state.entries.get_mut(&order))
    else {
        out.anomalies.push(Anomaly::UnknownReference {
            event: "steer_resolved",
            target: EntryId::ToolCall(tool_call),
        });
        return;
    };
    if is_final(entry.status) || entry.background.is_some() {
        return;
    }
    entry.background = Some(clauseo_protocol::BackgroundMark {
        steer,
        finished_at: None,
    });
    out.changes
        .push(ViewChange::Upsert(Box::new(ViewEntry::ToolCall(
            entry.clone(),
        ))));
}

/// The finish time of a background tool call, when an update gives it a final status at `ts`.
/// Says whether it changed.
pub(super) fn background_finished(
    entry: &mut clauseo_protocol::ToolCallEntry,
    ts: Timestamp,
) -> bool {
    match entry.background.as_mut() {
        Some(mark) if mark.finished_at.is_none() && is_final(entry.status) => {
            mark.finished_at = Some(ts);
            true
        }
        Some(_) | None => false,
    }
}

/// Whether a tool call's status is one it never leaves.
fn is_final(status: ToolCallStatus) -> bool {
    match status {
        ToolCallStatus::Completed | ToolCallStatus::Failed | ToolCallStatus::Cancelled => true,
        ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => false,
    }
}

/// `queue_held` as the host sets it (the fold never does: the host keeps the flag in memory
/// and logs nothing for it). Returns the header's `Upsert` when it changed.
pub(crate) fn set_queue_held(state: &mut ThreadState, held: bool) -> Option<ViewChange> {
    let header = state.header.as_mut()?;
    if !set(&mut header.queue_held, held) {
        return None;
    }
    Some(ViewChange::Upsert(Box::new(ViewEntry::Header(
        header.clone(),
    ))))
}
