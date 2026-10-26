//! Message text: appends from events and from display-stage deltas, and the durable part of
//! each message. Splitting text at `INLINE_LIMIT_BYTES` is in `split.rs`.
//!
//! Each message keeps the UTF-16 length of its shown text and of its durable part in its
//! [`TextMeta`], and adds `utf16_len(chunk)` on each append, so a `from_len` is never a rescan.

use clauseo_protocol::{AgentTextEntry, AppendTarget, Seq, TextAppended, ViewEntry};

use crate::change::{Anomaly, TextDelta};
use crate::order::{insert_entry, seq_plus, set_order};
use crate::split::{push_appends, push_text_upsert};
use crate::state::{TextMeta, ThreadState, count_u16};
use crate::{Applied, Error};

/// Folds a `TextAppended` at `seq`.
///
/// When the shown text past the durable part starts with the event's text, the durable part
/// grows and nothing is emitted: the screen already shows it. When the event's text starts with
/// that shown text (all of it when none is shown), the rest is appended as usual. Otherwise the
/// un-durable text is dropped, the event's text appended, and the whole entry sent again with
/// [`Anomaly::DeltaMismatch`]. A message opened by a delta at another order moves to `seq`
/// ([`Anomaly::OrderCorrected`]).
pub(crate) fn text_appended(
    state: &mut ThreadState,
    seq: Seq,
    event: &TextAppended,
    out: &mut Applied,
) {
    let Some(meta) = state.index.texts.get(&event.message).copied() else {
        open_message(state, seq, event, out);
        return;
    };
    let mut order = meta.order;
    let mut resend = false;
    if meta.provisional {
        state.index.provisional = state.index.provisional.saturating_sub(1);
        if order != seq {
            if let Some(mut entry) = state.entries.remove(&order) {
                set_order(&mut entry, seq);
                insert_entry(state, seq, entry, out);
            }
            out.anomalies.push(Anomaly::OrderCorrected {
                message: event.message,
                from: order,
                to: seq,
            });
            order = seq;
            resend = true;
        }
    }
    let Some(ViewEntry::AgentText(entry)) = state.entries.get_mut(&order) else {
        return;
    };
    if entry.kind != event.kind {
        out.anomalies.push(Anomaly::KindMismatch {
            message: event.message,
            kind: event.kind,
        });
    }
    let mut meta = TextMeta {
        order,
        provisional: false,
        ..meta
    };
    let undurable = entry.text.get(meta.durable_len..).unwrap_or_default();
    if undurable.starts_with(event.text.as_str()) {
        // The screen already shows it.
        meta.durable_len += event.text.len();
        meta.durable_u16 = meta.durable_u16.saturating_add(count_u16(&event.text));
        if resend {
            push_text_upsert(&mut out.changes, entry);
        }
    } else if let Some(extra) = event.text.strip_prefix(undurable) {
        // It extends what the screen shows (all of it when nothing un-durable is shown).
        let from_len = meta.shown_u16;
        entry.text.push_str(extra);
        meta.shown_u16 = from_len.saturating_add(count_u16(extra));
        meta.durable_len = entry.text.len();
        meta.durable_u16 = meta.shown_u16;
        if resend {
            push_text_upsert(&mut out.changes, entry);
        } else {
            let target = AppendTarget::AgentText;
            push_appends(&mut out.changes, event.message, target, from_len, extra);
        }
    } else {
        entry.text.truncate(meta.durable_len);
        entry.text.push_str(&event.text);
        meta.durable_len = entry.text.len();
        meta.durable_u16 = meta.durable_u16.saturating_add(count_u16(&event.text));
        meta.shown_u16 = meta.durable_u16;
        push_text_upsert(&mut out.changes, entry);
        out.anomalies.push(Anomaly::DeltaMismatch {
            message: event.message,
        });
    }
    state.index.texts.insert(event.message, meta);
}

/// Opens a message first seen as an event: a durable entry at `seq`.
fn open_message(state: &mut ThreadState, seq: Seq, event: &TextAppended, out: &mut Applied) {
    let entry = AgentTextEntry {
        order: seq,
        message: event.message,
        turn: event.turn,
        kind: event.kind,
        text: event.text.clone(),
    };
    push_text_upsert(&mut out.changes, &entry);
    let u16 = count_u16(&entry.text);
    state.index.texts.insert(
        event.message,
        TextMeta {
            order: seq,
            shown_u16: u16,
            durable_len: entry.text.len(),
            durable_u16: u16,
            provisional: false,
        },
    );
    insert_entry(state, seq, ViewEntry::AgentText(entry), out);
}

/// Folds display-stage text: appends it to the message's shown text, or opens the message at
/// a provisional order: `max(last entry order, pending_seq) + 1` (the host contract in `lib.rs`).
pub(crate) fn text_delta(state: &mut ThreadState, delta: &TextDelta<'_>) -> Result<Applied, Error> {
    if let Some(seq) = state.seq
        && delta.pending_seq < seq
    {
        return Err(Error::PendingSeqBehind {
            seq,
            pending_seq: delta.pending_seq,
        });
    }
    let mut out = Applied::default();
    if delta.text.is_empty() {
        return Ok(out);
    }
    if let Some(meta) = state.index.texts.get_mut(&delta.message)
        && let Some(ViewEntry::AgentText(entry)) = state.entries.get_mut(&meta.order)
    {
        if entry.kind != delta.kind {
            out.anomalies.push(Anomaly::KindMismatch {
                message: delta.message,
                kind: delta.kind,
            });
        }
        let from_len = meta.shown_u16;
        entry.text.push_str(delta.text);
        meta.shown_u16 = from_len.saturating_add(count_u16(delta.text));
        let target = AppendTarget::AgentText;
        push_appends(
            &mut out.changes,
            delta.message,
            target,
            from_len,
            delta.text,
        );
        return Ok(out);
    }
    // After the pending head. Under the host contract (`lib.rs`) a message's first delta comes
    // after the host flushed every other message's text, so no entry is above `pending_seq` and
    // this is the `seq` its first `TextAppended` gets; with the contract broken, `max` still puts
    // it after every entry.
    let last = state.entries.last_key_value().map(|(key, _)| *key);
    let order = seq_plus(
        last.map_or(delta.pending_seq, |last| last.max(delta.pending_seq)),
        1,
    );
    let entry = AgentTextEntry {
        order,
        message: delta.message,
        turn: delta.turn,
        kind: delta.kind,
        text: delta.text.to_owned(),
    };
    push_text_upsert(&mut out.changes, &entry);
    state.index.texts.insert(
        delta.message,
        TextMeta {
            order,
            shown_u16: count_u16(&entry.text),
            durable_len: 0,
            durable_u16: 0,
            provisional: true,
        },
    );
    state.index.provisional += 1;
    state.entries.insert(order, ViewEntry::AgentText(entry));
    Ok(out)
}
