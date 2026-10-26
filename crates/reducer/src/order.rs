//! Entry orders: inserting an entry at its order, and moving a message a delta opened when an
//! event takes its place.

use clauseo_protocol::{Seq, ViewEntry};

use crate::Applied;
use crate::change::Anomaly;
use crate::split::push_text_upsert;
use crate::state::ThreadState;

/// Inserts `entry` at `order`. A message a delta opened at that order moves after the last
/// entry, with its `Upsert` and [`Anomaly::OrderCorrected`]: under the host contract its
/// `TextAppended` would have taken this `seq`, and its own event corrects it again.
pub(crate) fn insert_entry(
    state: &mut ThreadState,
    order: Seq,
    entry: ViewEntry,
    out: &mut Applied,
) {
    if let Some(mut moved) = state.entries.insert(order, entry) {
        let ViewEntry::AgentText(text) = &moved else {
            return;
        };
        let message = text.message;
        let last = state
            .entries
            .last_key_value()
            .map_or(order, |(key, _)| *key);
        let to = seq_plus(last, 1);
        set_order(&mut moved, to);
        if let Some(meta) = state.index.texts.get_mut(&message) {
            meta.order = to;
        }
        if let ViewEntry::AgentText(text) = &moved {
            push_text_upsert(&mut out.changes, text);
        }
        state.entries.insert(to, moved);
        out.anomalies.push(Anomaly::OrderCorrected {
            message,
            from: order,
            to,
        });
    }
}

/// `seq + n`, saturating at the largest `Seq`.
pub(crate) fn seq_plus(seq: Seq, n: u64) -> Seq {
    Seq::new(seq.get().saturating_add(n)).unwrap_or(seq)
}

/// Sets an agent text entry's order; other entries never move.
pub(crate) fn set_order(entry: &mut ViewEntry, order: Seq) {
    if let ViewEntry::AgentText(text) = entry {
        text.order = order;
    }
}
