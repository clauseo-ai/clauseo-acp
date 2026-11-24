//! A helper's events (a subagent the lead agent launched): its start opens a `Helper` entry,
//! each run of its words is a `HelperText` entry of its own, and its end settles the `Helper`
//! entry once. Its steps are tool calls with `helper` set, folded in `thread.rs`.

use clauseo_protocol::{
    EntryId, EventId, HelperEnded, HelperEntry, HelperStarted, HelperText, HelperTextEntry, Seq,
    Timestamp, ViewEntry,
};

use super::insert_new;
use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::state::ThreadState;

/// Opens the helper's entry at `seq`; a second start for the same helper changes nothing.
pub(super) fn started(
    state: &mut ThreadState,
    seq: Seq,
    ts: Timestamp,
    event: &HelperStarted,
    out: &mut Applied,
) {
    if state.index.helpers.contains_key(&event.helper) {
        out.anomalies.push(Anomaly::Duplicate {
            event: "helper_started",
            target: EntryId::Helper(event.helper),
        });
        return;
    }
    state.index.helpers.insert(event.helper, seq);
    state.index.open_helpers += 1;
    let entry = HelperEntry {
        order: seq,
        helper: event.helper,
        turn: event.turn,
        parent: event.parent,
        call: event.call,
        name: event.name.clone(),
        label: event.label.clone(),
        task: event.task.clone(),
        model: event.model.clone(),
        outcome: None,
        summary: None,
        started_at: ts,
        finished_at: None,
    };
    insert_new(state, seq, ViewEntry::Helper(Box::new(entry)), out);
}

/// A run of the helper's words, as an entry of its own at `seq`. It is kept whether or not
/// the helper's start is in the state: the screen shows words of a helper it has no card for
/// as the lead's.
pub(super) fn text(
    state: &mut ThreadState,
    seq: Seq,
    event_id: EventId,
    event: &HelperText,
    out: &mut Applied,
) {
    let entry = HelperTextEntry {
        order: seq,
        event: event_id,
        turn: event.turn,
        helper: event.helper,
        text: event.text.clone(),
    };
    insert_new(state, seq, ViewEntry::HelperText(entry), out);
}

/// Settles the helper's entry: its outcome, summary and finish time. An end of a helper the
/// state does not hold, or of one already ended, changes nothing.
pub(super) fn ended(
    state: &mut ThreadState,
    ts: Timestamp,
    event: &HelperEnded,
    out: &mut Applied,
) {
    let target = EntryId::Helper(event.helper);
    let found = state.index.helpers.get(&event.helper).copied();
    let Some(ViewEntry::Helper(entry)) = found.and_then(|order| state.entries.get_mut(&order))
    else {
        out.anomalies.push(Anomaly::UnknownReference {
            event: "helper_ended",
            target,
        });
        return;
    };
    if entry.outcome.is_some() {
        out.anomalies.push(Anomaly::AlreadySettled {
            event: "helper_ended",
            target,
        });
        return;
    }
    entry.outcome = Some(event.outcome);
    state.index.open_helpers = state.index.open_helpers.saturating_sub(1);
    entry.summary.clone_from(&event.summary);
    entry.finished_at = Some(ts);
    out.changes
        .push(ViewChange::Upsert(Box::new(ViewEntry::Helper(
            entry.clone(),
        ))));
}
