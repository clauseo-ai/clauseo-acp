//! The [`Gate`]: when the line reader stops, and that it is woken as soon as it may go on.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use futures::task::{ArcWake, waker};

use crate::line::{Gate, MAX_LINES_IN_FLIGHT};

/// A waker that remembers it was woken.
#[derive(Default)]
struct Woken(AtomicBool);

impl ArcWake for Woken {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.0.store(true, Ordering::SeqCst);
    }
}

/// Polls the gate as the reader does, with a fresh waker; returns the poll and the waker's flag.
fn poll(gate: &Gate) -> (Poll<()>, Arc<Woken>) {
    let woken = Arc::new(Woken::default());
    let waker = waker(Arc::clone(&woken));
    let poll = gate.poll_open(&mut Context::from_waker(&waker));
    (poll, woken)
}

/// The reader may pass on 64 counted lines the handler has not started, and stops at the 64th;
/// the handler starting one of them wakes it, and it may go on.
#[test]
fn reader_stops_at_64_lines_in_flight_and_wakes_when_one_is_handled() {
    let gate = Gate::default();
    for _ in 1..MAX_LINES_IN_FLIGHT {
        gate.line_read();
    }
    assert_eq!(poll(&gate).0, Poll::Ready(()), "63 lines in flight");
    gate.line_read();
    let (stopped, woken) = poll(&gate);
    assert_eq!(stopped, Poll::Pending, "64 lines in flight");
    assert!(!woken.0.load(Ordering::SeqCst));
    gate.line_handled();
    assert!(woken.0.load(Ordering::SeqCst), "the reader was not woken");
    assert_eq!(poll(&gate).0, Poll::Ready(()));
}

/// While events wait for room in the event channel the reader stops; the last of them finding
/// room wakes it, and it may go on.
#[test]
fn reader_wakes_when_the_last_waiting_event_finds_room() {
    let gate = Gate::default();
    gate.close();
    gate.close();
    assert_eq!(poll(&gate).0, Poll::Pending, "two events wait");
    gate.open();
    // The reader polls again and waits for the second event.
    let (stopped, woken) = poll(&gate);
    assert_eq!(stopped, Poll::Pending, "one event still waits");
    gate.open();
    assert!(woken.0.load(Ordering::SeqCst), "the reader was not woken");
    assert_eq!(poll(&gate).0, Poll::Ready(()));
}
