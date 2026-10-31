use clauseo_protocol::{
    AppendTarget, Cursor, HostEvent, MessageId, StreamId, ThreadId, ViewEpoch, ViewPatchBody,
};
use clauseo_reducer::ViewChange;

use super::{Encoder, MAX_BATCH_BYTES};
use crate::view::StreamView;
use crate::window::PATCH_WINDOW_BYTES;

fn v7(n: u8) -> uuid::Uuid {
    uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &[n; 10]).into_uuid()
}

fn append(text_len: usize) -> ViewChange {
    ViewChange::AppendText {
        message_id: MessageId::new(v7(3)).unwrap(),
        target: AppendTarget::AgentText,
        from_len: 0,
        text: "a".repeat(text_len),
    }
}

/// Every patch of every batch, checking each batch's JSON is what `serde_json` writes for it.
fn cursors(events: &[super::EncodedEvent]) -> Vec<u64> {
    let mut all = Vec::new();
    for event in events {
        let HostEvent::Patches(batch) = event.decode().unwrap() else {
            panic!("not patches")
        };
        assert_eq!(
            event.json(),
            serde_json::to_string(&HostEvent::Patches(batch.clone())).unwrap()
        );
        all.extend(batch.patches.iter().map(|patch| patch.cursor.get()));
    }
    all
}

#[test]
fn patch_batches_stay_under_8_kb() {
    let stream = StreamId::Thread(ThreadId::new(v7(1)).unwrap());
    let mut view =
        StreamView::new(stream, ViewEpoch::new(v7(2)).unwrap(), PATCH_WINDOW_BYTES).unwrap();
    let mut enc = Encoder::new();
    let events = view
        .publish((0..16).map(|_| append(1024)).collect(), &mut enc)
        .unwrap();
    assert!(events.len() > 1, "16 KiB of text needs several batches");
    for event in &events {
        assert!(
            event.json().len() < MAX_BATCH_BYTES,
            "{} bytes",
            event.json().len()
        );
    }
    assert_eq!(cursors(&events), (1..=16).collect::<Vec<_>>());

    // One patch over 8 KB goes alone, between batches that keep the rule.
    let big = vec![append(100), append(9000), append(100)];
    let events = view.publish(big, &mut enc).unwrap();
    assert_eq!(events.len(), 3);
    assert!(events.get(1).unwrap().json().len() >= MAX_BATCH_BYTES);
    assert_eq!(cursors(&events), vec![17, 18, 19]);
    assert_eq!(view.cursor(), Cursor::new(19).unwrap());
}

#[test]
fn batches_fill_up_to_the_limit() {
    let stream = StreamId::Thread(ThreadId::new(v7(1)).unwrap());
    let mut view =
        StreamView::new(stream, ViewEpoch::new(v7(2)).unwrap(), PATCH_WINDOW_BYTES).unwrap();
    let mut enc = Encoder::new();
    let events = view
        .publish((0..200).map(|n| append(n % 97)).collect(), &mut enc)
        .unwrap();
    assert_eq!(cursors(&events), (1..=200).collect::<Vec<_>>());
    let lens: Vec<usize> = events.iter().map(|event| event.json().len()).collect();
    // Each batch but the last would have reached the limit with the next patch.
    for (event, next) in events.iter().zip(events.iter().skip(1)) {
        let HostEvent::Patches(next) = next.decode().unwrap() else {
            panic!("not patches")
        };
        let first = next.patches.first().unwrap();
        let first_len = serde_json::to_string(first).unwrap().len();
        assert!(
            event.json().len() + 1 + first_len >= MAX_BATCH_BYTES,
            "{lens:?}"
        );
        assert!(event.json().len() < MAX_BATCH_BYTES);
    }
    let HostEvent::Patches(last) = events.last().unwrap().decode().unwrap() else {
        panic!()
    };
    assert!(matches!(
        last.patches.last().unwrap().body,
        ViewPatchBody::AppendText { .. }
    ));
}
