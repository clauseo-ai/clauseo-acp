//! Splitting text so no change carries more than `INLINE_LIMIT_BYTES`, counted as
//! `escaped_len` counts it: the `Upsert` of a message with its text cut at the limit, and
//! `AppendText` changes for the rest.

use clauseo_protocol::{
    AgentTextEntry, AppendTarget, ContentBlock, INLINE_LIMIT_BYTES, MessageId, UserMessageEntry,
    ViewEntry, escaped_len,
};

use crate::change::ViewChange;
use crate::state::count_u16;

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
pub(crate) fn push_appends(
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
