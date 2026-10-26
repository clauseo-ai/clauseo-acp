//! Permission requests and elicitations: `NeedsYou` while one waits.

use clauseo_protocol::{
    ElicitationEntry, ElicitationExpired, ElicitationId, ElicitationRequested, ElicitationResolved,
    ElicitationState, EntryId, PermissionEntry, PermissionExpired, PermissionRequestId,
    PermissionRequested, PermissionResolved, PermissionState, Seq, ThreadStatus, ViewEntry,
};

use super::turn::settled_status;
use super::{insert_new, set_status};
use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::state::ThreadState;

pub(super) fn permission_requested(
    state: &mut ThreadState,
    seq: Seq,
    event: &PermissionRequested,
    out: &mut Applied,
) {
    if state.index.permissions.contains_key(&event.request) {
        out.anomalies.push(Anomaly::Duplicate {
            event: "permission_requested",
            target: EntryId::Permission(event.request),
        });
        return;
    }
    state.index.permissions.insert(event.request, seq);
    let entry = PermissionEntry {
        order: seq,
        request: event.request,
        tool_call: event.tool_call,
        options: event.options.clone(),
        deliverable: event.deliverable,
        state: PermissionState::Pending,
    };
    insert_new(state, seq, ViewEntry::Permission(entry), out);
    asked(state, "permission_requested", out);
}

pub(super) fn permission_resolved(
    state: &mut ThreadState,
    event: &PermissionResolved,
    out: &mut Applied,
) {
    let settled = PermissionState::Resolved {
        choice: event.choice.clone(),
        by: event.by.clone(),
    };
    settle_permission(state, event.request, "permission_resolved", settled, out);
}

pub(super) fn permission_expired(
    state: &mut ThreadState,
    event: &PermissionExpired,
    out: &mut Applied,
) {
    let settled = PermissionState::Expired {
        reason: event.reason,
    };
    settle_permission(state, event.request, "permission_expired", settled, out);
}

pub(super) fn elicitation_requested(
    state: &mut ThreadState,
    seq: Seq,
    event: &ElicitationRequested,
    out: &mut Applied,
) {
    if state.index.elicitations.contains_key(&event.elicitation) {
        out.anomalies.push(Anomaly::Duplicate {
            event: "elicitation_requested",
            target: EntryId::Elicitation(event.elicitation),
        });
        return;
    }
    state.index.elicitations.insert(event.elicitation, seq);
    let entry = ElicitationEntry {
        order: seq,
        elicitation: event.elicitation,
        request: event.request.clone(),
        state: ElicitationState::Pending,
    };
    insert_new(state, seq, ViewEntry::Elicitation(entry), out);
    asked(state, "elicitation_requested", out);
}

pub(super) fn elicitation_resolved(
    state: &mut ThreadState,
    event: &ElicitationResolved,
    out: &mut Applied,
) {
    let settled = ElicitationState::Resolved {
        outcome: event.outcome,
        by: event.by.clone(),
    };
    settle_elicitation(
        state,
        event.elicitation,
        "elicitation_resolved",
        settled,
        out,
    );
}

pub(super) fn elicitation_expired(
    state: &mut ThreadState,
    event: &ElicitationExpired,
    out: &mut Applied,
) {
    let settled = ElicitationState::Expired {
        reason: event.reason,
    };
    settle_elicitation(
        state,
        event.elicitation,
        "elicitation_expired",
        settled,
        out,
    );
}

fn asked(state: &mut ThreadState, event: &'static str, out: &mut Applied) {
    state.index.waiting = state.index.waiting.saturating_add(1);
    set_status(state, event, out, ThreadStatus::NeedsYou);
}

/// One fewer question waits; a `NeedsYou` status goes back to what the turn shows.
fn answered(state: &mut ThreadState, event: &'static str, out: &mut Applied) {
    state.index.waiting = state.index.waiting.saturating_sub(1);
    if state.status() == ThreadStatus::NeedsYou {
        let status = settled_status(state);
        set_status(state, event, out, status);
    }
}

fn settle_permission(
    state: &mut ThreadState,
    request: PermissionRequestId,
    event: &'static str,
    settled: PermissionState,
    out: &mut Applied,
) {
    let target = EntryId::Permission(request);
    let found = state.index.permissions.get(&request).copied();
    let Some(ViewEntry::Permission(entry)) = found.and_then(|order| state.entries.get_mut(&order))
    else {
        out.anomalies
            .push(Anomaly::UnknownReference { event, target });
        return;
    };
    if entry.state != PermissionState::Pending {
        out.anomalies
            .push(Anomaly::AlreadySettled { event, target });
        return;
    }
    entry.state = settled;
    out.changes
        .push(ViewChange::Upsert(Box::new(ViewEntry::Permission(
            entry.clone(),
        ))));
    answered(state, event, out);
}

fn settle_elicitation(
    state: &mut ThreadState,
    elicitation: ElicitationId,
    event: &'static str,
    settled: ElicitationState,
    out: &mut Applied,
) {
    let target = EntryId::Elicitation(elicitation);
    let found = state.index.elicitations.get(&elicitation).copied();
    let Some(ViewEntry::Elicitation(entry)) = found.and_then(|order| state.entries.get_mut(&order))
    else {
        out.anomalies
            .push(Anomaly::UnknownReference { event, target });
        return;
    };
    if entry.state != ElicitationState::Pending {
        out.anomalies
            .push(Anomaly::AlreadySettled { event, target });
        return;
    }
    entry.state = settled;
    out.changes
        .push(ViewChange::Upsert(Box::new(ViewEntry::Elicitation(
            entry.clone(),
        ))));
    answered(state, event, out);
}
