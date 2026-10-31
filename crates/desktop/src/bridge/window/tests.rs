//! The state machine alone, its time given as the time since the window opened.

use std::time::Duration;

use clauseo_protocol::{Ack, Cursor, StreamId, ThreadId, ViewEpoch};
use uuid::Uuid;

use super::*;

fn thread() -> StreamId {
    StreamId::Thread(ThreadId::new(Uuid::now_v7()).unwrap())
}

fn epoch() -> ViewEpoch {
    ViewEpoch::new(Uuid::now_v7()).unwrap()
}

fn at(epoch: ViewEpoch, cursor: u64) -> Position {
    (epoch, Cursor::new(cursor).unwrap())
}

/// The ack position of `stream` at `position`.
fn pos(stream: StreamId, (epoch, cursor): Position) -> Ack {
    Ack {
        stream,
        epoch,
        cursor,
    }
}

/// An event's text of `len` bytes, below the frame limit.
fn event(len: usize) -> String {
    format!("{{\"patches\":\"{}\"}}", "x".repeat(len - 15))
}

const SECOND: Duration = Duration::from_secs(1);

/// Forwards one event at `now` and returns its message count and whether it paused the window.
fn send_at(
    state: &mut WindowState,
    stream: StreamId,
    restart_id: u64,
    len: usize,
    now: Duration,
) -> (u32, bool) {
    match state.forward(stream, restart_id, &event(len), now) {
        Forward::Send { messages, pause } => (u32::try_from(messages.len()).unwrap(), pause),
        other => panic!("not sent: {other:?}"),
    }
}

/// [`send_at`] at time 0.
fn send(state: &mut WindowState, stream: StreamId, restart_id: u64, len: usize) -> (u32, bool) {
    send_at(state, stream, restart_id, len, Duration::ZERO)
}

#[test]
fn past_256_kb_unacknowledged_the_bridge_drops_then_resubscribes_after_the_last_ack() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let (one, two) = (thread(), thread());
    let e = epoch();
    let r1 = state.follow(one, None);
    let r2 = state.follow(two, Some(at(e, 4)));

    // 8000-byte events: the 33rd takes the window past 256 KB.
    let mut messages = 0;
    let mut paused_at = None;
    for i in 1..=40 {
        let (n, pause) = send(&mut state, one, r1, 8000);
        messages += n;
        if pause {
            paused_at = Some(i);
            break;
        }
    }
    assert_eq!(
        paused_at,
        Some(33),
        "{} bytes unacked",
        state.unacked_bytes()
    );
    assert!(state.is_paused());
    // Paused: nothing more is forwarded, from either stream.
    assert_eq!(state.forward(one, r1, &event(100), start), Forward::Drop);
    assert_eq!(state.forward(two, r2, &event(100), start), Forward::Drop);

    // Acks part of the way: still paused, nothing restarts.
    let acked = state.ack(&[pos(one, at(e, 10))], 20, start).unwrap();
    assert_eq!(acked.restarts, []);
    assert!(state.is_paused());

    // Acks catch up: every stream restarts after its last acknowledged position.
    let acked = state.ack(&[pos(one, at(e, 33))], messages, start).unwrap();
    assert_eq!(state.unacked_bytes(), 0);
    assert!(!state.is_paused());
    let after: Vec<_> = acked.restarts.iter().map(|r| (r.stream, r.after)).collect();
    let mut expected = vec![(one, Some(at(e, 33))), (two, Some(at(e, 4)))];
    expected.sort_by_key(|(stream, _)| *stream);
    assert_eq!(after, expected);
    // The old subscriptions' events are dropped; the new ones' are sent.
    assert_eq!(state.forward(one, r1, &event(100), start), Forward::Drop);
    for restart in &acked.restarts {
        assert!(state.is_current(restart.stream, restart.id));
        send(&mut state, restart.stream, restart.id, 100);
    }
}

#[test]
fn five_seconds_without_ack_progress_resubscribes() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let e = epoch();
    let restart_id = state.follow(stream, None);
    send(&mut state, stream, restart_id, 1000);
    send(&mut state, stream, restart_id, 1000);

    // Progress at 3 s moves the deadline.
    assert_eq!(state.tick(start + 3 * SECOND), []);
    state
        .ack(&[pos(stream, at(e, 1))], 1, start + 3 * SECOND)
        .unwrap();
    assert_eq!(state.tick(start + Duration::from_millis(7999)), []);
    // An ack with no new messages is not progress.
    state
        .ack(&[pos(stream, at(e, 1))], 1, start + 7 * SECOND)
        .unwrap();

    let restarts = state.tick(start + 8 * SECOND);
    assert_eq!(restarts.len(), 1);
    assert_eq!(restarts[0].stream, stream);
    assert_eq!(restarts[0].after, Some(at(e, 1)));
    assert!(!state.is_current(stream, restart_id));
    // It fires once per stall: not on every tick, and not again while acks have not covered
    // what was sent before it fired.
    assert_eq!(state.tick(start + 9 * SECOND), []);
    assert_eq!(state.tick(start + 13 * SECOND), []);
    assert_eq!(state.tick(start + 60 * SECOND), []);
}

