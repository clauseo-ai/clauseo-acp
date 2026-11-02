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

/// What a display flush at `now` shows, as (message number, text).
fn shown(text: &mut ThreadText, now: Duration) -> Vec<(MessageId, String)> {
    let mut out = Vec::new();
    text.flush_display(now, |piece| {
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
            display_now: true,
            display_at: None,
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

    let events = text.flush_durable(ms(3), |_| Ok::<(), ()>(())).unwrap();
    assert_eq!(texts(&events), vec![(message(1), "ac"), (message(0), "b")]);
    let kinds: Vec<TextKind> = events.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, vec![TextKind::Answer, TextKind::Thought]);
    assert!(!text.has_durable() && !text.has_display());
    assert!(text.is_new_message(message(1)));

    // The next delta comes after a quiet window: it is shown at once and arms the durable timer.
    let again = text.push(ms(300), delta(0, "d"));
    assert_eq!(
        again,
        Pushed {
            display_now: true,
            display_at: None,
            durable_at: Some(ms(300) + DURABLE_BATCH),
            full: false
        }
    );
}

#[test]
fn flush_before_non_text_event() {
    let mut text = ThreadText::new();
    let _pushed = text.push(ms(0), delta(1, "ab"));
    let _pushed = text.push(ms(1), delta(2, "x"));
    // The leading flush at 1 ms shows both messages, merged, in first-delta order.
    assert_eq!(
        shown(&mut text, ms(1)),
        vec![(message(1), "ab".to_owned()), (message(2), "x".to_owned())]
    );
    assert!(!text.has_display() && text.has_durable());
    assert!(shown(&mut text, ms(1)).is_empty(), "nothing is shown twice");

    // More text arrives 4 ms after that flush, inside its window: the display timer waits for
    // the window's end, at 17 ms. Then a non-text event needs the durable flush. The flush shows
    // the unshown text first, and only then gives the events; the caller sends the event after.
    let display_again = text.push(ms(5), delta(1, "cd"));
    assert_eq!(display_again.display_at, Some(ms(1) + DISPLAY_BATCH));
    assert!(!display_again.display_now);
    assert_eq!(
        display_again.durable_at, None,
        "the durable timer is still armed"
    );
    let mut order = Vec::new();
    let events = text
        .flush_durable(ms(5), |piece| {
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
        text.flush_durable(ms(10), |_| Err("reducer refused")),
        Err("reducer refused")
    );
    assert!(text.has_display() && text.has_durable());
    let events = text.flush_durable(ms(10), |_| Ok::<(), ()>(())).unwrap();
    assert_eq!(texts(&events), vec![(message(1), "e")]);
}

#[test]
fn full_batch_asks_for_a_flush() {
    let mut text = ThreadText::new();
    let half = "z".repeat(DURABLE_FLUSH_BYTES / 2);
    assert!(!text.push(ms(0), delta(1, &half)).full);
    assert!(text.push(ms(1), delta(2, &half)).full);
    let events = text.flush_durable(ms(2), |_| Ok::<(), ()>(())).unwrap();
    assert_eq!(events.len(), 2);
    assert!(!text.push(ms(3), delta(1, "a")).full);
}

#[test]
fn first_delta_after_a_quiet_window_shows_now() {
    let mut text = ThreadText::new();
    // A fresh thread has never flushed: its first delta is shown at once, with no timer.
    let fresh = text.push(ms(0), delta(1, "a"));
    assert!(fresh.display_now && fresh.display_at.is_none());
    assert_eq!(shown(&mut text, ms(0)), vec![(message(1), "a".to_owned())]);

    // A delta 16 ms after the last display flush is outside its window: shown at once.
    let after = text.push(ms(16), delta(1, "b"));
    assert!(after.display_now && after.display_at.is_none());
    assert_eq!(shown(&mut text, ms(16)), vec![(message(1), "b".to_owned())]);
}

#[test]
fn delta_inside_the_window_waits_for_its_end() {
    let mut text = ThreadText::new();
    let _pushed = text.push(ms(100), delta(1, "a"));
    assert_eq!(shown(&mut text, ms(100)).len(), 1);

    // 5 ms after the flush: the timer is set for the window's end, 116 ms, not now + 16.
    let inside = text.push(ms(105), delta(1, "b"));
    assert_eq!(
        inside,
        Pushed {
            display_now: false,
            display_at: Some(ms(116)),
            durable_at: None,
            full: false
        }
    );
    assert!(text.display_due(ms(116)));
    assert!(
        !text.display_due(ms(115)),
        "the window is still open at 115 ms"
    );
}

#[test]
fn burst_in_a_window_is_one_leading_and_one_trailing_flush() {
    let mut text = ThreadText::new();
    let mut flushes = Vec::new();

    // The first delta of the burst is shown at once: the leading flush.
    let leading = text.push(ms(0), delta(1, "a"));
    assert!(leading.display_now);
    flushes.push((ms(0), shown(&mut text, ms(0))));

    // Fifteen more deltas inside the window. Only the first arms the timer; the rest are held.
    let mut armed = Vec::new();
    for at in 1..16 {
        let pushed = text.push(ms(at), delta(1, "b"));
        assert!(!pushed.display_now, "no second leading flush at {at} ms");
        if let Some(deadline) = pushed.display_at {
            armed.push(deadline);
        }
    }
    assert_eq!(
        armed,
        vec![ms(16)],
        "one trailing timer, at the window's end"
    );

    // Timers that fire before the window's end do nothing; the one at 16 ms flushes the burst.
    for at in 1..16 {
        assert!(!text.display_due(ms(at)));
    }
    assert!(text.display_due(ms(16)));
    flushes.push((ms(16), shown(&mut text, ms(16))));
    assert!(!text.display_due(ms(16)), "nothing is left to flush");

    assert_eq!(
        flushes,
        vec![
            (ms(0), vec![(message(1), "a".to_owned())]),
            (ms(16), vec![(message(1), "b".repeat(15))]),
        ]
    );
}

#[test]
fn durable_flush_counts_as_a_display_flush() {
    let mut text = ThreadText::new();
    // The first delta is shown at once; the second, inside the window, is held for the timer.
    let _pushed = text.push(ms(0), delta(1, "a"));
    let _pushed = text.push(ms(4), delta(1, "b"));
    // A durable flush at 100 ms shows the held text, so it is the last display flush.
    let events = text.flush_durable(ms(100), |_| Ok::<(), ()>(())).unwrap();
    assert_eq!(texts(&events), vec![(message(1), "ab")]);

    // 5 ms after that durable flush the window is still open: the timer waits for 116 ms. Had the
    // durable flush not counted, this delta would be shown at once.
    let pushed = text.push(ms(105), delta(1, "c"));
    assert!(!pushed.display_now);
    assert_eq!(pushed.display_at, Some(ms(116)));
}

#[test]
fn overtaken_display_timer_shows_nothing() {
    let mut text = ThreadText::new();
    let _pushed = text.push(ms(0), delta(1, "a"));
    assert_eq!(shown(&mut text, ms(0)).len(), 1);
    // A delta at 4 ms arms the timer for 16 ms.
    assert_eq!(text.push(ms(4), delta(1, "b")).display_at, Some(ms(16)));
    // A durable flush at 10 ms shows it first, so the window has moved to 26 ms.
    let _events = text.flush_durable(ms(10), |_| Ok::<(), ()>(())).unwrap();
    assert_eq!(text.push(ms(12), delta(1, "c")).display_at, Some(ms(26)));

    // The 16 ms timer, armed before the durable flush, finds the window still open.
    assert!(!text.display_due(ms(16)));
    assert!(text.display_due(ms(26)));
    assert!(
        !ThreadText::new().display_due(ms(100)),
        "nothing held, nothing due"
    );
}
