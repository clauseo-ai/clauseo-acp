//! Message text: appends from events and from display-stage deltas, the durable part of each
//! message, and splitting text so no change carries more than `INLINE_LIMIT_BYTES`.
//!
//! Each message keeps the UTF-16 length of its shown text and of its durable part in its
//! [`TextMeta`], and adds `utf16_len(chunk)` on each append, so a `from_len` is never a rescan.

use clauseo_protocol::{
    AgentTextEntry, AppendTarget, ContentBlock, INLINE_LIMIT_BYTES, MessageId, Seq, TextAppended,
    UserMessageEntry, ViewEntry, escaped_len,
};

use crate::change::{Anomaly, TextDelta, ViewChange};
use crate::state::{TextMeta, ThreadState, count_u16};
use crate::thread::insert_entry;
use crate::{Applied, Error};

/// The most bytes JSON escaping makes of one byte (`\u00XX`): text no longer than the limit
/// divided by it fits without counting.
const MAX_ESCAPE: usize = 6;

/// The first piece of `text` whose escaped cost is at most `budget`, cut at a `char` boundary,
/// and the rest.
fn cut(text: &str, budget: usize) -> (&str, &str) {
    // Escaping never shortens text, so only text within the budget in bytes can fit whole; a
    // longer text is walked only as far as its first piece, never scanned to its end.
    if text.len().saturating_mul(MAX_ESCAPE) <= budget
        || (text.len() <= budget && escaped_len(text) <= budget)
    {
        return (text, "");
    }
    let mut cost = 0;
    for (at, ch) in text.char_indices() {
        cost += char_cost(ch);
        if cost > budget {
            return text.split_at_checked(at).unwrap_or((text, ""));
        }
    }
    (text, "")
}

/// One char's share of `escaped_len`, as `serde_json` escapes it.
fn char_cost(ch: char) -> usize {
    match ch {
        '"' | '\\' | '\u{8}' | '\t' | '\n' | '\u{c}' | '\r' => 2,
        '\0'..='\u{1f}' => 6,
        other => other.len_utf8(),
    }
}

/// Pushes `AppendText` changes for `text` on a text `from_len` long, each at most
/// `INLINE_LIMIT_BYTES` escaped. Returns nothing: the caller keeps its own count.
fn push_appends(
    out: &mut Vec<ViewChange>,
    message_id: MessageId,
    target: AppendTarget,
    mut from_len: u32,
    mut text: &str,
) {
    while !text.is_empty() {
        let (piece, rest) = cut(text, INLINE_LIMIT_BYTES);
        if piece.is_empty() {
            // Unreachable for a non-empty text (one char costs at most 6 bytes); stops a loop.
            return;
        }
        out.push(ViewChange::AppendText {
            message_id,
            target,
            from_len,
            text: piece.to_owned(),
        });
        if !rest.is_empty() {
            from_len = from_len.saturating_add(count_u16(piece));
        }
        text = rest;
    }
}

/// Pushes an `Upsert` of an agent text entry with its text cut at the limit, then `AppendText`
/// changes for the rest.
pub(crate) fn push_text_upsert(out: &mut Vec<ViewChange>, entry: &AgentTextEntry) {
    let (head, rest) = cut(&entry.text, INLINE_LIMIT_BYTES);
    out.push(ViewChange::Upsert(Box::new(ViewEntry::AgentText(
        AgentTextEntry {
            order: entry.order,
            message: entry.message,
            turn: entry.turn,
            kind: entry.kind,
            text: head.to_owned(),
        },
    ))));
    if !rest.is_empty() {
        let target = AppendTarget::AgentText;
        push_appends(out, entry.message, target, count_u16(head), rest);
    }
}

/// Pushes an `Upsert` of a user message whose `Text` blocks are cut to the limit in all, summed
/// over the blocks in order (a block past the limit goes empty), then `AppendText` changes with
/// `AppendTarget::UserBlock` for the rest of each cut block.
pub(crate) fn push_user_upsert(out: &mut Vec<ViewChange>, entry: &UserMessageEntry) {
    let mut budget = INLINE_LIMIT_BYTES;
    let mut rests: Vec<(u32, u32, &str)> = Vec::new();
    let mut content = Vec::with_capacity(entry.content.len());
    for (block_index, block) in entry.content.iter().enumerate() {
        let ContentBlock::Text { text } = block else {
            content.push(block.clone());
            continue;
        };
        let (head, rest) = cut(text, budget);
        budget = budget.saturating_sub(escaped_len(head));
        if !rest.is_empty() {
            let at = u32::try_from(block_index).unwrap_or(u32::MAX);
            rests.push((at, count_u16(head), rest));
        }
        content.push(ContentBlock::Text {
            text: head.to_owned(),
        });
    }
    out.push(ViewChange::Upsert(Box::new(ViewEntry::UserMessage(
        UserMessageEntry {
            content,
            ..without_content(entry)
        },
    ))));
    for (at, from_len, rest) in rests {
        push_appends(
            out,
            entry.message,
            AppendTarget::UserBlock(at),
            from_len,
            rest,
        );
    }
}

/// A copy of `entry` with an empty `content`, for building the cut copy.
fn without_content(entry: &UserMessageEntry) -> UserMessageEntry {
    UserMessageEntry {
        order: entry.order,
        message: entry.message,
        turn: entry.turn,
        content: Vec::new(),
        delivery: entry.delivery,
        status: entry.status,
        edited: entry.edited,
        steer: entry.steer.clone(),
    }
}

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
/// a provisional order: after `pending_seq` and after every message a delta opened above it.
pub(crate) fn text_delta(state: &mut ThreadState, delta: &TextDelta) -> Result<Applied, Error> {
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
        entry.text.push_str(&delta.text);
        meta.shown_u16 = from_len.saturating_add(count_u16(&delta.text));
        let target = AppendTarget::AgentText;
        push_appends(
            &mut out.changes,
            delta.message,
            target,
            from_len,
            &delta.text,
        );
        return Ok(out);
    }
    // After the pending head and after every message a delta opened whose `TextAppended` the
    // host has not sent yet: those are exactly the entries above `pending_seq`.
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
        text: delta.text.clone(),
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
