//! Message text: cached UTF-16 counts, display-stage deltas and their events, and splitting text
//! so no change carries more than `INLINE_LIMIT_BYTES`, counted escaped.

use clauseo_protocol::{
    AppendTarget, ContentBlock, Delivery, INLINE_LIMIT_BYTES, MessageId, TextKind, TurnId,
    UserMessage, ViewEntry, escaped_len, utf16_len,
};
use clauseo_reducer::{Anomaly, StreamState, ViewChange, apply};

use crate::fixtures::{Log, start_turn, text, thread_created, tool_started, tool_status, usage};
use crate::helpers::{appends, delta, delta_at, first_upsert, fold, seq, shown_text};

/// A thread with a running turn, its state, the turn and a fresh message id.
#[cfg(test)]
fn running() -> (Log, StreamState, TurnId, MessageId) {
    let mut log = Log::thread(7).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Redraft clause 4.").unwrap();
    let message = log.fresh().unwrap();
    let state = fold(log.events());
    (log, state, turn, message)
}

/// Pushes an answer chunk of `chunk` on `message` and applies it.
#[cfg(test)]
fn answer(
    log: &mut Log,
    state: &mut StreamState,
    turn: TurnId,
    message: MessageId,
    chunk: &str,
) -> clauseo_reducer::Applied {
    let envelope = log
        .push(text(turn, message, TextKind::Answer, chunk.to_owned()))
        .unwrap();
    apply(state, &envelope).unwrap()
}

/// `n` bytes or a little more of text mixing 1-, 2-, 3- and 4-byte chars.
#[cfg(test)]
fn multibyte(n: usize) -> String {
    "Clause é 日本 😀 \u{301}. "
        .chars()
        .cycle()
        .scan(0, |len, ch| {
            (*len < n).then(|| {
                *len += ch.len_utf8();
                ch
            })
        })
        .collect()
}

/// Checks appends of `whole` starting at `from_len`: each within the limit, non-empty, their
/// `from_len`s consecutive, and together `whole`.
#[cfg(test)]
fn check_pieces(pieces: &[(AppendTarget, u32, String)], from_len: u32, whole: &str) {
    let mut at = from_len;
    let mut joined = String::new();
    for (_, from, piece) in pieces {
        assert!(!piece.is_empty(), "{pieces:?}");
        assert!(
            escaped_len(piece) <= INLINE_LIMIT_BYTES,
            "{}",
            escaped_len(piece)
        );
        assert_eq!(*from, at);
        at += u32::try_from(utf16_len(piece)).unwrap();
        joined.push_str(piece);
    }
    assert_eq!(joined, whole);
}

#[test]
fn from_len_uses_cached_count() {
    let (mut log, mut state, turn, message) = running();
    let out = answer(&mut log, &mut state, turn, message, "é");
    assert!(matches!(first_upsert(&out.changes), ViewEntry::AgentText(t) if t.text == "é"));
    let mut shown = "é".to_owned();
    for chunk in ["a😀", "b", "日本", "😀😀", "z"] {
        let out = answer(&mut log, &mut state, turn, message, chunk);
        let from_len = u32::try_from(utf16_len(&shown)).unwrap();
        assert_eq!(
            appends(&out.changes),
            [(AppendTarget::AgentText, from_len, chunk.to_owned())]
        );
        shown.push_str(chunk);
    }
    // After a round trip the cached count goes on from the decoded state.
    let mut decoded = crate::helpers::round_trip(&state);
    let envelope = log
        .push(text(turn, message, TextKind::Answer, "!".to_owned()))
        .unwrap();
    let out = apply(&mut decoded, &envelope).unwrap();
    let from_len = u32::try_from(utf16_len(&shown)).unwrap();
    assert_eq!(
        appends(&out.changes),
        [(AppendTarget::AgentText, from_len, "!".to_owned())]
    );
}

#[test]
fn deltas_then_event_show_text_once() {
    let (mut log, mut state, turn, message) = running();
    // "Hello" cut after 3 bytes, so the spell check sees no fragment.
    let (head, tail) = "Hello".split_at(3);
    let opened = delta(&mut state, turn, message, TextKind::Answer, head);
    assert_eq!(opened.changes.len(), 1);
    assert!(matches!(first_upsert(&opened.changes), ViewEntry::AgentText(t) if t.text == head));
    let grown = delta(&mut state, turn, message, TextKind::Answer, tail);
    assert_eq!(
        grown.changes,
        [ViewChange::AppendText {
            message_id: message,
            target: AppendTarget::AgentText,
            from_len: 3,
            text: tail.to_owned(),
        }]
    );
    let out = answer(&mut log, &mut state, turn, message, "Hello");
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(shown_text(&state, message).as_deref(), Some("Hello"));
    // Folded from the events alone, the state is the same.
    assert_eq!(state, fold(log.events()));
}

