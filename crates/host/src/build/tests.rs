use clauseo_protocol::{
    AgentContentEntry, CompactString, CompactionStatus, ContentBlock, Delivery, EventId,
    INLINE_LIMIT_BYTES, MessageId, PREVIEW_BYTES, Seq, TextKind, TextOrBlob, ToolCallContent,
    ToolCallEntry, ToolCallId, ToolCallPatch, ToolCallStatus, ToolKind, TurnId, UserMessageEntry,
    UserMessageStatus, ViewEntry,
};
use clauseo_store::BlobIn;
use proptest::prelude::*;
use uuid::{Builder, Uuid};

use super::{
    Built, ToolFields, compaction_changed, content_appended, tool_call_started, tool_call_updated,
    user_message,
};

fn uuid(n: u8) -> Uuid {
    Builder::from_unix_timestamp_millis(1_700_000_000_000, &[n; 10]).into_uuid()
}

fn turn() -> TurnId {
    TurnId::new(uuid(1)).unwrap()
}

fn tool_call_id() -> ToolCallId {
    ToolCallId::new(uuid(2)).unwrap()
}

fn message() -> MessageId {
    MessageId::new(uuid(3)).unwrap()
}

fn inline(text: &str) -> TextOrBlob {
    TextOrBlob::Inline(text.to_owned())
}

fn base64_of(len: usize) -> String {
    "iVBORw0K".chars().cycle().take(len).collect()
}

fn image(data: String) -> ContentBlock {
    ContentBlock::Image {
        data: TextOrBlob::Inline(data),
        mime_type: CompactString::const_new("image/png"),
        uri: None,
    }
}

fn call(
    raw_input: Option<TextOrBlob>,
    raw_output: Option<TextOrBlob>,
    content: Vec<ToolCallContent>,
) -> clauseo_acp::ToolCall {
    clauseo_acp::ToolCall {
        acp_id: CompactString::const_new("tc-1"),
        title: CompactString::const_new("Read a.txt"),
        kind: ToolKind::Read,
        tool_name: None,
        status: ToolCallStatus::InProgress,
        locations: Vec::new(),
        raw_input,
        raw_output,
        content,
    }
}

/// Asserts `value` is a blob whose bytes ride in `blobs` and equal `expected`.
fn assert_blob(value: &TextOrBlob, blobs: &[BlobIn], expected: &str) {
    let TextOrBlob::Blob(blob_ref) = value else {
        panic!("expected a blob, got {value:?}");
    };
    let blob = blobs
        .iter()
        .find(|blob| blob.hash() == blob_ref.hash())
        .expect("the blob rides with the event");
    assert_eq!(blob.bytes(), expected.as_bytes());
    assert_eq!(blob_ref.size_bytes().get(), expected.len() as u64);
    assert_eq!(blob_ref, &blob.blob_ref());
}

/// The view entry R6 keeps for a tool call with these fields.
fn tool_entry(
    raw_input: Option<TextOrBlob>,
    raw_output: Option<TextOrBlob>,
    content: Vec<ToolCallContent>,
) -> ViewEntry {
    ViewEntry::ToolCall(Box::new(ToolCallEntry {
        order: Seq::FIRST,
        tool_call: tool_call_id(),
        turn: turn(),
        title: CompactString::const_new("Read a.txt"),
        kind: ToolKind::Read,
        tool_name: None,
        status: ToolCallStatus::InProgress,
        locations: Vec::new(),
        raw_input,
        raw_output,
        content,
    }))
}

fn started_entry(built: &Built<clauseo_protocol::ToolCallStarted>) -> ViewEntry {
    let v = &built.value;
    tool_entry(v.raw_input.clone(), v.raw_output.clone(), v.content.clone())
}

#[test]
fn large_image_is_a_blob() {
    let data = base64_of(64 * 1024);
    let (started, before) = tool_call_started(turn(), tool_call_id(), call(None, None, vec![]));
    assert_eq!(started.blobs.len(), 0);
    let patch = ToolCallPatch {
        content: Some(vec![ToolCallContent::Content {
            content: image(data.clone()),
        }]),
        ..ToolCallPatch::default()
    };
    let (updated, after) = tool_call_updated(tool_call_id(), patch, before);
    let content = updated.value.patch.content.as_ref().unwrap();
    let [
        ToolCallContent::Content {
            content: ContentBlock::Image { data: logged, .. },
        },
    ] = content.as_slice()
    else {
        panic!("expected one image block, got {content:?}");
    };
    assert_blob(logged, &updated.blobs, &data);
    assert_eq!(updated.blobs.len(), 1);
    assert_eq!(after.inline_len(), 0);
}

