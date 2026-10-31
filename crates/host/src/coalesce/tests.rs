use std::time::Duration;

use clauseo_protocol::{MessageId, TextAppended, TextKind, TurnId};

use super::{DISPLAY_BATCH, DURABLE_BATCH, DURABLE_FLUSH_BYTES, Pushed, TextIn, ThreadText};

fn v7(n: u8) -> uuid::Uuid {
    uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &[n; 10]).into_uuid()
}

fn turn() -> TurnId {
    TurnId::new(v7(1)).unwrap()
}

fn message(n: u8) -> MessageId {
    MessageId::new(v7(10 + n)).unwrap()
}

fn delta(n: u8, text: &str) -> TextIn {
    TextIn {
        turn: turn(),
        message: message(n),
        acp_message_id: None,
        kind: if n == 0 {
            TextKind::Thought
        } else {
            TextKind::Answer
        },
        text: text.to_owned(),
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// What a display flush shows, as (message number, text).
fn shown(text: &mut ThreadText) -> Vec<(MessageId, String)> {
    let mut out = Vec::new();
    text.flush_display(|piece| {
        out.push((piece.message, piece.text.to_owned()));
        Ok::<(), ()>(())
    })
    .unwrap();
    out
}

fn texts(events: &[TextAppended]) -> Vec<(MessageId, &str)> {
    events
        .iter()
        .map(|event| (event.message, event.text.as_str()))
        .collect()
}

#[test]
fn durable_flush_in_first_delta_order() {
    let mut text = ThreadText::new();
    assert!(text.is_new_message(message(1)));
    let first = text.push(ms(0), delta(1, "a"));
    assert_eq!(
        first,
        Pushed {
            display_at: Some(DISPLAY_BATCH),
            durable_at: Some(DURABLE_BATCH),
            full: false
        }
    );
    assert!(!text.is_new_message(message(1)));
    assert_eq!(text.push(ms(1), delta(0, "b")), Pushed::default());
    assert_eq!(text.push(ms(2), delta(1, "c")), Pushed::default());
    assert_eq!(
        text.push(ms(3), delta(2, "")),
        Pushed::default(),
        "an empty delta is nothing"
    );

    let events = text.flush_durable(|_| Ok::<(), ()>(())).unwrap();
    assert_eq!(texts(&events), vec![(message(1), "ac"), (message(0), "b")]);
    let kinds: Vec<TextKind> = events.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, vec![TextKind::Answer, TextKind::Thought]);
    assert!(!text.has_durable() && !text.has_display());
    assert!(text.is_new_message(message(1)));

    // The next delta arms both timers again.
    let again = text.push(ms(300), delta(0, "d"));
    assert_eq!(again.display_at, Some(ms(300) + DISPLAY_BATCH));
    assert_eq!(again.durable_at, Some(ms(300) + DURABLE_BATCH));
}

#[test]
fn flush_before_non_text_event() {
    let mut text = ThreadText::new();
    let _pushed = text.push(ms(0), delta(1, "ab"));
    let _pushed = text.push(ms(1), delta(2, "x"));
    // The display timer fires: both messages show, merged, in first-delta order.
    assert_eq!(
        shown(&mut text),
        vec![(message(1), "ab".to_owned()), (message(2), "x".to_owned())]
    );
    assert!(!text.has_display() && text.has_durable());
    assert!(shown(&mut text).is_empty(), "nothing is shown twice");

    // More text arrives, then a non-text event needs the durable flush. The flush shows the
    // unshown text first, and only then gives the events; the caller sends the event after them.
    let display_again = text.push(ms(5), delta(1, "cd"));
    assert_eq!(display_again.display_at, Some(ms(5) + DISPLAY_BATCH));
    assert_eq!(
        display_again.durable_at, None,
        "the durable timer is still armed"
    );
    let mut order = Vec::new();
    let events = text
        .flush_durable(|piece| {
            order.push(format!("display {}", piece.text));
            Ok::<(), ()>(())
        })
        .unwrap();
    order.extend(events.iter().map(|event| format!("durable {}", event.text)));
    order.push("event".to_owned());
    assert_eq!(
        order,
        vec!["display cd", "durable abcd", "durable x", "event"]
    );

    // When showing fails, nothing is made durable and the text stays held.
    let _pushed = text.push(ms(10), delta(1, "e"));
    assert_eq!(
        text.flush_durable(|_| Err("reducer refused")),
        Err("reducer refused")
    );
    assert!(text.has_display() && text.has_durable());
    let events = text.flush_durable(|_| Ok::<(), ()>(())).unwrap();
    assert_eq!(texts(&events), vec![(message(1), "e")]);
}

#[test]
fn full_batch_asks_for_a_flush() {
    let mut text = ThreadText::new();
    let half = "z".repeat(DURABLE_FLUSH_BYTES / 2);
    assert!(!text.push(ms(0), delta(1, &half)).full);
    assert!(text.push(ms(1), delta(2, &half)).full);
    let events = text.flush_durable(|_| Ok::<(), ()>(())).unwrap();
    assert_eq!(events.len(), 2);
    assert!(!text.push(ms(2), delta(1, "a")).full);
}
