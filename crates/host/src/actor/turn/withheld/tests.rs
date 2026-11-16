//! The withheld set: what an answer keeps, how it replaces, and when it lets go.

use clauseo_protocol::{
    ContentBlock, ElicitationId, MatterPath, RawFrom, RawPayload, TextOrBlob, ToolCallContent,
    ToolCallPatch, ToolCallStatus, ToolLocation, WITHHELD,
};
use serde_json::{Value, json};

use super::{Withheld, forms, replace_all};

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
fn a_scoped_answer_withholds_every_string_of_three_chars_or_more_from_its_call() {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.accepted(
        id(1),
        &values(
            json!({"api_key": "sk-SECRET", "note": "ok", "picks": ["Fees"], "n": 42,
            "flag": true, "blank": "", "spaced": " a "}),
        ),
    );
    // "ok" and " a " are under 3 chars once trimmed: not withheld.
    let mut patch = echo("sk-SECRET ok Fees 42 true a", None);
    withheld.scrub_patch("tc-1", &mut patch);
    assert!(!says(&patch, "sk-SECRET"), "{patch:?}");
    assert!(!says(&patch, "Fees"), "{patch:?}");
    assert_eq!(
        patch.title.as_deref(),
        Some("[withheld] ok [withheld] 42 true a")
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
    let (scoped, _) = withheld.needles("tc-1").unwrap();
    assert_eq!(
        replace_all("abc xabcdefghx", &scoped).as_deref(),
        Some("[withheld] [withheld]")
    );
    assert_eq!(withheld.needles("tc-2").unwrap().0, ["xabcdefghx"]);
}

#[test]
fn debug_prints_no_value() {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.accepted(id(1), &values(json!({ "v": "sk-SECRET" })));
    assert!(!format!("{withheld:?}").contains("sk-SECRET"));
}

/// A scoped set holding `value` for tc-1.
fn scoped(value: &str) -> Withheld {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.accepted(id(1), &values(json!({ "v": value })));
    withheld
}

fn raw_output(withheld: &mut Withheld, output: &Value) -> Value {
    let mut patch = ToolCallPatch {
        raw_output: Some(TextOrBlob::Inline(output.to_string())),
        ..ToolCallPatch::default()
    };
    withheld.scrub_patch("tc-1", &mut patch);
    let Some(TextOrBlob::Inline(text)) = &patch.raw_output else {
        panic!("{patch:?}");
    };
    serde_json::from_str(text).unwrap()
}

/// The review's `rev_uris_names_and_paths_are_not_scrubbed`: every string field of the call
/// and its content is scrubbed, paths and URIs included.
#[test]
fn paths_uris_names_and_every_other_string_field_are_scrubbed() {
    const S: &str = "sk-test-SECRET-123";
    let mut withheld = scoped(S);
    let mut patch = ToolCallPatch {
        tool_name: Some(format!("tool-{S}").into()),
        locations: Some(vec![
            ToolLocation::Inside {
                path: MatterPath::new(&format!("notes/{S}.txt")).unwrap(),
                line: Some(4),
            },
            ToolLocation::Outside {
                display: format!("/tmp/{S}/x"),
            },
        ]),
        content: Some(vec![
            ToolCallContent::Diff {
                path: ToolLocation::Outside {
                    display: format!("~/{S}.md"),
                },
                old: None,
                new: TextOrBlob::Inline(String::new()),
            },
            ToolCallContent::Content {
                content: ContentBlock::ResourceLink {
                    uri: format!("https://x.example/?key={S}"),
                    name: format!("name {S}"),
                    mime_type: Some(format!("text/{S}").into()),
                    title: Some(format!("title {S}")),
                    description: Some(format!("about {S}")),
                    size: None,
                },
            },
            ToolCallContent::Content {
                content: ContentBlock::Resource {
                    uri: format!("file:///{S}"),
                    mime_type: None,
                    text: None,
                    blob: None,
                },
            },
            ToolCallContent::Content {
                content: ContentBlock::Image {
                    data: TextOrBlob::Inline("AAAA".to_owned()),
                    mime_type: "image/png".into(),
                    uri: Some(format!("https://img.example/{S}.png")),
                },
            },
            ToolCallContent::Content {
                content: ContentBlock::Unsupported {
                    kind: format!("kind-{S}"),
                    raw_from: RawFrom::Acp,
                    raw: Some(TextOrBlob::Inline(json!({ "k": S }).to_string())),
                },
            },
        ]),
        ..ToolCallPatch::default()
    };
    withheld.scrub_patch("tc-1", &mut patch);
    assert!(!says(&patch, S), "{patch:?}");
    let locations = patch.locations.as_deref().unwrap();
    assert_eq!(
        locations.first(),
        Some(&ToolLocation::Inside {
            path: MatterPath::new("notes/[withheld].txt").unwrap(),
            line: Some(4),
        })
    );
}

/// The review's `rev_a_value_json_escaped_inside_a_string_is_not_scrubbed`: an MCP result is
/// JSON text inside a string, so the value sits there escaped.
#[test]
fn a_value_json_escaped_inside_a_string_or_text_is_scrubbed() {
    let value = "say \"hi\" C:\\dir";
    let mut withheld = scoped(value);
    let inner = json!({ "v": value }).to_string();
    assert_eq!(
        raw_output(&mut withheld, &json!({ "text": inner })),
        json!({"text": "{\"v\":\"[withheld]\"}"})
    );
    let mut patch = echo(&inner, None);
    withheld.scrub_patch("tc-1", &mut patch);
    assert_eq!(patch.title.as_deref(), Some("{\"v\":\"[withheld]\"}"));
}

/// The review's `rev_non_ascii_value_ascii_escaped_inside_a_string_is_not_scrubbed`: Python's
/// `ensure_ascii` writes `\u00fc`, in either hex case, and a surrogate pair beyond the BMP.
#[test]
fn a_value_ascii_escaped_is_scrubbed_in_both_hex_cases_and_beyond_the_bmp() {
    let mut withheld = scoped("Müller-Lüdenscheidt");
    for echoed in [
        "M\\u00fcller-L\\u00fcdenscheidt",
        "M\\u00FCller-L\\u00FCdenscheidt",
    ] {
        let mut patch = echo(&format!("got {echoed}"), None);
        withheld.scrub_patch("tc-1", &mut patch);
        assert_eq!(patch.title.as_deref(), Some("got [withheld]"), "{echoed}");
    }
    let mut emoji = scoped("key\u{1F600}abc");
    let mut patch = echo("x key\\ud83d\\ude00abc", None);
    emoji.scrub_patch("tc-1", &mut patch);
    assert_eq!(patch.title.as_deref(), Some("x [withheld]"));
    let mut patch = echo("x key\\uD83D\\uDE00abc", None);
    emoji.scrub_patch("tc-1", &mut patch);
    assert_eq!(patch.title.as_deref(), Some("x [withheld]"));
}

#[test]
fn the_forms_of_a_value() {
    let all: Vec<String> = forms(" Mü\"x ").iter().map(|f| f.to_string()).collect();
    assert_eq!(
        all,
        [
            " Mü\"x ",
            " M\\u00fc\\\"x ",
            " M\\u00FC\\\"x ",
            " Mü\\\"x ",
            "Mü\"x",
            "M\\u00fc\\\"x",
            "M\\u00FC\\\"x",
            "Mü\\\"x",
        ]
    );
}

/// The review's `rev_a_string_value_echoed_as_a_json_number_is_not_scrubbed`.
#[test]
fn a_value_echoed_as_a_json_number_is_scrubbed() {
    let mut withheld = scoped("12345678");
    assert_eq!(
        raw_output(
            &mut withheld,
            &json!({"pin": 12_345_678, "other": 1_234_567, "s": "12345678"})
        ),
        json!({"pin": WITHHELD, "other": 1_234_567, "s": WITHHELD})
    );
    let mut decimal = scoped("1.50");
    assert_eq!(
        raw_output(&mut decimal, &json!({ "fee": 1.5 })),
        json!({ "fee": WITHHELD })
    );
}

#[test]
fn a_value_pasted_with_spaces_matches_its_trimmed_echo() {
    let mut withheld = scoped("pasted-value-123 \n");
    let mut patch = echo("got=pasted-value-123.", None);
    withheld.scrub_patch("tc-1", &mut patch);
    assert_eq!(patch.title.as_deref(), Some("got=[withheld]."));
}

/// The review's `rev_two_withheld_keys_collapse_into_one`.
#[test]
fn keys_that_become_withheld_stay_apart() {
    let mut withheld = Withheld::default();
    withheld.requested(id(1), Some("tc-1".into()));
    withheld.accepted(id(1), &values(json!({"a": "first-key", "b": "second-key"})));
    assert_eq!(
        raw_output(&mut withheld, &json!({"first-key": 1, "second-key": 2})),
        json!({"[withheld]": 1, "[withheld] 2": 2})
    );
}