#[test]
fn agent_image_chunk_is_logged() {
    let large = base64_of(64 * 1024);
    let built = content_appended(
        turn(),
        message(),
        None,
        TextKind::Answer,
        image(large.clone()),
    );
    let ContentBlock::Image { data, .. } = &built.value.block else {
        panic!("expected an image, got {:?}", built.value.block);
    };
    assert_blob(data, &built.blobs, &large);
    assert_eq!(built.blobs.len(), 1);

    let small = base64_of(1024);
    let built = content_appended(
        turn(),
        message(),
        None,
        TextKind::Answer,
        image(small.clone()),
    );
    assert_eq!(built.value.block, image(small));
    assert_eq!(built.blobs.len(), 0);
}

#[test]
fn inline_limit_counts_escaped_bytes() {
    let tabs = "\t".repeat(3_000);
    let (built, fields) = tool_call_started(
        turn(),
        tool_call_id(),
        call(Some(inline(&tabs)), None, vec![]),
    );
    assert_blob(built.value.raw_input.as_ref().unwrap(), &built.blobs, &tabs);
    assert_eq!(fields.inline_len(), 0);

    let tabs = "\t".repeat(2_000);
    let (built, fields) = tool_call_started(
        turn(),
        tool_call_id(),
        call(Some(inline(&tabs)), None, vec![]),
    );
    assert_eq!(built.value.raw_input, Some(inline(&tabs)));
    assert_eq!(built.blobs.len(), 0);
    assert_eq!(fields.inline_len(), 4_000);
    assert_eq!(fields.inline_len(), started_entry(&built).inline_len());
}

#[test]
fn large_tool_output_has_a_preview() {
    let text = "x".repeat(65_536);
    let content = vec![ToolCallContent::Text {
        text: inline(&text),
    }];
    let (built, _) = tool_call_started(turn(), tool_call_id(), call(None, None, content));
    let [ToolCallContent::Text { text: logged }] = built.value.content.as_slice() else {
        panic!("expected one text item");
    };
    assert_blob(logged, &built.blobs, &text);
    let TextOrBlob::Blob(blob_ref) = logged else {
        panic!("checked above")
    };
    assert_eq!(blob_ref.preview().len(), PREVIEW_BYTES);
    assert_eq!(blob_ref.size_bytes().get(), 65_536);
}

#[test]
fn values_are_placed_in_field_order() {
    // raw_input fills most of the budget, raw_output no longer fits, the small content item does.
    let input = "a".repeat(4_000);
    let output = "b".repeat(200);
    let content = vec![ToolCallContent::Text { text: inline("c") }];
    let (built, fields) = tool_call_started(
        turn(),
        tool_call_id(),
        call(Some(inline(&input)), Some(inline(&output)), content),
    );
    assert_eq!(built.value.raw_input, Some(inline(&input)));
    assert_blob(
        built.value.raw_output.as_ref().unwrap(),
        &built.blobs,
        &output,
    );
    assert_eq!(
        built.value.content,
        vec![ToolCallContent::Text { text: inline("c") }]
    );
    assert_eq!(fields.inline_len(), 4_001);
    assert_eq!(fields.inline_len(), started_entry(&built).inline_len());
}

#[test]
fn update_recounts_from_fields_after_the_patch() {
    let input = "a".repeat(3_000);
    let (started, before) = tool_call_started(
        turn(),
        tool_call_id(),
        call(Some(inline(&input)), None, vec![]),
    );
    assert_eq!(before.inline_len(), 3_000);

    // raw_input stays; a 2,000-byte output would pass the limit with it, so it becomes a blob.
    let output = "o".repeat(2_000);
    let patch = ToolCallPatch {
        raw_output: Some(inline(&output)),
        ..ToolCallPatch::default()
    };
    let (updated, after) = tool_call_updated(tool_call_id(), patch, before);
    let logged = updated.value.patch.raw_output.as_ref().unwrap();
    assert_blob(logged, &updated.blobs, &output);
    assert_eq!(after.inline_len(), 3_000);
    let entry = tool_entry(started.value.raw_input, Some(logged.clone()), vec![]);
    assert_eq!(after.inline_len(), entry.inline_len());

    // A patch replacing raw_input frees its cost: now the content fits beside the new input.
    let content = vec![ToolCallContent::Text {
        text: inline(&"t".repeat(3_000)),
    }];
    let patch = ToolCallPatch {
        raw_input: Some(inline("{}")),
        content: Some(content.clone()),
        ..ToolCallPatch::default()
    };
    let (updated, again) = tool_call_updated(tool_call_id(), patch, after);
    assert_eq!(updated.blobs.len(), 0);
    assert_eq!(updated.value.patch.content, Some(content.clone()));
    assert_eq!(again.inline_len(), 3_002);
    let entry = tool_entry(Some(inline("{}")), Some(logged.clone()), content);
    assert_eq!(again.inline_len(), entry.inline_len());
    assert_eq!(again, after.patched(&updated.value.patch));
}