/// The reviewer's case: one stream whose page never acks, the host answering each restart with a
/// 1 MB `Reset`, the clock moving 5 s at a time for 12 cycles. Before the fix every cycle added
/// another 1 MB (12.6 MB and 1,560 messages unacknowledged); now the watchdog fires once and the
/// window stays paused, holding two Resets: the larger of the cap and what was unacknowledged at
/// the fire (one Reset), plus the one event the restart sends.
#[test]
fn a_page_that_stops_acking_gets_one_event_more_per_stall_not_one_per_tick() {
    let mut state = WindowState::new();
    let stream = thread();
    let mut restart_id = state.follow(stream, None);
    let reset = format!("{{\"patches\":\"{}\"}}", "y".repeat(1_000_000));
    let one: u64 = frame::messages(&reset, 0)
        .unwrap()
        .iter()
        .map(|message| message.len() as u64)
        .sum();
    let mut now = Duration::ZERO;
    let mut fires = 0;
    for _ in 0..12 {
        // The host answers each subscription with the Reset; a paused window drops it.
        let _forwarded = state.forward(stream, restart_id, &reset, now);
        now += WATCHDOG;
        if let [restart] = state.tick(now).as_slice() {
            fires += 1;
            restart_id = restart.id;
        }
    }
    assert_eq!(fires, 1);
    assert!(state.is_paused());
    assert_eq!(state.unacked_bytes(), 2 * one);
    assert!(state.unacked_bytes() <= UNACKED_CAP.max(one) + one);
}

#[test]
fn a_lost_ack_needs_one_restart_and_the_watchdog_fires_again_once_acks_cover_it() {
    let mut state = WindowState::new();
    let stream = thread();
    let restart_id = state.follow(stream, None);
    let mut sent = 0;
    loop {
        let (n, pause) = send(&mut state, stream, restart_id, 8000);
        sent += n;
        if pause {
            break;
        }
    }
    // The page applied everything, but its ack was lost: one restart ends the pause.
    let restarts = state.tick(WATCHDOG);
    assert_eq!(restarts.len(), 1);
    assert!(!state.is_paused());
    // The restart's tail arrives (still over the cap, so it pauses again), and the page acks
    // everything it has: the pause ends as any pause does.
    let (n, pause) = send_at(&mut state, stream, restarts[0].id, 500, WATCHDOG);
    assert!(pause);
    sent += n;
    let acked = state.ack(&[], sent, WATCHDOG + SECOND).unwrap();
    assert_eq!(state.unacked_bytes(), 0);
    let [restart] = acked.restarts.as_slice() else {
        panic!("not one restart: {acked:?}");
    };
    // A later stall fires again.
    let later = WATCHDOG + 2 * SECOND;
    send_at(&mut state, stream, restart.id, 500, later);
    assert_eq!(state.tick(later + WATCHDOG).len(), 1);
}

#[test]
fn the_watchdog_waits_while_nothing_is_unacknowledged() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let restart_id = state.follow(stream, None);
    assert_eq!(state.tick(start + 60 * SECOND), []);
    let (messages, _) = send(&mut state, stream, restart_id, 500);
    state
        .ack(&[pos(stream, at(epoch(), 1))], messages, start)
        .unwrap();
    assert_eq!(state.tick(start + 120 * SECOND), []);
}

#[test]
fn a_replayed_cursor_is_a_no_op() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let e = epoch();
    let restart_id = state.follow(stream, None);
    let (messages, _) = send(&mut state, stream, restart_id, 500);
    state
        .ack(&[pos(stream, at(e, 9))], messages, start)
        .unwrap();
    // Acks that crossed: an older cursor and message count change nothing.
    state.ack(&[pos(stream, at(e, 5))], 0, start).unwrap();
    state
        .ack(&[pos(stream, at(e, 9))], messages, start)
        .unwrap();
    let restarts = state.restart_all(start);
    assert_eq!(restarts[0].after, Some(at(e, 9)));
    // A new epoch replaces the position, whatever its cursor.
    let newer = epoch();
    state
        .ack(&[pos(stream, at(newer, 2))], messages, start)
        .unwrap();
    assert_eq!(state.restart_all(start)[0].after, Some(at(newer, 2)));
}

#[test]
fn an_ack_of_more_messages_than_sent_or_of_a_stream_twice_is_refused_whole() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let e = epoch();
    let restart_id = state.follow(stream, Some(at(e, 1)));
    send(&mut state, stream, restart_id, 500);
    assert_eq!(
        state.ack(&[pos(stream, at(e, 2))], 2, start),
        Err(AckError::TooManyMessages)
    );
    let twice = [pos(stream, at(e, 3)), pos(stream, at(e, 4))];
    assert_eq!(
        state.ack(&twice, 1, start),
        Err(AckError::RepeatedStream(stream))
    );
    // Neither moved anything.
    assert_ne!(state.unacked_bytes(), 0);
    assert_eq!(state.restart_all(start)[0].after, Some(at(e, 1)));
}

