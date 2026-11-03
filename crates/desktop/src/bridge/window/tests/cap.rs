//! Where the pause lands, to the byte: the window pauses after the event that takes its
//! unacknowledged bytes past [`UNACKED_CAP`], never at the cap itself, and never part way
//! through an event's fragments. A message counts its bytes, so a character of several bytes
//! counts every one of them, wherever the cap falls inside it.

use super::*;

const CAP: usize = 256 * 1024;

/// Sends single-message events of exactly `total` bytes in all, none pausing the window.
fn fill(state: &mut WindowState, stream: StreamId, restart_id: u64, total: usize) {
    assert!(total <= CAP, "fill stays at or below the cap: {total}");
    let mut left = total;
    while left > 0 {
        // Every event is at least 15 bytes and below the frame limit: one message each.
        let len = if left > 8000 + 15 { 8000 } else { left };
        let (n, pause) = send(state, stream, restart_id, len);
        assert_eq!((n, pause), (1, false), "{left} bytes left of {total}");
        left -= len;
    }
    assert_eq!(state.unacked_bytes(), total as u64);
}

/// Forwards `json` and returns its messages' lengths and whether it paused the window.
fn send_json(
    state: &mut WindowState,
    stream: StreamId,
    restart_id: u64,
    json: &str,
) -> (Vec<usize>, bool) {
    match state.forward(stream, restart_id, json, Duration::ZERO) {
        Forward::Send { messages, pause } => (messages.iter().map(Frame::len).collect(), pause),
        other => panic!("not sent: {other:?}"),
    }
}

/// `UNACKED_CAP` is the 256 KiB this file's tests are built around.
#[test]
fn the_cap_is_256_kib() {
    assert_eq!(UNACKED_CAP, CAP as u64);
}

/// Totals of cap − 1 and cap leave the window running, and the next event, of the fewest bytes
/// an event has here, pauses it; a total of cap + 1 pauses it with the event that reaches it.
#[test]
fn the_window_pauses_only_past_the_cap_to_the_byte() {
    for (total, paused) in [(CAP - 1, false), (CAP, false), (CAP + 1, true)] {
        let mut state = WindowState::new();
        let stream = thread();
        let r = state.follow(stream, None);
        // All but the last event, then the last one, of 100 bytes, ending on `total`.
        fill(&mut state, stream, r, total - 100);
        let (_, pause) = send(&mut state, stream, r, 100);
        assert_eq!(
            (pause, state.is_paused()),
            (paused, paused),
            "total {total}"
        );
        assert_eq!(state.unacked_bytes(), total as u64);
        if !paused {
            let (_, pause) = send(&mut state, stream, r, 15);
            assert!(pause, "total {} is past the cap", total + 15);
            assert_eq!(state.unacked_bytes(), (total + 15) as u64);
        }
    }
}

/// An event in fragments pauses the window after its last fragment, all of them sent, whether a
/// fragment ends one byte before the cap, on it or one byte past it: the pause depends on the
/// event's total alone. The event's last fragment ending on cap − 1 or cap does not pause it.
#[test]
fn an_event_in_fragments_pauses_after_its_last_fragment_wherever_its_fragments_end() {
    let json = event(20_000);
    // A fresh window's first fragmented event takes fragment id 0.
    let lens: Vec<usize> = frame::messages(&json, 0)
        .unwrap()
        .iter()
        .map(Frame::len)
        .collect();
    assert_eq!(lens.len(), 3, "{lens:?}");
    let total: usize = lens.iter().sum();
    for fragments in 1..=lens.len() {
        let ends_at: usize = lens.iter().take(fragments).sum();
        for offset in [-1_isize, 0, 1] {
            // Fragment `fragments` ends `offset` bytes from the cap.
            let before = CAP.checked_add_signed(offset).unwrap() - ends_at;
            let mut state = WindowState::new();
            let stream = thread();
            let r = state.follow(stream, None);
            fill(&mut state, stream, r, before);
            let (sent, pause) = send_json(&mut state, stream, r, &json);
            let past = before + total > CAP;
            assert_eq!(sent, lens, "every fragment goes, paused or not");
            assert_eq!(
                (pause, state.is_paused()),
                (past, past),
                "fragment {fragments} ending at cap{offset:+}"
            );
            assert_eq!(state.unacked_bytes(), (before + total) as u64);
            // Only the last fragment ending on or before the cap leaves the window running.
            assert_eq!(past, fragments < lens.len() || offset > 0);
        }
    }
}

/// A three-byte character counts its three bytes: with the cap on the byte before it, on any
/// of its bytes, or after it, the window pauses exactly when the event's last byte is past the
/// cap. Counting characters instead would leave the window running with the cap on the
/// character's last byte or the next one. The same holds where a fragment ends.
#[test]
fn a_multi_byte_character_counts_every_byte_wherever_the_cap_falls_in_it() {
    // 100 bytes: `{"patches":"` is bytes 1 to 12, the x's 13 to 95, the € 96 to 98, `"}` 99
    // and 100.
    let euro = format!("{{\"patches\":\"{}€\"}}", "x".repeat(83));
    assert_eq!((euro.len(), euro.find('€'), "€".len()), (100, Some(95), 3));
    for (cap_on, paused) in [
        (95, true),   // the byte before the €
        (96, true),   // its first byte
        (97, true),   // its second byte
        (98, true),   // its last byte: 98 characters, but 100 bytes
        (99, true),   // the byte after it
        (100, false), // the event's last byte: a total of exactly the cap
        (101, false), // a total of cap - 1
    ] {
        // The event's byte `cap_on` is the window's byte number CAP.
        let before = CAP - cap_on;
        let mut state = WindowState::new();
        let stream = thread();
        let r = state.follow(stream, None);
        fill(&mut state, stream, r, before);
        let (sent, pause) = send_json(&mut state, stream, r, &euro);
        assert_eq!(sent, [100]);
        assert_eq!(
            (pause, state.is_paused()),
            (paused, paused),
            "cap on the event's byte {cap_on}"
        );
        assert_eq!(state.unacked_bytes(), (before + 100) as u64);
    }

    // A long event of three-byte characters: the fragments end on character boundaries, and
    // the window counts the fragments' bytes, pausing after the last one.
    let long = format!("{{\"patches\":\"{}\"}}", "€".repeat(6000));
    let lens: Vec<usize> = frame::messages(&long, 0)
        .unwrap()
        .iter()
        .map(Frame::len)
        .collect();
    assert!(lens.len() > 1, "{lens:?}");
    let total: usize = lens.iter().sum();
    // The first fragment ends exactly on the cap.
    let before = CAP - lens[0];
    let mut state = WindowState::new();
    let stream = thread();
    let r = state.follow(stream, None);
    fill(&mut state, stream, r, before);
    let (sent, pause) = send_json(&mut state, stream, r, &long);
    assert_eq!(sent, lens);
    assert!(pause && state.is_paused());
    assert_eq!(state.unacked_bytes(), (before + total) as u64);
}
