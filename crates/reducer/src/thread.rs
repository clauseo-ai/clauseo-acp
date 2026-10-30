//! The fold of a thread's events other than text: the header, tool calls, notices and markers
//! here; turns, steps and the status in `thread/turn.rs`; permissions and elicitations in
//! `thread/asks.rs`.
//!
//! Each field is compared as it is set, and an `Upsert` is emitted only when something changed;
//! no old entry is cloned to compare afterwards.

mod asks;
mod turn;

use clauseo_protocol::{
    AgentContentEntry, CompactionChanged, ContentAppended, ContextUsage, DomainEvent, EntryId,
    Envelope, EventId, HeaderEntry, MarkerEntry, MarkerKind, Notice, NoticeEntry, Seq,
    ThreadCreated, ThreadStatus, ToolCallEntry, ToolCallStarted, ToolCallUpdated, UsageUpdated,
    ViewEntry,
};

use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::order::insert_entry;
use crate::state::ThreadState;
use crate::text::text_appended;

/// Folds one envelope of the thread's stream; the caller has checked its stream and `seq`.
pub(crate) fn fold(state: &mut ThreadState, envelope: &Envelope, out: &mut Applied) {
    let seq = envelope.seq;
    let event_id = envelope.event_id;
    match &envelope.event {
        DomainEvent::ThreadCreated(event) => thread_created(state, seq, event, out),
        DomainEvent::ThreadRenamed(event) => {
            update_header(state, "thread_renamed", out, |h| {
                set_some(&mut h.title, &event.title)
            });
        }
        DomainEvent::ThreadArchived(_) => {
            set_status(state, "thread_archived", out, ThreadStatus::Archived);
        }
        // An attach changes nothing yet: the next prompt gives the status. Going back to `Idle`
        // after an exit is a restart transition, not folded yet.
        DomainEvent::AgentSessionAttached(_) | DomainEvent::Unknown { .. } => {}
        DomainEvent::AgentSessionDetached(event) => turn::detached(state, &event.reason, out),
        DomainEvent::UserMessage(event) => turn::user_message(state, seq, event, out),
        DomainEvent::TurnQueued(event) => turn::turn_queued(state, event, out),
        DomainEvent::QueuedPromptEdited(_) => not_yet(out, "queued_prompt_edited"),
        DomainEvent::SteerResolved(_) => not_yet(out, "steer_resolved"),
        DomainEvent::TurnStarted(event) => turn::turn_started(state, event.turn, out),
        DomainEvent::TextAppended(event) => text_appended(state, seq, event, out),
        DomainEvent::ContentAppended(event) => content_appended(state, seq, event_id, event, out),
        DomainEvent::ToolCallStarted(event) => tool_call_started(state, seq, event, out),
        DomainEvent::ToolCallUpdated(event) => tool_call_updated(state, event, out),
        DomainEvent::PermissionRequested(event) => {
            asks::permission_requested(state, seq, event, out);
        }
        DomainEvent::PermissionResolved(event) => asks::permission_resolved(state, event, out),
        DomainEvent::PermissionExpired(event) => asks::permission_expired(state, event, out),
        DomainEvent::ElicitationRequested(event) => {
            asks::elicitation_requested(state, seq, event, out);
        }
        DomainEvent::ElicitationResolved(event) => asks::elicitation_resolved(state, event, out),
        DomainEvent::ElicitationExpired(event) => asks::elicitation_expired(state, event, out),
        DomainEvent::PlanReplaced(event) => {
            update_header(state, "plan_replaced", out, |h| {
                set_vec(&mut h.plan, &event.entries)
            });
        }
        DomainEvent::ModeChanged(event) => {
            update_header(state, "mode_changed", out, |h| {
                set_some(&mut h.mode_id, &event.mode_id)
            });
        }
        DomainEvent::ConfigOptionsReplaced(event) => {
            update_header(state, "config_options_replaced", out, |h| {
                set_vec(&mut h.config_options, &event.options)
            });
        }
        DomainEvent::CommandsAvailable(event) => {
            update_header(state, "commands_available", out, |h| {
                set_vec(&mut h.commands, &event.commands)
            });
        }
        DomainEvent::UsageUpdated(event) => usage_updated(state, event, out),
        DomainEvent::Notice(event) => notice(state, seq, event_id, event, out),
        DomainEvent::CompactionChanged(event) => compaction(state, seq, event_id, event, out),
        DomainEvent::FilesChanged(event) => turn::files_changed(state, event, out),
        DomainEvent::TurnFailed(event) => turn::turn_failed(state, event, out),
        DomainEvent::TurnStoppedAtLimit(event) => turn::stopped_at_limit(state, seq, event, out),
        DomainEvent::TurnEnded(event) => turn::turn_ended(state, seq, event, out),
        DomainEvent::StepUndone(event) => turn::step_undone(state, seq, event_id, event, out),
        DomainEvent::MatterCreated(_) => wrong_stream(out, "matter_created"),
        DomainEvent::CheckpointRecorded(_) => wrong_stream(out, "checkpoint_recorded"),
        DomainEvent::OutsideEdits(_) => wrong_stream(out, "outside_edits"),
        DomainEvent::VersionRestored(_) => wrong_stream(out, "version_restored"),
    }
}

