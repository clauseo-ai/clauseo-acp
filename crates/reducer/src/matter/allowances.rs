//! The matter header's R18 fields: the remembered answers (`AllowanceRecorded`,
//! `AllowanceForgotten`), the reads granted to other matters (`ReadGrantedTo`,
//! `ReadGrantForgotten`) and the protection switch (`ProtectionSwitched`).

use clauseo_protocol::{
    AllowanceForgotten, AllowanceRecorded, EntryId, Envelope, MatterAllowance, MatterHeaderEntry,
    ProtectionSwitched, ReadGrantForgotten, ReadGrantedTo, ViewEntry,
};

use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::state::MatterState;

/// A remembered answer, appended in the order recorded, timed by its envelope. One the header
/// already holds is [`Anomaly::Duplicate`] and changes nothing.
pub(super) fn recorded(
    state: &mut MatterState,
    envelope: &Envelope,
    event: &AllowanceRecorded,
    out: &mut Applied,
) {
    let id = event.allowance;
    if held(state, |h| h.allowances.iter().any(|a| a.allowance == id)) {
        duplicate(state, "allowance_recorded", out);
        return;
    }
    let allowance = MatterAllowance {
        matter: state.matter,
        allowance: id,
        target: event.target.clone(),
        mode: event.mode,
        allowed: event.allowed,
        at: envelope.ts,
    };
    update_header(state, "allowance_recorded", out, |h| {
        h.allowances.push(allowance);
        true
    });
}

/// A remembered answer forgotten. One the header does not hold is
/// [`Anomaly::UnknownReference`] and changes nothing.
pub(super) fn forgotten(state: &mut MatterState, event: &AllowanceForgotten, out: &mut Applied) {
    let id = event.allowance;
    if !held(state, |h| h.allowances.iter().any(|a| a.allowance == id)) {
        unknown(state, "allowance_forgotten", out);
        return;
    }
    update_header(state, "allowance_forgotten", out, |h| {
        h.allowances.retain(|a| a.allowance != id);
        true
    });
}

/// Protection switched off or on: the header's `switched_off`.
pub(super) fn switched(state: &mut MatterState, event: ProtectionSwitched, out: &mut Applied) {
    update_header(state, "protection_switched", out, |h| {
        let changed = h.switched_off != event.off;
        h.switched_off = event.off;
        changed
    });
}

/// Another matter may read some of this one's files, appended in the order granted. A grant
/// the header already holds (the same matter and allowance) is [`Anomaly::Duplicate`].
pub(super) fn granted(state: &mut MatterState, event: &ReadGrantedTo, out: &mut Applied) {
    let same = |g: &ReadGrantedTo| g.by_matter == event.by_matter && g.allowance == event.allowance;
    if held(state, |h| h.read_grants.iter().any(same)) {
        duplicate(state, "read_granted_to", out);
        return;
    }
    update_header(state, "read_granted_to", out, |h| {
        h.read_grants.push(event.clone());
        true
    });
}

/// A read grant ended. One the header does not hold is [`Anomaly::UnknownReference`].
pub(super) fn grant_forgotten(
    state: &mut MatterState,
    event: &ReadGrantForgotten,
    out: &mut Applied,
) {
    let same = |g: &ReadGrantedTo| g.by_matter == event.by_matter && g.allowance == event.allowance;
    if !held(state, |h| h.read_grants.iter().any(same)) {
        unknown(state, "read_grant_forgotten", out);
        return;
    }
    update_header(state, "read_grant_forgotten", out, |h| {
        h.read_grants.retain(|g| !same(g));
        true
    });
}

/// Changes the header with `change`, which says whether it changed anything, and emits the
/// header's `Upsert` if so. No header yet is [`Anomaly::UnknownReference`].
fn update_header(
    state: &mut MatterState,
    event: &'static str,
    out: &mut Applied,
    change: impl FnOnce(&mut MatterHeaderEntry) -> bool,
) {
    let Some(header) = state.header.as_mut() else {
        unknown(state, event, out);
        return;
    };
    if change(header) {
        out.changes
            .push(ViewChange::Upsert(Box::new(ViewEntry::MatterHeader(
                header.clone(),
            ))));
    }
}

/// Whether the header exists and `test` holds for it.
fn held(state: &MatterState, test: impl FnOnce(&MatterHeaderEntry) -> bool) -> bool {
    state.header.as_ref().is_some_and(test)
}

fn duplicate(state: &MatterState, event: &'static str, out: &mut Applied) {
    out.anomalies.push(Anomaly::Duplicate {
        event,
        target: EntryId::MatterHeader(state.matter),
    });
}

fn unknown(state: &MatterState, event: &'static str, out: &mut Applied) {
    out.anomalies.push(Anomaly::UnknownReference {
        event,
        target: EntryId::MatterHeader(state.matter),
    });
}
