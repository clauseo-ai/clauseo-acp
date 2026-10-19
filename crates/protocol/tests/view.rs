//! View entries and patches: boxed large variants, entry ids, and how an entry's inline cost
//! counts.

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

use std::mem::size_of;

use clauseo_protocol::{
    CompactionStatus, ContentBlock, Cursor, DomainEvent, EntryId, HeaderEntry, HostEvent,
    INLINE_LIMIT_BYTES, MarkerEntry, MarkerKind, PatchBatch, RawFrom, StreamId, TextOrBlob,
    ThreadStatus, ToolCallContent, ToolCallEntry, ToolCallStatus, ToolKind, ToolLocation,
    ViewEntry, ViewEpoch, ViewPatch, ViewPatchBody, ViewSnapshot,
};

use fixtures::{
    agent, blob, event_id, json_cost, matter_id, path, round_trips, seq, short, thread_id,
    tool_call_id, turn_id, v7,
};

fn inline(text: &str) -> TextOrBlob {
    TextOrBlob::Inline(text.to_owned())
}

fn tool_call() -> ToolCallEntry {
    ToolCallEntry {
        order: seq(5),
        tool_call: tool_call_id(3),
        turn: turn_id(2),
        title: short("Edit draft"),
        kind: ToolKind::Edit,
        tool_name: None,
        status: ToolCallStatus::Completed,
        locations: vec![ToolLocation::Inside {
            path: path("draft.docx"),
            line: None,
        }],
        raw_input: Some(inline(r#"{"path":"draft.docx","note":"line1\nline2"}"#)),
        raw_output: Some(inline("ok\t\"done\"")),
        content: vec![
            ToolCallContent::Text(inline("tab\there\u{1}")),
            ToolCallContent::Content(ContentBlock::Image {
                data: TextOrBlob::Blob(blob(9, 100_000)),
                mime_type: short("image/png"),
                uri: None,
            }),
            ToolCallContent::Diff {
                path: ToolLocation::Inside {
                    path: path("draft.docx"),
                    line: None,
                },
                old: Some(inline("before \\ slash")),
                new: inline("after é"),
            },
            ToolCallContent::Content(ContentBlock::Resource {
                uri: "file:///x".to_owned(),
                mime_type: None,
                text: Some(inline("resource\r\n")),
                blob: Some(TextOrBlob::Blob(blob(8, 50))),
            }),
            ToolCallContent::Content(ContentBlock::Unsupported {
                kind: "later".to_owned(),
                raw_from: RawFrom::Acp,
                raw: Some(inline(r#"{"type":"later"}"#)),
            }),
        ],
    }
}

#[test]
fn entry_inline_len_sums_vec_items() {
    let entry = ViewEntry::ToolCall(Box::new(tool_call()));
    let inline_parts = [
        r#"{"path":"draft.docx","note":"line1\nline2"}"#,
        "ok\t\"done\"",
        "tab\there\u{1}",
        "before \\ slash",
        "after é",
        "resource\r\n",
        r#"{"type":"later"}"#,
    ];
    let expected: usize = inline_parts.iter().map(|t| json_cost(t)).sum();
    assert_eq!(entry.inline_len(), expected);
    // Escapes count: the raw bytes are fewer than the escaped ones.
    let raw_bytes: usize = inline_parts.iter().map(|t| t.len()).sum();
    assert!(expected > raw_bytes, "{expected} vs {raw_bytes}");
    assert!(entry.inline_len() <= INLINE_LIMIT_BYTES);

    // Every Vec item counts: dropping the last item lowers the cost by exactly its share.
    let mut fewer = tool_call();
    fewer.content.pop();
    let fewer = ViewEntry::ToolCall(Box::new(fewer));
    assert_eq!(
        fewer.inline_len(),
        expected - json_cost(r#"{"type":"later"}"#)
    );

    // A blob counts 0, whatever its size; the inline form of the same field counts its text.
    let mut as_blob = tool_call();
    as_blob.raw_input = Some(TextOrBlob::Blob(blob(1, 1_000_000)));
    let as_blob = ViewEntry::ToolCall(Box::new(as_blob));
    assert_eq!(
        as_blob.inline_len(),
        expected - json_cost(r#"{"path":"draft.docx","note":"line1\nline2"}"#)
    );

    // A marker counts its summary; a header counts nothing.
    let marker = ViewEntry::Marker(MarkerEntry {
        order: seq(9),
        event: event_id(9),
        kind: MarkerKind::Compaction {
            status: CompactionStatus::Completed,
        },
        summary: Some(inline("short \"summary\"")),
    });
    assert_eq!(marker.inline_len(), json_cost("short \"summary\""));
    assert_eq!(header().inline_len(), 0);
}

fn header() -> ViewEntry {
    ViewEntry::Header(HeaderEntry {
        order: seq(1),
        thread: thread_id(1),
        matter: matter_id(2),
        title: Some(short("Lease review")),
        agent: agent("claude-acp"),
        status: ThreadStatus::Queued {
            behind: thread_id(4),
        },
        mode_id: None,
        config_options: Vec::new(),
        commands: Vec::new(),
        plan: Vec::new(),
        usage: None,
    })
}

#[test]
fn entry_id_and_order_come_from_the_entry() {
    let entry = ViewEntry::ToolCall(Box::new(tool_call()));
    assert_eq!(entry.id(), EntryId::ToolCall(tool_call_id(3)));
    assert_eq!(entry.order(), seq(5));
    assert_eq!(header().id(), EntryId::Header(thread_id(1)));
    assert_eq!(header().id().to_string(), format!("header:{}", v7(1)));
    assert_ne!(header().id(), EntryId::Header(thread_id(2)));
}

#[test]
fn large_variants_are_boxed() {
    let sizes = [
        ("ViewEntry", size_of::<ViewEntry>()),
        ("ViewPatch", size_of::<ViewPatch>()),
        ("ViewPatchBody", size_of::<ViewPatchBody>()),
        ("HostEvent", size_of::<HostEvent>()),
        ("DomainEvent", size_of::<DomainEvent>()),
        ("ToolCallEntry", size_of::<ToolCallEntry>()),
        ("HeaderEntry", size_of::<HeaderEntry>()),
    ];
    println!("{sizes:?}");
    assert!(size_of::<ViewPatch>() <= 64, "{sizes:?}");
    // What the boxes save: either entry inline would put `ViewPatch` far over the bound.
    assert!(size_of::<ToolCallEntry>() > 64, "{sizes:?}");
    assert!(size_of::<ViewSnapshot>() > 64, "{sizes:?}");
}

#[test]
fn patches_round_trip() {
    let epoch = ViewEpoch::new(v7(30)).unwrap();
    let batch = PatchBatch {
        stream: StreamId::Thread(thread_id(1)),
        epoch,
        patches: vec![
            ViewPatch {
                cursor: Cursor::new(1).unwrap(),
                body: ViewPatchBody::Upsert {
                    entry: Box::new(header()),
                },
            },
            ViewPatch {
                cursor: Cursor::new(2).unwrap(),
                body: ViewPatchBody::Reset {
                    view_snapshot: Box::new(ViewSnapshot {
                        header: Some(header()),
                        entries: vec![ViewEntry::ToolCall(Box::new(tool_call()))],
                        has_older: false,
                    }),
                },
            },
        ],
    };
    round_trips(&HostEvent::Patches(batch));
    let json = serde_json::to_string(&ViewPatch {
        cursor: Cursor::new(3).unwrap(),
        body: ViewPatchBody::Remove {
            entry_id: EntryId::Notice(event_id(5)),
        },
    })
    .unwrap();
    assert_eq!(
        json,
        format!(
            r#"{{"cursor":3,"body":{{"remove":{{"entry_id":"notice:{}"}}}}}}"#,
            v7(5)
        )
    );
}