fn not_yet(out: &mut Applied, event: &'static str) {
    out.anomalies.push(Anomaly::NotYetFolded { event });
}

fn wrong_stream(out: &mut Applied, event: &'static str) {
    out.anomalies.push(Anomaly::WrongStreamKind { event });
}

/// Sets `field` to `value` and says whether it changed.
pub(super) fn set<T: PartialEq>(field: &mut T, value: T) -> bool {
    if *field == value {
        return false;
    }
    *field = value;
    true
}

/// Sets `field` to a copy of `value`, copying only when it differs.
pub(super) fn set_clone<T: PartialEq + Clone>(field: &mut T, value: &T) -> bool {
    if field == value {
        return false;
    }
    value.clone_into(field);
    true
}

/// Sets `field` to a copy of `value` in `Some`, copying only when it differs.
pub(super) fn set_some<T: PartialEq + Clone>(field: &mut Option<T>, value: &T) -> bool {
    if field.as_ref() == Some(value) {
        return false;
    }
    *field = Some(value.clone());
    true
}

/// Sets a list to a copy of `value`, copying only when it differs.
pub(super) fn set_vec<T: PartialEq + Clone>(field: &mut Vec<T>, value: &[T]) -> bool {
    if field.as_slice() == value {
        return false;
    }
    value.clone_into(field);
    true
}

/// Changes the header with `change`, which says whether it changed anything, and emits the
/// header's `Upsert` if so. No header yet is [`Anomaly::UnknownReference`].
pub(super) fn update_header(
    state: &mut ThreadState,
    event: &'static str,
    out: &mut Applied,
    change: impl FnOnce(&mut HeaderEntry) -> bool,
) {
    let Some(header) = state.header.as_mut() else {
        out.anomalies.push(Anomaly::UnknownReference {
            event,
            target: EntryId::Header(state.thread),
        });
        return;
    };
    if change(header) {
        out.changes
            .push(ViewChange::Upsert(Box::new(ViewEntry::Header(
                header.clone(),
            ))));
    }
}

pub(super) fn set_status(
    state: &mut ThreadState,
    event: &'static str,
    out: &mut Applied,
    to: ThreadStatus,
) {
    update_header(state, event, out, |h| set(&mut h.status, to));
}

fn thread_created(state: &mut ThreadState, seq: Seq, event: &ThreadCreated, out: &mut Applied) {
    if state.header.is_some() {
        out.anomalies.push(Anomaly::Duplicate {
            event: "thread_created",
            target: EntryId::Header(state.thread),
        });
        return;
    }
    let header = HeaderEntry {
        order: seq,
        thread: state.thread,
        matter: event.matter,
        title: event.title.clone(),
        agent: event.agent.clone(),
        status: ThreadStatus::Idle,
        queue_held: false,
        mode_id: None,
        config_options: Vec::new(),
        commands: Vec::new(),
        plan: Vec::new(),
        usage: None,
    };
    out.changes
        .push(ViewChange::Upsert(Box::new(ViewEntry::Header(
            header.clone(),
        ))));
    state.header = Some(header);
}

/// Each field the event carries replaces the shown one; a missing field keeps it.
fn usage_updated(state: &mut ThreadState, event: &UsageUpdated, out: &mut Applied) {
    if event.context_used.is_none() && event.context_size.is_none() && event.cost.is_none() {
        return;
    }
    update_header(state, "usage_updated", out, |h| {
        let mut changed = h.usage.is_none();
        let usage = h.usage.get_or_insert_with(ContextUsage::default);
        if let Some(used) = event.context_used {
            changed |= set(&mut usage.context_used, Some(used));
        }
        if let Some(size) = event.context_size {
            changed |= set(&mut usage.context_size, Some(size));
        }
        if let Some(cost) = &event.cost {
            changed |= set(&mut usage.cost, Some(cost.clone()));
        }
        changed
    });
}

