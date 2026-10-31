//! Where a message a delta opened ends up when its order turns out wrong, and the view
//! snapshot's window at exactly its size.

use clauseo_protocol::{AppendTarget, MessageId, Notice, NoticeLevel, Seq, TextKind, ViewEntry};
use clauseo_reducer::{Anomaly, VIEW_SNAPSHOT_ENTRIES, ViewChange, apply, view_snapshot};

use crate::fixtures::{Log, start_turn, text, thread_created};
use crate::helpers::{appends, delta_at, fold, seq, shown_text};

/// The order of each agent text `Upsert` of `message` among `changes`.
#[cfg(test)]
fn text_upsert_orders(changes: &[ViewChange], message: MessageId) -> Vec<Seq> {
    changes
        .iter()
        .filter_map(|change| match change {
            ViewChange::Upsert(entry) => match &**entry {
                ViewEntry::AgentText(text) if text.message == message => Some(text.order),
                _ => None,
            },
            ViewChange::AppendText { .. } | ViewChange::Remove(_) => None,
        })
        .collect()
}

/// The key and order of each of `message`'s entries.
#[cfg(test)]
fn entries_of(state: &clauseo_reducer::StreamState, message: MessageId) -> Vec<(Seq, Seq)> {
    state
        .entries()
        .iter()
        .filter_map(|(key, entry)| match entry {
            ViewEntry::AgentText(text) if text.message == message => Some((*key, text.order)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_message_corrected_to_its_first_text_appended_is_shown_there_and_grows_there() {
    let mut log = Log::thread(24).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Summarise the lease.").unwrap();
    let mut state = fold(log.events());
    let message: MessageId = log.fresh().unwrap();
    // A pending head past the next seq breaks the host contract: the message opens at 7.
    let opened = delta_at(&mut state, turn, message, TextKind::Answer, "Hello", seq(6));
    assert_eq!(text_upsert_orders(&opened.changes, message), vec![seq(7)]);

    let event = text(turn, message, TextKind::Answer, "Hello".to_owned());
    let envelope = log.push(event).unwrap();
    assert_eq!(envelope.seq, seq(4));
    let out = apply(&mut state, &envelope).unwrap();
    let corrected = Anomaly::OrderCorrected {
        message,
        from: seq(7),
        to: seq(4),
    };
    assert_eq!(out.anomalies, vec![corrected]);
    assert_eq!(text_upsert_orders(&out.changes, message), vec![seq(4)]);
    assert_eq!(entries_of(&state, message), vec![(seq(4), seq(4))]);

    // Later text of the message appends to it where it now is.
    let pending = state.seq().unwrap();
    let more = delta_at(
        &mut state,
        turn,
        message,
        TextKind::Answer,
        " world",
        pending,
    );
    assert!(more.anomalies.is_empty(), "{:?}", more.anomalies);
    let append = (AppendTarget::AgentText, 5, " world".to_owned());
    assert_eq!(appends(&more.changes), vec![append]);
    assert_eq!(entries_of(&state, message), vec![(seq(4), seq(4))]);
    assert_eq!(shown_text(&state, message).as_deref(), Some("Hello world"));
}

#[test]
fn a_message_an_event_displaces_is_shown_at_its_new_order() {
    let mut log = Log::thread(24).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Summarise the lease.").unwrap();
    let mut state = fold(log.events());
    let message: MessageId = log.fresh().unwrap();
    let pending = state.seq().unwrap();
    delta_at(
        &mut state,
        turn,
        message,
        TextKind::Answer,
        "Hello",
        pending,
    );

    // A notice takes the seq the message's first `TextAppended` would have had.
    let notice = Notice {
        level: NoticeLevel::Info,
        text: "Context is nearly full.".to_owned(),
    };
    let envelope = log.push(notice).unwrap();
    assert_eq!(envelope.seq, seq(4));
    let out = apply(&mut state, &envelope).unwrap();
    let corrected = Anomaly::OrderCorrected {
        message,
        from: seq(4),
        to: seq(5),
    };
    assert_eq!(out.anomalies, vec![corrected]);
    assert_eq!(text_upsert_orders(&out.changes, message), vec![seq(5)]);
    assert_eq!(entries_of(&state, message), vec![(seq(5), seq(5))]);
}

/// A thread with `n` notices: `n` entries besides the header.
#[cfg(test)]
fn thread_of_notices(n: usize) -> Log {
    let mut log = Log::thread(24).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    for at in 0..n {
        let notice = Notice {
            level: NoticeLevel::Info,
            text: format!("Notice {at}."),
        };
        log.push(notice).unwrap();
    }
    log
}

#[test]
fn view_snapshot_has_older_only_past_its_window() {
    let full = fold(thread_of_notices(VIEW_SNAPSHOT_ENTRIES).events());
    let window = view_snapshot(&full);
    assert_eq!(window.entries.len(), VIEW_SNAPSHOT_ENTRIES);
    assert!(!window.has_older);

    let past = fold(thread_of_notices(VIEW_SNAPSHOT_ENTRIES + 1).events());
    let window = view_snapshot(&past);
    assert_eq!(window.entries.len(), VIEW_SNAPSHOT_ENTRIES);
    assert!(window.has_older);
}
