use clauseo_protocol::{
    AppendTarget, Cursor, HostEvent, MessageId, StreamId, ThreadId, ViewEpoch, ViewPatchBody,
};
use clauseo_reducer::{StreamState, ViewChange};

use super::StreamView;
use crate::encode::{EncodedEvent, Encoder};

fn v7(n: u8) -> uuid::Uuid {
    uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &[n; 10]).into_uuid()
}

fn stream() -> StreamId {
    StreamId::Thread(ThreadId::new(v7(1)).unwrap())
}

fn epoch() -> ViewEpoch {
    ViewEpoch::new(v7(2)).unwrap()
}

fn append(text: &str) -> ViewChange {
    ViewChange::AppendText {
        message_id: MessageId::new(v7(3)).unwrap(),
        target: AppendTarget::AgentText,
        from_len: 0,
        text: text.to_owned(),
    }
}

/// The patches of `events` as (cursor, is_reset), checking each is `serde_json`'s own text.
fn patches(events: &[EncodedEvent]) -> Vec<(u64, bool)> {
    let mut all = Vec::new();
    for event in events {
        let decoded = event.decode().unwrap();
        assert_eq!(event.json(), serde_json::to_string(&decoded).unwrap());
        let HostEvent::Patches(batch) = decoded else {
            panic!("not patches")
        };
        assert_eq!(batch.stream, stream());
        assert_eq!(batch.epoch, epoch());
        for patch in batch.patches {
            all.push((
                patch.cursor.get(),
                matches!(patch.body, ViewPatchBody::Reset { .. }),
            ));
        }
    }
    all
}

fn at(cursor: u64) -> Option<(ViewEpoch, Cursor)> {
    Some((epoch(), Cursor::new(cursor).unwrap()))
}

#[test]
fn catch_up_gives_the_tail_or_one_reset() {
    let state = StreamState::new(stream());
    let mut view = StreamView::new(stream(), epoch(), 1024).unwrap();
    let mut enc = Encoder::new();
    assert_eq!(view.cursor(), Cursor::ZERO);
    let events = view
        .publish(vec![append("a"), append("b"), append("c")], &mut enc)
        .unwrap();
    assert_eq!(patches(&events), vec![(1, false), (2, false), (3, false)]);
    assert_eq!(view.cursor(), Cursor::new(3).unwrap());

    let tail = view.catch_up(at(1), &state, &mut enc).unwrap();
    assert_eq!(patches(&tail), vec![(2, false), (3, false)]);
    assert!(
        view.catch_up(at(3), &state, &mut enc).unwrap().is_empty(),
        "nothing after the end"
    );
    let from_zero = view.catch_up(at(0), &state, &mut enc).unwrap();
    assert_eq!(
        patches(&from_zero),
        vec![(1, false), (2, false), (3, false)]
    );
    // The tail is the same bytes as the live batch.
    assert_eq!(
        from_zero.first().unwrap().json(),
        events.first().unwrap().json()
    );

    let reset = vec![(3, true)];
    assert_eq!(
        patches(&view.catch_up(None, &state, &mut enc).unwrap()),
        reset
    );
    assert_eq!(
        patches(&view.catch_up(at(4), &state, &mut enc).unwrap()),
        reset,
        "ahead"
    );
    let other = Some((ViewEpoch::new(v7(9)).unwrap(), Cursor::new(2).unwrap()));
    assert_eq!(
        patches(&view.catch_up(other, &state, &mut enc).unwrap()),
        reset,
        "old epoch"
    );
    assert_eq!(view.epoch(), epoch());
}

#[test]
fn patch_window_capped_by_bytes() {
    let state = StreamState::new(stream());
    let cap = 600;
    let mut view = StreamView::new(stream(), epoch(), cap).unwrap();
    let mut enc = Encoder::new();
    let mut sizes = Vec::new();
    for n in 0..40 {
        let events = view
            .publish(vec![append(&"x".repeat(n * 3))], &mut enc)
            .unwrap();
        let HostEvent::Patches(batch) = events.first().unwrap().decode().unwrap() else {
            panic!("not patches")
        };
        let patch = batch.patches.first().unwrap();
        sizes.push(serde_json::to_string(patch).unwrap().len());
        assert!(
            view.window_bytes() <= cap,
            "{} bytes held",
            view.window_bytes()
        );
    }
    // The window holds exactly the newest patches that fit, counted by their serde_json length.
    let kept = sizes.iter().rev().scan(0, |sum, len| {
        *sum += len;
        (*sum <= cap).then_some(*len)
    });
    let kept: Vec<usize> = kept.collect();
    assert_eq!(view.window_bytes(), kept.iter().sum::<usize>());
    let oldest_kept = 40 - kept.len() as u64 + 1;
    let tail = view
        .catch_up(at(oldest_kept - 1), &state, &mut enc)
        .unwrap();
    let expected: Vec<(u64, bool)> = (oldest_kept..=40).map(|c| (c, false)).collect();
    assert_eq!(
        patches(&tail),
        expected,
        "a cursor still inside gets the tail"
    );
    let fell_out = view
        .catch_up(at(oldest_kept - 2), &state, &mut enc)
        .unwrap();
    assert_eq!(
        patches(&fell_out),
        vec![(40, true)],
        "a cursor that fell out gets a Reset"
    );

    // A single patch over the cap empties the window and is not kept.
    view.publish(vec![append(&"y".repeat(cap))], &mut enc)
        .unwrap();
    assert_eq!(view.window_bytes(), 0);
    assert_eq!(
        patches(&view.catch_up(at(40), &state, &mut enc).unwrap()),
        vec![(41, true)]
    );
    assert!(view.catch_up(at(41), &state, &mut enc).unwrap().is_empty());
}

#[test]
fn reset_carries_the_view_snapshot_at_the_cursor() {
    let state = StreamState::new(stream());
    let mut view = StreamView::new(stream(), epoch(), 1024).unwrap();
    let mut enc = Encoder::new();
    view.publish(vec![append("a")], &mut enc).unwrap();
    let reset = view.reset(&state, &mut enc).unwrap();
    let HostEvent::Patches(batch) = reset.decode().unwrap() else {
        panic!("not patches")
    };
    let [patch] = batch.patches.as_slice() else {
        panic!("one patch")
    };
    assert_eq!(patch.cursor, Cursor::new(1).unwrap());
    let ViewPatchBody::Reset { view_window } = &patch.body else {
        panic!("not a reset")
    };
    assert_eq!(**view_window, clauseo_reducer::view_snapshot(&state));
    assert_eq!(
        view.window_bytes(),
        serde_json::to_string(&clauseo_protocol::ViewPatch {
            cursor: Cursor::new(1).unwrap(),
            body: ViewPatchBody::AppendText {
                message_id: MessageId::new(v7(3)).unwrap(),
                target: AppendTarget::AgentText,
                from_len: 0,
                text: "a".to_owned(),
            },
        })
        .unwrap()
        .len(),
        "the reset is not kept"
    );
}
