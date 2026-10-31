//! Large payloads: a 64 KiB tool output is stored as a blob and read back in ranges.

use clauseo_protocol::{
    BlobRange, BlobRef, Count, DomainEvent, HostQuery, QueryResult, ReadLen, StopReason, StreamId,
    TextOrBlob, ToolCallContent, ViewEntry, ViewPatchBody,
};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// The text `large-tool-output` sends: 4,096 lines of 16 bytes, each with a two-byte `é`.
fn big_text() -> String {
    "Clause text \u{e9}.\n".repeat(4096)
}

/// The blob the turn's `ToolCallUpdated` names in its content, when it does.
fn updated_blob(events: &Events) -> Option<BlobRef> {
    events.events().find_map(|event| match event {
        DomainEvent::ToolCallUpdated(updated) => match updated.patch.content.as_deref() {
            Some(
                [
                    ToolCallContent::Text {
                        text: TextOrBlob::Blob(blob),
                    },
                ],
            ) => Some(blob.clone()),
            _ => None,
        },
        _ => None,
    })
}

/// Reads `len` bytes of `blob` from `offset`.
fn read(host: &Running, blob: &BlobRef, offset: u64, len: u32) -> BlobRange {
    let query = HostQuery::ReadBlob {
        hash: blob.hash(),
        offset: Count::new(offset).unwrap(),
        len: ReadLen::new(len).unwrap(),
    };
    match host.query(query) {
        Ok(QueryResult::Blob(range)) => range,
        other => panic!("ReadBlob at {offset} answered {other:?}"),
    }
}

#[test]
fn large_tool_output_is_a_blob() {
    let dir = Dir::new(
        "large_tool_output_is_a_blob",
        &[(FAKE, Scenario::Local("large-tool-output"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let stream = StreamId::Thread(thread);
    let mut view = host.view(stream, None);
    view.until_items(1);
    let mut events = host.events(stream);
    let (turn, _) = host.prompt(thread, "Read the long file.");
    let big = big_text();

    // A live event is published only after its commit returned, so a read made as soon as the
    // event arrives finds the bytes only if they rode in that commit (or an earlier one).
    events.until("the ToolCallUpdated with a blob", |e| {
        updated_blob(e).is_some()
    });
    let blob = updated_blob(&events).unwrap();
    let whole = read(&host, &blob, 0, 1024 * 1024);
    assert_eq!(blob.size_bytes().get(), 65_536);
    assert_eq!(blob.preview().len(), 512);
    assert_eq!(blob.preview(), &big[..512]);
    assert_eq!((whole.offset.get(), whole.end.get()), (0, 65_536));
    assert!(whole.text == big, "ReadBlob returned other bytes");

    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    host.settle();
    view.drain_ready();
    let in_view = view.patches().any(|patch| {
        match &patch.body {
        ViewPatchBody::Upsert { entry } => match entry.as_ref() {
            ViewEntry::ToolCall(call) => call.content.iter().any(|content| {
                matches!(content, ToolCallContent::Text { text: TextOrBlob::Blob(b) } if *b == blob)
            }),
            _ => false,
        },
        _ => false,
    }
    });
    assert!(in_view, "no Upsert carries the BlobRef: {}", view.dump());

    let mut joined = String::new();
    let (mut offset, mut reads) = (0, 0);
    while offset < 65_536 {
        let range = read(&host, &blob, offset, 1000);
        assert_eq!(range.offset.get(), offset, "a read from the last end moved");
        assert!(
            range.end.get() > offset,
            "a read made no progress at {offset}"
        );
        joined.push_str(&range.text);
        offset = range.end.get();
        reads += 1;
        assert!(reads <= 100, "too many reads");
    }
    assert_eq!(offset, 65_536);
    assert!(joined == big, "the ranges do not join to the blob");
    host.shutdown();
}