#[test]
fn a_messages_only_ack_retires_bytes_and_unfollowed_streams_are_left_out() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let (stream, gone) = (thread(), thread());
    let e = epoch();
    let restart_id = state.follow(stream, None);
    let (messages, _) = send(&mut state, stream, restart_id, 500);
    let acked = state.ack(&[], messages, start).unwrap();
    assert_eq!(acked, Acked::default());
    assert_eq!(state.unacked_bytes(), 0);

    // A position of a stream the window does not follow (an ack that crossed an unsubscribe)
    // is not passed on; the followed one is.
    let positions = [pos(gone, at(e, 7)), pos(stream, at(e, 2))];
    let acked = state.ack(&positions, messages, start).unwrap();
    assert_eq!(acked.positions, [pos(stream, at(e, 2))]);
}

#[test]
fn an_unfollowed_stream_is_dropped() {
    let mut state = WindowState::new();
    let stream = thread();
    let restart_id = state.follow(stream, None);
    assert!(state.unfollow(stream));
    assert_eq!(
        state.forward(stream, restart_id, &event(100), Duration::ZERO),
        Forward::Drop
    );
}

#[test]
fn fragments_count_as_one_message_each() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let restart_id = state.follow(stream, None);
    let big = format!("{{\"patches\":\"{}\"}}", "y".repeat(20_000));
    let Forward::Send { messages, .. } = state.forward(stream, restart_id, &big, start) else {
        panic!("not sent");
    };
    assert_eq!(messages.len(), 3);
    let total: usize = messages.iter().map(Frame::len).sum();
    assert_eq!(state.unacked_bytes(), total as u64);
    state.ack(&[pos(stream, at(epoch(), 1))], 2, start).unwrap();
    assert_eq!(state.unacked_bytes(), messages[2].len() as u64);
}

#[test]
fn the_watchdog_waits_from_the_first_send_after_an_idle_spell() {
    let mut state = WindowState::new();
    let stream = thread();
    let restart_id = state.follow(stream, None);
    assert_eq!(state.tick(60 * SECOND), []);
    let sent = state.forward(stream, restart_id, &event(500), 60 * SECOND);
    assert!(matches!(sent, Forward::Send { .. }));
    assert_eq!(state.tick(Duration::from_millis(60_500)), []);
    assert_eq!(state.tick(Duration::from_millis(64_999)), []);
    assert_eq!(state.tick(65 * SECOND).len(), 1);
}

#[test]
fn a_failed_restart_is_retried_on_the_clock_with_a_doubling_wait() {
    let mut state = WindowState::new();
    let stream = thread();
    let e = epoch();
    let first = state.follow(stream, Some(at(e, 3)));
    let failed_at = 10 * SECOND;
    assert!(state.failed(stream, first, failed_at));
    let due = failed_at + RETRY_FIRST;
    assert_eq!(state.retry_at(stream), Some(due));
    assert_eq!(
        state.tick(due.checked_sub(Duration::from_millis(1)).unwrap()),
        []
    );
    let restarts = state.tick(due);
    assert_eq!(restarts.len(), 1);
    assert_eq!(restarts[0].after, Some(at(e, 3)));
    // Not again while that try is out.
    assert_eq!(state.tick(due + SECOND), []);

    // Each failure doubles the wait, up to RETRY_MAX.
    let (mut now, mut restart_id) = (due, restarts[0].id);
    let mut waits = Vec::new();
    for _ in 0..6 {
        assert!(state.failed(stream, restart_id, now));
        let next = state.retry_at(stream).unwrap();
        waits.push(next.checked_sub(now).unwrap().as_secs());
        now = next;
        restart_id = state.tick(now)[0].id;
    }
    assert_eq!(waits, [10, 20, 40, 60, 60, 60]);
    // A failure of a subscription that is no longer current tells nobody.
    assert!(!state.failed(stream, first, now));
    // Started: nothing more is due.
    state.started(stream, restart_id);
    assert_eq!(state.retry_at(stream), None);
    assert_eq!(state.tick(now + RETRY_MAX), []);
}

#[test]
fn nothing_retries_while_paused_and_the_pause_end_restarts_a_failed_stream_once() {
    let mut state = WindowState::new();
    let (busy, broken) = (thread(), thread());
    let busy_id = state.follow(busy, None);
    let broken_id = state.follow(broken, None);
    assert!(state.failed(broken, broken_id, Duration::ZERO));
    let mut sent = 0;
    loop {
        let (n, pause) = send_at(&mut state, busy, busy_id, 8000, 2 * SECOND);
        sent += n;
        if pause {
            break;
        }
    }
    // The retry is due at 5 s, but the window is paused (and the watchdog is due at 7 s).
    assert_eq!(state.tick(RETRY_FIRST), []);
    let acked = state.ack(&[], sent, 6 * SECOND).unwrap();
    assert_eq!(acked.restarts.len(), 2);
    // The pause's end restarted it; the clock does not a second time.
    assert_eq!(state.retry_at(broken), Some(6 * SECOND + RETRY_FIRST));
    assert_eq!(state.tick(7 * SECOND), []);
}