#[test]
fn rebuilt_fields_match_built_fields() {
    let content = vec![
        ToolCallContent::Text {
            text: inline("line\n"),
        },
        ToolCallContent::Content {
            content: image(base64_of(5_000)),
        },
    ];
    let (built, fields) = tool_call_started(
        turn(),
        tool_call_id(),
        call(Some(inline("{\"a\":1}")), None, content),
    );
    assert_eq!(ToolFields::of_started(&built.value), fields);
}

#[test]
fn compaction_summary_over_the_limit_is_a_blob() {
    let summary = "s".repeat(INLINE_LIMIT_BYTES + 1);
    let built = compaction_changed(CompactionStatus::Completed, Some(summary.clone()));
    assert_blob(
        built.value.summary.as_ref().unwrap(),
        &built.blobs,
        &summary,
    );

    let built = compaction_changed(CompactionStatus::Completed, Some("short".to_owned()));
    assert_eq!(built.value.summary, Some(inline("short")));
    assert_eq!(built.blobs.len(), 0);
}

#[test]
fn user_message_and_agent_content_match_view_counts() {
    let content = vec![
        ContentBlock::Text {
            text: "t".repeat(10_000),
        },
        image(base64_of(3_000)),
        image(base64_of(3_000)),
    ];
    let built = user_message(turn(), message(), None, content, Delivery::Queue);
    assert_eq!(built.blobs.len(), 1);
    let entry = ViewEntry::UserMessage(UserMessageEntry {
        order: Seq::FIRST,
        message: message(),
        turn: turn(),
        content: built.value.content,
        delivery: Delivery::Queue,
        status: UserMessageStatus::Sent,
        edited: false,
        steer: None,
    });
    assert_eq!(entry.inline_len(), 3_000);

    let built = content_appended(
        turn(),
        message(),
        None,
        TextKind::Thought,
        image(base64_of(9)),
    );
    let entry = ViewEntry::AgentContent(AgentContentEntry {
        order: Seq::FIRST,
        event: EventId::new(uuid(4)).unwrap(),
        turn: turn(),
        message: message(),
        kind: TextKind::Thought,
        block: built.value.block,
    });
    assert_eq!(entry.inline_len(), 9);
}

/// Characters with each escape cost: plain ASCII (1 byte), a short escape (2), a `\u00XX`
/// escape (6), a quote and a backslash (2), and a 2-byte UTF-8 char with no escape.
const CHARS: [char; 6] = ['a', '\t', '\u{1}', '"', '\\', 'é'];

fn text_strategy() -> impl Strategy<Value = String> {
    // Up to three runs of one character each, so values reach the limit and mix escape costs
    // without generating thousands of characters one by one.
    proptest::collection::vec((0..CHARS.len(), 0..1_500usize), 0..4).prop_map(|runs| {
        runs.into_iter()
            .map(|(pick, len)| CHARS[pick].to_string().repeat(len))
            .collect()
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0x5EED_B11D),
        ..ProptestConfig::default()
    })]

    #[test]
    fn inline_total_stays_within_limit_and_matches_view(
        input in proptest::option::of(text_strategy()),
        output in proptest::option::of(text_strategy()),
        items in proptest::collection::vec(text_strategy(), 0..4),
        patch_output in proptest::option::of(text_strategy()),
    ) {
        let content: Vec<_> = items
            .into_iter()
            .map(|text| ToolCallContent::Text { text: TextOrBlob::Inline(text) })
            .collect();
        let (built, fields) = tool_call_started(
            turn(),
            tool_call_id(),
            call(input.map(TextOrBlob::Inline), output.map(TextOrBlob::Inline), content),
        );
        let entry = started_entry(&built);
        prop_assert!(fields.inline_len() <= INLINE_LIMIT_BYTES);
        prop_assert_eq!(fields.inline_len(), entry.inline_len());

        let patch = ToolCallPatch {
            raw_output: patch_output.map(TextOrBlob::Inline),
            ..ToolCallPatch::default()
        };
        let (updated, after) = tool_call_updated(tool_call_id(), patch, fields);
        let v = &built.value;
        let raw_output = updated.value.patch.raw_output.or_else(|| v.raw_output.clone());
        let entry = tool_entry(v.raw_input.clone(), raw_output, v.content.clone());
        prop_assert!(after.inline_len() <= INLINE_LIMIT_BYTES);
        prop_assert_eq!(after.inline_len(), entry.inline_len());
    }
}
