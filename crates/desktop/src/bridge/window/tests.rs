//! The state machine alone, its time given as the time since the window opened.

use std::time::Duration;

use clauseo_protocol::{Cursor, StreamId, ThreadId, ViewEpoch};
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

/// An event's text of `len` bytes, below the frame limit.
fn event(len: usize) -> String {
    format!("{{\"patches\":\"{}\"}}", "x".repeat(len - 15))
}

/// Forwards one event and returns its frame count and whether it paused the window.
fn send(state: &mut WindowState, stream: StreamId, generation: u64, len: usize) -> (u32, bool) {
    match state.forward(stream, generation, &event(len), Duration::ZERO) {
        Forward::Send { frames, pause } => (u32::try_from(frames.len()).unwrap(), pause),
        other => panic!("not sent: {other:?}"),
    }
}

#[test]
fn past_256_kb_unacknowledged_the_bridge_drops_then_resubscribes_after_the_last_ack() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let (one, two) = (thread(), thread());
    let e = epoch();
    let g1 = state.follow(one, None);
    let g2 = state.follow(two, Some(at(e, 4)));

    // 8000-byte events: the 33rd takes the window past 256 KB.
    let mut frames = 0;
    let mut paused_at = None;
    for i in 1..=40 {
        let (n, pause) = send(&mut state, one, g1, 8000);
        frames += n;
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
    assert_eq!(
        state.forward(one, g1, &event(100), Duration::ZERO),
        Forward::Drop
    );
    assert_eq!(
        state.forward(two, g2, &event(100), Duration::ZERO),
        Forward::Drop
    );

    // Acks part of the way: still paused, nothing restarts.
    let restarts = state.ack(one, at(e, 10), 20, start).unwrap();
    assert_eq!(restarts, []);
    assert!(state.is_paused());

    // Acks catch up: every stream restarts after its last acknowledged position.
    let restarts = state.ack(one, at(e, 33), frames, start).unwrap();
    assert_eq!(state.unacked_bytes(), 0);
    assert!(!state.is_paused());
    let after: Vec<_> = restarts.iter().map(|r| (r.stream, r.after)).collect();
    let mut expected = vec![(one, Some(at(e, 33))), (two, Some(at(e, 4)))];
    expected.sort_by_key(|(stream, _)| *stream);
    assert_eq!(after, expected);
    // The old subscriptions' events are dropped; the new ones' are sent.
    assert_eq!(
        state.forward(one, g1, &event(100), Duration::ZERO),
        Forward::Drop
    );
    for restart in &restarts {
        assert!(state.is_current(restart.stream, restart.generation));
        send(&mut state, restart.stream, restart.generation, 100);
    }
}

#[test]
fn five_seconds_without_ack_progress_resubscribes() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let e = epoch();
    let generation = state.follow(stream, None);
    send(&mut state, stream, generation, 1000);
    send(&mut state, stream, generation, 1000);

    // Progress at 3 s moves the deadline.
    assert_eq!(state.tick(start + Duration::from_secs(3)), []);
    state
        .ack(stream, at(e, 1), 1, start + Duration::from_secs(3))
        .unwrap();
    assert_eq!(state.tick(start + Duration::from_millis(7999)), []);
    // An ack with no new frames is not progress.
    state
        .ack(stream, at(e, 1), 1, start + Duration::from_secs(7))
        .unwrap();

    let restarts = state.tick(start + Duration::from_secs(8));
    assert_eq!(restarts.len(), 1);
    assert_eq!(restarts[0].stream, stream);
    assert_eq!(restarts[0].after, Some(at(e, 1)));
    assert!(!state.is_current(stream, generation));
    // It fires once per 5 s stall, not on every tick.
    assert_eq!(state.tick(start + Duration::from_secs(9)), []);
    assert_eq!(state.tick(start + Duration::from_secs(13)).len(), 1);
}

#[test]
fn the_watchdog_waits_while_nothing_is_unacknowledged() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let generation = state.follow(stream, None);
    assert_eq!(state.tick(start + Duration::from_secs(60)), []);
    let (frames, _) = send(&mut state, stream, generation, 500);
    state.ack(stream, at(epoch(), 1), frames, start).unwrap();
    assert_eq!(state.tick(start + Duration::from_secs(120)), []);
}

#[test]
fn the_watchdog_ends_a_pause_whose_ack_was_lost() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let generation = state.follow(stream, None);
    while !send(&mut state, stream, generation, 8000).1 {}
    let restarts = state.tick(start + WATCHDOG);
    assert_eq!(restarts.len(), 1);
    assert!(!state.is_paused());
}

#[test]
fn a_replayed_cursor_is_a_no_op() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let e = epoch();
    let generation = state.follow(stream, None);
    let (frames, _) = send(&mut state, stream, generation, 500);
    state.ack(stream, at(e, 9), frames, start).unwrap();
    // Acks that crossed: an older cursor and frame count change nothing.
    state.ack(stream, at(e, 5), 0, start).unwrap();
    state.ack(stream, at(e, 9), frames, start).unwrap();
    let restarts = state.restart_all();
    assert_eq!(restarts[0].after, Some(at(e, 9)));
    // A new epoch replaces the position, whatever its cursor.
    let newer = epoch();
    state.ack(stream, at(newer, 2), frames, start).unwrap();
    assert_eq!(state.restart_all()[0].after, Some(at(newer, 2)));
}

#[test]
fn an_ack_of_more_frames_than_sent_is_refused() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let generation = state.follow(stream, None);
    send(&mut state, stream, generation, 500);
    assert_eq!(
        state.ack(stream, at(epoch(), 1), 2, start),
        Err(AckError::TooManyFrames)
    );
}

#[test]
fn an_unfollowed_stream_is_dropped() {
    let mut state = WindowState::new();
    let stream = thread();
    let generation = state.follow(stream, None);
    assert!(state.unfollow(stream));
    assert_eq!(
        state.forward(stream, generation, &event(100), Duration::ZERO),
        Forward::Drop
    );
}

#[test]
fn fragments_count_as_one_frame_each() {
    let start = Duration::ZERO;
    let mut state = WindowState::new();
    let stream = thread();
    let generation = state.follow(stream, None);
    let big = format!("{{\"patches\":\"{}\"}}", "y".repeat(20_000));
    let Forward::Send { frames, .. } = state.forward(stream, generation, &big, Duration::ZERO)
    else {
        panic!("not sent");
    };
    assert_eq!(frames.len(), 3);
    let total: usize = frames.iter().map(Frame::len).sum();
    assert_eq!(state.unacked_bytes(), total as u64);
    state.ack(stream, at(epoch(), 1), 2, start).unwrap();
    assert_eq!(state.unacked_bytes(), frames[2].len() as u64);
}

#[test]
fn the_watchdog_waits_from_the_first_send_after_an_idle_spell() {
    let mut state = WindowState::new();
    let stream = thread();
    let generation = state.follow(stream, None);
    assert_eq!(state.tick(Duration::from_secs(60)), []);
    let sent = state.forward(stream, generation, &event(500), Duration::from_secs(60));
    assert!(matches!(sent, Forward::Send { .. }));
    assert_eq!(state.tick(Duration::from_millis(60_500)), []);
    assert_eq!(state.tick(Duration::from_millis(64_999)), []);
    assert_eq!(state.tick(Duration::from_secs(65)).len(), 1);
}
