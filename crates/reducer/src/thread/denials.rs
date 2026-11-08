//! Denials (R18): a refusal's entry and its card's state, the dialog's answer, a dismissed
//! card, a paused answer, and the cards an agent's exit leaves with nothing to widen.

use clauseo_protocol::{
    DenialDismissed, DenialEntry, DenialId, DenialRecorded, DenialState, DenialsPaused,
    DenialsPausedEntry, EntryId, EscalationAnswer, EscalationAnswered, EventId, Seq, Timestamp,
    ViewEntry,
};

use super::{insert_new, set};
use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::state::ThreadState;

/// A refusal: a new entry, `Pending` when its card may widen it, else `NotOffered`. The same
/// denial logged again updates its `count` and nothing else.
pub(super) fn recorded(
    state: &mut ThreadState,
    seq: Seq,
    ts: Timestamp,
    event: &DenialRecorded,
    out: &mut Applied,
) {
    if state.index.denials.contains_key(&event.denial) {
        with_denial(state, "denial_recorded", event.denial, out, |entry| {
            set(&mut entry.count, event.count)
        });
        return;
    }
    let entry = DenialEntry {
        order: seq,
        denial: event.denial,
        turn: event.turn,
        kind: event.kind.clone(),
        source: event.source.clone(),
        state: if event.offered {
            DenialState::Pending
        } else {
            DenialState::NotOffered
        },
        answer: None,
        count: event.count,
        ts,
    };
    state.index.denials.insert(event.denial, seq);
    insert_new(state, seq, ViewEntry::Denial(entry), out);
}

/// The dialog's answer, on the card that asked: `Allowed` or `Refused`, an answer this version
/// does not know read as a refusal. A matter-level escalation names no card and changes
/// nothing here.
pub(super) fn answered(state: &mut ThreadState, event: &EscalationAnswered, out: &mut Applied) {
    let Some(denial) = event.denial else {
        return;
    };
    let to = match event.answer {
        EscalationAnswer::AllowOnce | EscalationAnswer::AllowForMatter => DenialState::Allowed,
        EscalationAnswer::Refuse
        | EscalationAnswer::RefuseForMatter
        | EscalationAnswer::Unknown => DenialState::Refused,
    };
    with_denial(state, "escalation_answered", denial, out, |entry| {
        set(&mut entry.answer, Some(event.answer)) | set(&mut entry.state, to)
    });
}

/// The lawyer dismissed the card: `Dismissed`, which the webview hides.
pub(super) fn dismissed(state: &mut ThreadState, event: &DenialDismissed, out: &mut Applied) {
    with_denial(state, "denial_dismissed", event.denial, out, |entry| {
        set(&mut entry.state, DenialState::Dismissed)
    });
}

/// The host paused the answer: a new entry of its own.
pub(super) fn paused(
    state: &mut ThreadState,
    seq: Seq,
    event_id: EventId,
    ts: Timestamp,
    event: &DenialsPaused,
    out: &mut Applied,
) {
    let entry = ViewEntry::DenialsPaused(DenialsPausedEntry {
        order: seq,
        event: event_id,
        turn: event.turn,
        refused: event.refused,
        ts,
    });
    insert_new(state, seq, entry, out);
}

/// The agent left for good (any detach but a restart): every card still pending expires,
/// since the launch it would widen is gone. A restart keeps them: the turn goes on.
pub(super) fn expire_pending(state: &mut ThreadState, out: &mut Applied) {
    for entry in state.entries.values_mut() {
        if let ViewEntry::Denial(denial) = entry
            && denial.state == DenialState::Pending
        {
            denial.state = DenialState::Expired;
            out.changes
                .push(ViewChange::Upsert(Box::new(entry.clone())));
        }
    }
}

/// Changes the denial's entry with `change`, which says whether it changed anything, and emits
/// its `Upsert` if so. A denial the state does not hold is [`Anomaly::UnknownReference`].
fn with_denial(
    state: &mut ThreadState,
    event: &'static str,
    denial: DenialId,
    out: &mut Applied,
    change: impl FnOnce(&mut DenialEntry) -> bool,
) {
    let found = state.index.denials.get(&denial).copied();
    let Some(ViewEntry::Denial(entry)) = found.and_then(|order| state.entries.get_mut(&order))
    else {
        out.anomalies.push(Anomaly::UnknownReference {
            event,
            target: EntryId::Denial(denial),
        });
        return;
    };
    if change(entry) {
        out.changes
            .push(ViewChange::Upsert(Box::new(ViewEntry::Denial(
                entry.clone(),
            ))));
    }
}
