//! The withheld set: what an answer keeps, how it replaces, and when it lets go.

use clauseo_protocol::{
    ContentBlock, ElicitationId, RawPayload, TextOrBlob, ToolCallContent, ToolCallPatch,
    ToolCallStatus, WITHHELD,
};
use serde_json::{Value, json};

use super::{Withheld, replace_all};

fn values(json: Value) -> RawPayload {
    serde_json::from_value(json).unwrap()
}

fn id(n: u8) -> ElicitationId {
    let mut bytes = [0u8; 16];
    bytes[15] = n;
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    ElicitationId::new(uuid::Uuid::from_bytes(bytes)).unwrap()
}

/// A patch whose output, input, title and text content all say `text`.
fn echo(text: &str, status: Option<ToolCallStatus>) -> ToolCallPatch {
    ToolCallPatch {
        title: Some(text.into()),
        raw_input: Some(TextOrBlob::Inline(json!({ "in": text }).to_string())),
        raw_output: Some(TextOrBlob::Inline(json!({ "out": text }).to_string())),
        content: Some(vec![
            ToolCallContent::Text {
                text: TextOrBlob::Inline(text.to_owned()),
            },
            ToolCallContent::Content {
                content: ContentBlock::Text {
                    text: text.to_owned(),
                },
            },
        ]),
        status,
        ..ToolCallPatch::default()
    }
}

fn says(patch: &ToolCallPatch, needle: &str) -> bool {
    serde_json::to_string(patch).unwrap().contains(needle)
}

#[test]
fn replaces_the_longest_value_at_the_earliest_position() {
    let needles = ["secret-long", "secret", "ok"];
    assert_eq!(
        replace_all("a secret-long, a secret, ok.", &needles).as_deref(),
        Some("a [withheld], a [withheld], [withheld].")
    );
    assert_eq!(replace_all("nothing here", &needles), None);
}

#[test]
fn the_marker_is_never_matched_again() {
    // "held" is inside "[withheld]": a second pass would leave "[with[withheld]]".
    assert_eq!(
        replace_all("abc held", &["abc", "held"]).as_deref(),
        Some("[withheld] [withheld]")
    );
}

#[test]
fn a_scoped_answer_withholds_every_string_from_its_call() {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.accepted(
        id(1),
        &values(
            json!({"api_key": "sk-SECRET", "note": "ok", "picks": ["Fees"], "n": 42,
            "flag": true, "blank": ""}),
        ),
    );
    let mut patch = echo("sk-SECRET ok Fees 42 true", None);
    withheld.scrub_patch("tc-1", &mut patch);
    assert!(!says(&patch, "sk-SECRET"), "{patch:?}");
    assert!(!says(&patch, "Fees"), "{patch:?}");
    assert_eq!(
        patch.title.as_deref(),
        Some("[withheld] [withheld] [withheld] 42 true")
    );
    // Another call keeps its output: the scope is the call.
    let mut other = echo("sk-SECRET", None);
    withheld.scrub_patch("tc-2", &mut other);
    assert!(says(&other, "sk-SECRET"));
}

#[test]
fn json_is_scrubbed_in_leaves_and_keys_and_stays_valid() {
    let mut withheld = Withheld::default();
    let tricky = "a \"quoted\" C:\\dir";
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.accepted(id(1), &values(json!({ "v": tricky })));
    let output = json!({ "echo": format!("got {tricky}"), tricky: 1 }).to_string();
    let mut patch = ToolCallPatch {
        raw_output: Some(TextOrBlob::Inline(output)),
        ..ToolCallPatch::default()
    };
    withheld.scrub_patch("tc-1", &mut patch);
    let Some(TextOrBlob::Inline(text)) = &patch.raw_output else {
        panic!("{patch:?}");
    };
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed, json!({"echo": "got [withheld]", WITHHELD: 1}));
}

#[test]
fn text_that_is_not_json_is_scrubbed_with_escaped_forms_too() {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.accepted(id(1), &values(json!({ "v": "x\"y" })));
    let mut patch = ToolCallPatch {
        raw_output: Some(TextOrBlob::Inline("{\"a\": \"x\\\"y\" broken".to_owned())),
        ..ToolCallPatch::default()
    };
    withheld.scrub_patch("tc-1", &mut patch);
    assert_eq!(
        patch.raw_output,
        Some(TextOrBlob::Inline(
            "{\"a\": \"[withheld]\" broken".to_owned()
        ))
    );
}

#[test]
fn an_unscoped_answer_withholds_strings_of_eight_chars_or_more_from_every_call() {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), None);
    // "seven77" is 7 chars; "naïve-é1" is 8 chars but 10 bytes.
    withheld.accepted(
        id(1),
        &values(json!({"token": "unscoped-SECRET", "short": "seven77", "u": "naïve-é1"})),
    );
    let mut patch = echo("unscoped-SECRET seven77 naïve-é1", None);
    withheld.scrub_patch("tc-any", &mut patch);
    assert_eq!(
        patch.title.as_deref(),
        Some("[withheld] seven77 [withheld]")
    );
    // The turn's set outlives a call's completion.
    let mut done = echo("unscoped-SECRET", Some(ToolCallStatus::Completed));
    withheld.scrub_patch("tc-any", &mut done);
    let mut later = echo("unscoped-SECRET", None);
    withheld.scrub_patch("tc-any", &mut later);
    assert!(!says(&later, "unscoped-SECRET"));
}

#[test]
fn a_declined_or_cancelled_answer_keeps_nothing() {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.requested(id(2), None);
    withheld.forget(id(1));
    withheld.forget(id(2));
    assert!(!withheld.covers("tc-1"));
    let mut patch = echo("anything-at-all", None);
    withheld.scrub_patch("tc-1", &mut patch);
    assert!(says(&patch, "anything-at-all"));
}

#[test]
fn a_scoped_set_is_released_when_its_call_completes_or_fails() {
    for status in [ToolCallStatus::Completed, ToolCallStatus::Failed] {
        let mut withheld = Withheld::default();
        withheld.requested(id(1), Some("tc-1".into()));
        withheld.accepted(id(1), &values(json!({ "v": "sk-SECRET" })));
        assert!(withheld.covers("tc-1"));
        // The completing update itself is scrubbed, then the set goes.
        let mut last = echo("sk-SECRET", Some(status));
        withheld.scrub_patch("tc-1", &mut last);
        assert!(!says(&last, "sk-SECRET"));
        assert!(!withheld.covers("tc-1"));
        let mut after = echo("sk-SECRET", None);
        withheld.scrub_patch("tc-1", &mut after);
        assert!(says(&after, "sk-SECRET"));
    }
}

#[test]
fn both_sets_apply_to_a_scoped_call() {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.requested(id(2), None);
    withheld.accepted(id(1), &values(json!({ "v": "abc" })));
    withheld.accepted(id(2), &values(json!({ "v": "xabcdefghx" })));
    assert_eq!(
        replace_all("abc xabcdefghx", &withheld.needles("tc-1").unwrap()).as_deref(),
        Some("[withheld] [withheld]")
    );
    assert_eq!(withheld.needles("tc-2").unwrap(), ["xabcdefghx"]);
}

#[test]
fn debug_prints_no_value() {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.accepted(id(1), &values(json!({ "v": "sk-SECRET" })));
    assert!(!format!("{withheld:?}").contains("sk-SECRET"));
}