fn notice(state: &mut ThreadState, seq: Seq, event_id: EventId, event: &Notice, out: &mut Applied) {
    let entry = NoticeEntry {
        order: seq,
        event: event_id,
        level: event.level,
        text: event.text.clone(),
    };
    insert_new(state, seq, ViewEntry::Notice(entry), out);
}

fn compaction(
    state: &mut ThreadState,
    seq: Seq,
    event_id: EventId,
    event: &CompactionChanged,
    out: &mut Applied,
) {
    let kind = MarkerKind::Compaction {
        status: event.status,
        summary: event.summary.clone(),
    };
    insert_marker(state, seq, event_id, kind, out);
}

fn content_appended(
    state: &mut ThreadState,
    seq: Seq,
    event_id: EventId,
    event: &ContentAppended,
    out: &mut Applied,
) {
    let entry = AgentContentEntry {
        order: seq,
        event: event_id,
        turn: event.turn,
        message: event.message,
        kind: event.kind,
        block: event.block.clone(),
    };
    insert_new(state, seq, ViewEntry::AgentContent(entry), out);
}

fn tool_call_started(
    state: &mut ThreadState,
    seq: Seq,
    event: &ToolCallStarted,
    out: &mut Applied,
) {
    if state.index.tool_calls.contains_key(&event.tool_call) {
        out.anomalies.push(Anomaly::Duplicate {
            event: "tool_call_started",
            target: EntryId::ToolCall(event.tool_call),
        });
        return;
    }
    state.index.tool_calls.insert(event.tool_call, seq);
    let entry = ToolCallEntry {
        order: seq,
        tool_call: event.tool_call,
        turn: event.turn,
        title: event.title.clone(),
        kind: event.kind,
        tool_name: event.tool_name.clone(),
        status: event.status,
        locations: event.locations.clone(),
        raw_input: event.raw_input.clone(),
        raw_output: event.raw_output.clone(),
        content: event.content.clone(),
    };
    insert_new(state, seq, ViewEntry::ToolCall(Box::new(entry)), out);
}

fn tool_call_updated(state: &mut ThreadState, event: &ToolCallUpdated, out: &mut Applied) {
    let target = EntryId::ToolCall(event.tool_call);
    let found = state.index.tool_calls.get(&event.tool_call).copied();
    let Some(ViewEntry::ToolCall(entry)) = found.and_then(|order| state.entries.get_mut(&order))
    else {
        out.anomalies.push(Anomaly::UnknownReference {
            event: "tool_call_updated",
            target,
        });
        return;
    };
    let patch = &event.patch;
    let mut changed = false;
    if let Some(title) = &patch.title {
        changed |= set_clone(&mut entry.title, title);
    }
    if let Some(kind) = patch.kind {
        changed |= set(&mut entry.kind, kind);
    }
    if let Some(name) = &patch.tool_name {
        changed |= set_some(&mut entry.tool_name, name);
    }
    if let Some(status) = patch.status {
        changed |= set(&mut entry.status, status);
    }
    if let Some(locations) = &patch.locations {
        changed |= set_vec(&mut entry.locations, locations);
    }
    if let Some(input) = &patch.raw_input {
        changed |= set_some(&mut entry.raw_input, input);
    }
    if let Some(output) = &patch.raw_output {
        changed |= set_some(&mut entry.raw_output, output);
    }
    if let Some(content) = &patch.content {
        changed |= set_vec(&mut entry.content, content);
    }
    if changed {
        out.changes
            .push(ViewChange::Upsert(Box::new(ViewEntry::ToolCall(
                entry.clone(),
            ))));
    }
}

pub(super) fn insert_marker(
    state: &mut ThreadState,
    seq: Seq,
    event: EventId,
    kind: MarkerKind,
    out: &mut Applied,
) {
    let entry = MarkerEntry {
        order: seq,
        event,
        kind,
    };
    insert_new(state, seq, ViewEntry::Marker(entry), out);
}

/// Emits a new entry's `Upsert` and inserts it at `order`.
pub(super) fn insert_new(state: &mut ThreadState, order: Seq, entry: ViewEntry, out: &mut Applied) {
    out.changes
        .push(ViewChange::Upsert(Box::new(entry.clone())));
    insert_entry(state, order, entry, out);
}