#[test]
fn delta_mismatch_is_corrected() {
    let (mut log, mut state, turn, message) = running();
    delta(&mut state, turn, message, TextKind::Answer, "Hello");
    let out = answer(&mut log, &mut state, turn, message, "Help");
    assert_eq!(out.changes.len(), 1);
    assert!(matches!(first_upsert(&out.changes), ViewEntry::AgentText(t) if t.text == "Help"));
    assert_eq!(out.anomalies, [Anomaly::DeltaMismatch { message }]);
    assert_eq!(shown_text(&state, message).as_deref(), Some("Help"));
    assert_eq!(state, fold(log.events()));
}

#[test]
fn delta_order_uses_pending_seq() {
    let (mut log, _, turn, message) = running();
    let other = log.fresh().unwrap();
    for n in 0..7 {
        log.push(text(
            turn,
            other,
            TextKind::Thought,
            format!("Thought {n}. "),
        ))
        .unwrap();
    }
    let mut state = fold(log.events());
    assert_eq!(state.seq(), Some(seq(10)));
    let out = delta_at(
        &mut state,
        turn,
        message,
        TextKind::Answer,
        "Done.",
        seq(13),
    );
    assert_eq!(first_upsert(&out.changes).order(), seq(14));
    // Events 11 to 13, which the host sent before the delta arrived.
    let tool_call = log.fresh().unwrap();
    let events = [
        tool_started(turn, tool_call, 1).unwrap(),
        tool_status(tool_call, clauseo_protocol::ToolCallStatus::Completed),
        usage(5000).unwrap(),
    ];
    for event in events {
        let out = apply(&mut state, &log.push(event).unwrap()).unwrap();
        assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    }
    let out = answer(&mut log, &mut state, turn, message, "Done.");
    assert_eq!(log.events().last().unwrap().seq, seq(14));
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(state, fold(log.events()));
}

#[test]
fn long_text_split_at_inline_limit() {
    // An append onto an existing message.
    let (mut log, mut state, turn, message) = running();
    answer(&mut log, &mut state, turn, message, "Start: ");
    let long = multibyte(10 * 1024);
    let out = answer(&mut log, &mut state, turn, message, &long);
    let pieces = appends(&out.changes);
    assert_eq!(pieces.len(), out.changes.len());
    assert!(pieces.len() >= 3, "{}", pieces.len());
    check_pieces(&pieces, 7, &long);
    // Each piece but the last is full: one more char would not fit.
    for (_, _, piece) in &pieces[..pieces.len() - 1] {
        assert!(
            escaped_len(piece) + 4 > INLINE_LIMIT_BYTES,
            "{}",
            escaped_len(piece)
        );
    }

    // A long first text: an Upsert within the limit, then appends.
    let fresh = log.fresh().unwrap();
    let out = answer(&mut log, &mut state, turn, fresh, &long);
    let ViewEntry::AgentText(head) = first_upsert(&out.changes) else {
        panic!("not agent text");
    };
    assert!(matches!(out.changes[0], ViewChange::Upsert(_)));
    assert!(escaped_len(&head.text) <= INLINE_LIMIT_BYTES);
    assert!(!head.text.is_empty(), "{head:?}");
    let rest = long.strip_prefix(head.text.as_str()).unwrap();
    let pieces = appends(&out.changes);
    assert_eq!(pieces.len() + 1, out.changes.len());
    check_pieces(&pieces, u32::try_from(utf16_len(&head.text)).unwrap(), rest);
    assert_eq!(shown_text(&state, fresh), Some(long.clone()));

    // A long user message: Text blocks within the limit in all, then UserBlock appends.
    let blocks = [multibyte(3000), multibyte(3000), multibyte(200)];
    let user = log.fresh().unwrap();
    let event = UserMessage {
        turn: log.fresh().unwrap(),
        message: user,
        acp_message_id: None,
        content: blocks
            .iter()
            .map(|text| ContentBlock::Text { text: text.clone() })
            .collect(),
        delivery: Delivery::Queue,
    };
    let out = apply(&mut state, &log.push(event).unwrap()).unwrap();
    let ViewEntry::UserMessage(sent) = first_upsert(&out.changes) else {
        panic!("not a user message");
    };
    let heads: Vec<String> = sent
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.clone(),
            other => panic!("not text: {other:?}"),
        })
        .collect();
    let total: usize = heads.iter().map(|text| escaped_len(text)).sum();
    assert!(total <= INLINE_LIMIT_BYTES, "{total}");
    assert_eq!(heads[0], blocks[0]);
    let pieces = appends(&out.changes);
    assert!(!pieces.is_empty(), "{:?}", out.changes);
    for (index, (whole, head)) in blocks.iter().zip(&heads).enumerate() {
        let target = AppendTarget::UserBlock(u32::try_from(index).unwrap());
        let mine: Vec<_> = pieces.iter().filter(|p| p.0 == target).cloned().collect();
        let rest = whole.strip_prefix(head.as_str()).unwrap();
        check_pieces(&mine, u32::try_from(utf16_len(head)).unwrap(), rest);
    }
    let all = pieces
        .iter()
        .all(|p| matches!(p.0, AppendTarget::UserBlock(1 | 2)));
    assert!(all, "{pieces:?}");
}

