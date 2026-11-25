//! The agent switch's fold: `AgentSwitched` resets the header for the new agent, and a
//! `ContextHandoff` becomes a handoff entry that its `HandoffSettled` settles.

use clauseo_protocol::{
    AgentId, AgentSwitched, ContextHandoff, EntryId, HandoffEntry, HandoffEntryStatus,
    HandoffSettled, HeaderEntry, Seq, Timestamp, ViewEntry,
};

use super::{insert_new, queue, set, set_clone, update_header};
use crate::Applied;
use crate::change::{Anomaly, ViewChange};
use crate::state::ThreadState;

/// The thread's agent is now `to`: what the old session reported (its name, mode, options,
/// commands, usage and tools) and the model and effort its last turn ran with are cleared until
/// the new session reports its own and the next `TurnStarted` names them, and whether a steer
/// joins the running turn falls back to what the new agent's kind says. One `Upsert`.
pub(super) fn agent_switched(state: &mut ThreadState, event: &AgentSwitched, out: &mut Applied) {
    update_header(state, "agent_switched", out, |h| {
        switched_header(h, &event.to)
    });
}

/// `header` as `AgentSwitched` to `to` leaves it ([`agent_switched`]); whether it changed. The
/// host lays the same rule over the header it keeps for an unloaded stream at the switch's
/// commit, so no copy of the header holds the old agent's options, mode, commands or tools.
pub fn switched_header(header: &mut HeaderEntry, to: &AgentId) -> bool {
    let h = header;
    let mut changed = set_clone(&mut h.agent, to);
    changed |= set(&mut h.agent_name, None);
    changed |= set(&mut h.mode_id, None);
    changed |= set(&mut h.config_options, Vec::new());
    changed |= set(&mut h.commands, Vec::new());
    changed |= set(&mut h.usage, None);
    changed |= set(&mut h.tools, Vec::new());
    changed |= set(&mut h.model, None);
    changed |= set(&mut h.effort, None);
    changed |= set(&mut h.steer_joins_turn, queue::steer_joins_by_kind(to));
    changed
}

/// A pending handoff entry at `seq`; a second one of the same handover is a duplicate.
pub(super) fn context_handoff(
    state: &mut ThreadState,
    seq: Seq,
    ts: Timestamp,
    event: &ContextHandoff,
    out: &mut Applied,
) {
    if state.index.handoffs.contains_key(&event.handoff) {
        out.anomalies.push(Anomaly::Duplicate {
            event: "context_handoff",
            target: EntryId::Handoff(event.handoff),
        });
        return;
    }
    state.index.handoffs.insert(event.handoff, seq);
    let entry = HandoffEntry {
        order: seq,
        handoff: event.handoff,
        message: event.message,
        from: event.from.clone(),
        to: event.to.clone(),
        answers_covered: event.answers_covered,
        items_included: event.items_included,
        items_left_out: event.items_left_out,
        resumed: event.resumed,
        status: HandoffEntryStatus::Pending,
        ts,
    };
    insert_new(state, seq, ViewEntry::Handoff(entry), out);
}

/// Sets the handoff entry's status; an `Upsert` only when it changed.
pub(super) fn handoff_settled(state: &mut ThreadState, event: &HandoffSettled, out: &mut Applied) {
    let found = state.index.handoffs.get(&event.handoff).copied();
    let Some(ViewEntry::Handoff(entry)) = found.and_then(|order| state.entries.get_mut(&order))
    else {
        out.anomalies.push(Anomaly::UnknownReference {
            event: "handoff_settled",
            target: EntryId::Handoff(event.handoff),
        });
        return;
    };
    if set(&mut entry.status, HandoffEntryStatus::from(event.status)) {
        out.changes
            .push(ViewChange::Upsert(Box::new(ViewEntry::Handoff(
                entry.clone(),
            ))));
    }
}