#[test]
fn split_counts_escaped_bytes() {
    let (mut log, mut state, turn, message) = running();
    answer(&mut log, &mut state, turn, message, "a");
    // 1,000 control chars: 1,000 bytes, but 6,000 escaped.
    let control = "\u{1}".repeat(1000);
    assert!(control.len() < INLINE_LIMIT_BYTES);
    let out = answer(&mut log, &mut state, turn, message, &control);
    let pieces = appends(&out.changes);
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    check_pieces(&pieces, 1, &control);
    assert_eq!(pieces[0].2.len(), INLINE_LIMIT_BYTES / 6);

    // Quotes, newlines and control chars: 2, 2 and 6 bytes escaped.
    let mixed = "\"\n\u{2}".repeat(1000);
    let out = answer(&mut log, &mut state, turn, message, &mixed);
    let pieces = appends(&out.changes);
    assert!(pieces.len() >= 3, "{}", pieces.len());
    check_pieces(&pieces, 1001, &mixed);
    for (_, _, piece) in &pieces[..pieces.len() - 1] {
        assert!(escaped_len(piece) + 6 > INLINE_LIMIT_BYTES);
        assert!(piece.len() < INLINE_LIMIT_BYTES / 2);
    }
}

#[test]
fn deltas_in_flight_take_the_seqs_their_events_get() {
    let (mut log, _, turn, first) = running();
    let other = log.fresh().unwrap();
    for n in 0..10 {
        let chunk = format!("Thought {n}. ");
        log.push(text(turn, other, TextKind::Thought, chunk))
            .unwrap();
    }
    let mut state = fold(log.events());
    assert_eq!(state.seq(), Some(seq(13)));
    let second = log.fresh().unwrap();
    let opened = delta_at(
        &mut state,
        turn,
        first,
        TextKind::Answer,
        "Alpha é",
        seq(13),
    );
    assert_eq!(first_upsert(&opened.changes).order(), seq(14));
    // The host sends the first message's TextAppended (seq 14); the state has not applied it.
    let sent = log
        .push(text(turn, first, TextKind::Answer, "Alpha é".to_owned()))
        .unwrap();
    let opened = delta_at(
        &mut state,
        turn,
        second,
        TextKind::Answer,
        "Beta 😀",
        seq(14),
    );
    assert_eq!(first_upsert(&opened.changes).order(), seq(15));
    let out = apply(&mut state, &sent).unwrap();
    assert!(
        out.changes.is_empty() && out.anomalies.is_empty(),
        "{out:?}"
    );
    let sent = log
        .push(text(turn, second, TextKind::Answer, "Beta 😀".to_owned()))
        .unwrap();
    assert_eq!(sent.seq, seq(15));
    let out = apply(&mut state, &sent).unwrap();
    assert!(
        out.changes.is_empty() && out.anomalies.is_empty(),
        "{out:?}"
    );
    assert_eq!(state, fold(log.events()));
}

#[test]
fn event_extending_shown_text_appends_only_the_rest() {
    // A message only deltas opened: "Hi😀" is 4 UTF-16 units (6 bytes).
    let (mut log, mut state, turn, message) = running();
    delta(&mut state, turn, message, TextKind::Answer, "Hi😀");
    let out = answer(&mut log, &mut state, turn, message, "Hi😀 there");
    assert_eq!(
        out.changes,
        [ViewChange::AppendText {
            message_id: message,
            target: AppendTarget::AgentText,
            from_len: 4,
            text: " there".to_owned(),
        }]
    );
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(shown_text(&state, message).as_deref(), Some("Hi😀 there"));
    assert_eq!(state, fold(log.events()));

    // A durable message with un-durable text after it: "Hi😀 there é" is 12 units.
    delta(&mut state, turn, message, TextKind::Answer, " é");
    let out = answer(&mut log, &mut state, turn, message, " é and more");
    let rest = (AppendTarget::AgentText, 12, " and more".to_owned());
    assert_eq!(appends(&out.changes), [rest]);
    assert_eq!(out.changes.len(), 1);
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    assert_eq!(state, fold(log.events()));
}
