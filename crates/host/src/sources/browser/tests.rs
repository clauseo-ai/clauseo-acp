//! Which tool calls become `PageVisited { via: Browser }`, and with what URL and title.

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    ContentBlock, PageVia, TextOrBlob, ToolCallContent, ToolCallId, ToolCallPatch, ToolCallStatus,
    ToolKind, TurnId,
};
use serde_json::json;

use super::{CLAUDE_OPEN, CODEX_OPEN, Opens};
use crate::pipeline::tests::support::id;

fn turn() -> TurnId {
    id(3)
}

fn tool_id() -> ToolCallId {
    id(4)
}

fn inline(value: &serde_json::Value) -> TextOrBlob {
    TextOrBlob::Inline(value.to_string())
}

fn call(title: &str, name: Option<&str>, input: &serde_json::Value) -> ToolCall {
    ToolCall {
        acp_id: "tc-1".into(),
        title: title.into(),
        kind: ToolKind::Other,
        tool_name: name.map(Into::into),
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: Some(inline(input)),
        raw_output: None,
        content: Vec::new(),
    }
}

fn completed(text: Option<&str>) -> ToolCallPatch {
    ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        content: text.map(|text| {
            vec![ToolCallContent::Text {
                text: TextOrBlob::Inline(text.to_owned()),
            }]
        }),
        ..ToolCallPatch::default()
    }
}

#[test]
fn a_claude_open_that_completes_is_one_page_with_its_title() {
    let mut opens = Opens::default();
    let started = call(
        "agent_browser_open",
        Some(CLAUDE_OPEN),
        &json!({"url": "https://sci.gov.in/"}),
    );
    assert_eq!(opens.started(turn(), tool_id(), &started), None);
    let page = opens
        .updated(
            turn(),
            "tc-1",
            &completed(Some("Supreme Court of India\nmore")),
        )
        .unwrap();
    assert_eq!(page.turn, turn());
    assert_eq!(page.tool_call, tool_id());
    assert_eq!(page.url, "https://sci.gov.in/");
    assert_eq!(page.title.as_deref(), Some("Supreme Court of India"));
    assert_eq!(page.via, PageVia::Browser);
    // Recorded once: a second completed update finds nothing.
    assert_eq!(opens.updated(turn(), "tc-1", &completed(None)), None);
}

#[test]
fn the_url_is_the_last_input_s() {
    let mut opens = Opens::default();
    let started = call(CLAUDE_OPEN, None, &json!({}));
    opens.started(turn(), tool_id(), &started);
    let input = ToolCallPatch {
        raw_input: Some(inline(&json!({"url": "https://a.example/"}))),
        status: Some(ToolCallStatus::InProgress),
        ..ToolCallPatch::default()
    };
    assert_eq!(opens.updated(turn(), "tc-1", &input), None);
    let last = ToolCallPatch {
        raw_input: Some(inline(&json!({"url": "https://b.example/#frag"}))),
        ..completed(None)
    };
    let page = opens.updated(turn(), "tc-1", &last).unwrap();
    // `PageVisited::new` applies the URL rule: the fragment goes.
    assert_eq!(page.url, "https://b.example/");
    assert_eq!(page.title, None);
}

#[test]
fn a_codex_open_takes_the_url_from_arguments_and_the_title_from_raw_output() {
    let mut opens = Opens::default();
    let started = call(
        CODEX_OPEN,
        None,
        &json!({"server": "browser", "tool": "agent_browser_open",
               "arguments": {"url": "https://indiacode.nic.in/"}}),
    );
    opens.started(turn(), tool_id(), &started);
    let done = ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        raw_output: Some(inline(&json!({
            "result": {"content": [{"type": "text", "text": "India Code"}], "isError": false},
            "error": null
        }))),
        ..ToolCallPatch::default()
    };
    let page = opens.updated(turn(), "tc-1", &done).unwrap();
    assert_eq!(page.url, "https://indiacode.nic.in/");
    assert_eq!(page.title.as_deref(), Some("India Code"));
}

#[test]
fn a_title_that_is_the_url_is_none() {
    let mut opens = Opens::default();
    opens.started(
        turn(),
        tool_id(),
        &call(CLAUDE_OPEN, None, &json!({"url": "https://x.example/"})),
    );
    let page = opens
        .updated(turn(), "tc-1", &completed(Some("https://x.example/")))
        .unwrap();
    assert_eq!(page.title, None);
}

#[test]
fn failed_cancelled_unfinished_and_other_tools_write_nothing() {
    let mut opens = Opens::default();
    for status in [ToolCallStatus::Failed, ToolCallStatus::Cancelled] {
        opens.started(
            turn(),
            tool_id(),
            &call(CLAUDE_OPEN, None, &json!({"url": "https://x.example/"})),
        );
        let patch = ToolCallPatch {
            status: Some(status),
            ..ToolCallPatch::default()
        };
        assert_eq!(opens.updated(turn(), "tc-1", &patch), None);
        // Forgotten: a later completed update records nothing.
        assert_eq!(opens.updated(turn(), "tc-1", &completed(None)), None);
    }
    // A click, and a tool merely named like the open.
    for (title, name) in [
        ("mcp__browser__agent_browser_click", None),
        ("mcp.browser.agent_browser_click", None),
        ("Open", Some("mcp__other__agent_browser_open")),
    ] {
        let mut click = call(title, name, &json!({"url": "https://x.example/"}));
        click.status = ToolCallStatus::Completed;
        assert_eq!(opens.started(turn(), tool_id(), &click), None, "{title}");
        assert_eq!(opens.updated(turn(), "tc-1", &completed(None)), None);
    }
    // An open with no url.
    opens.started(
        turn(),
        tool_id(),
        &call(CLAUDE_OPEN, None, &json!({"u": 1})),
    );
    assert_eq!(opens.updated(turn(), "tc-1", &completed(None)), None);
}

#[test]
fn an_open_that_starts_completed_is_recorded_at_once() {
    let mut opens = Opens::default();
    let mut started = call(CLAUDE_OPEN, None, &json!({"url": "https://x.example/"}));
    started.status = ToolCallStatus::Completed;
    started.content = vec![ToolCallContent::Content {
        content: ContentBlock::Text {
            text: "not a text item".to_owned(),
        },
    }];
    let page = opens.started(turn(), tool_id(), &started).unwrap();
    assert_eq!(page.url, "https://x.example/");
    assert_eq!(page.title, None);
}

#[test]
fn a_blob_input_is_passed_over() {
    let mut opens = Opens::default();
    let mut started = call(CLAUDE_OPEN, None, &json!({}));
    started.raw_input = None;
    opens.started(turn(), tool_id(), &started);
    assert_eq!(opens.updated(turn(), "tc-1", &completed(None)), None);
}

/// A completed open whose result is an error records nothing: Codex's `error`, `isError` at the
/// top or under `result`, and the guard's refusal text.
#[test]
fn a_completed_open_with_an_error_result_writes_nothing() {
    let outputs = [
        json!({"result": null, "error": "tool failed"}),
        json!({"result": {"content": [{"type": "text", "text": "x"}], "isError": true}, "error": null}),
        json!({"content": [{"type": "text", "text": "x"}], "isError": true}),
    ];
    for output in outputs {
        let mut opens = Opens::default();
        opens.started(
            turn(),
            tool_id(),
            &call(
                CODEX_OPEN,
                None,
                &json!({"arguments": {"url": "https://x.example/"}}),
            ),
        );
        let done = ToolCallPatch {
            status: Some(ToolCallStatus::Completed),
            raw_output: Some(inline(&output)),
            ..ToolCallPatch::default()
        };
        assert_eq!(opens.updated(turn(), "tc-1", &done), None, "{output}");
    }
    let mut opens = Opens::default();
    opens.started(
        turn(),
        tool_id(),
        &call(CLAUDE_OPEN, None, &json!({"url": "https://x.example/"})),
    );
    let refused = completed(Some("Clauseo does not allow --proxy in the browser."));
    assert_eq!(opens.updated(turn(), "tc-1", &refused), None);
    // A result that only mentions an error is still a page.
    let mut opens = Opens::default();
    opens.started(
        turn(),
        tool_id(),
        &call(CLAUDE_OPEN, None, &json!({"url": "https://x.example/"})),
    );
    let fine = ToolCallPatch {
        raw_output: Some(inline(
            &json!({"result": {"isError": false}, "error": null}),
        )),
        ..completed(Some("Errors and omissions"))
    };
    assert!(opens.updated(turn(), "tc-1", &fine).is_some());
}

/// Only web pages count: `about:blank` and other schemes are none, a value with no `:` gets
/// `https://`, and a last input that names no page undoes an earlier one.
#[test]
fn only_web_urls_are_pages() {
    let page = |url: &str| {
        let mut opens = Opens::default();
        opens.started(
            turn(),
            tool_id(),
            &call(CLAUDE_OPEN, None, &json!({"url": url})),
        );
        opens
            .updated(turn(), "tc-1", &completed(None))
            .map(|page| page.url)
    };
    assert_eq!(
        page("sci.gov.in/judgments"),
        Some("https://sci.gov.in/judgments".to_owned())
    );
    assert_eq!(
        page("  HTTP://x.example/ "),
        Some("HTTP://x.example/".to_owned())
    );
    for none in [
        "about:blank",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:text/html,x",
        "https://",
        "",
    ] {
        assert_eq!(page(none), None, "{none:?}");
    }
    let mut opens = Opens::default();
    opens.started(
        turn(),
        tool_id(),
        &call(CLAUDE_OPEN, None, &json!({"url": "https://x.example/"})),
    );
    let blank = ToolCallPatch {
        raw_input: Some(inline(&json!({"url": "about:blank"}))),
        ..completed(None)
    };
    assert_eq!(opens.updated(turn(), "tc-1", &blank), None);
}

/// A page's title loses control and bidi formatting characters.
#[test]
fn the_title_loses_control_and_bidi_characters() {
    let mut opens = Opens::default();
    opens.started(
        turn(),
        tool_id(),
        &call(CLAUDE_OPEN, None, &json!({"url": "https://x.example/"})),
    );
    let text = "\u{202E}Court\u{0007} \u{2066}order\u{2069}\u{200B}\tdocx";
    let page = opens
        .updated(turn(), "tc-1", &completed(Some(text)))
        .unwrap();
    // U+200B is no control or bidi formatting character, so it stays.
    assert_eq!(page.title.as_deref(), Some("Court order\u{200B}docx"));
    let mut opens = Opens::default();
    opens.started(
        turn(),
        tool_id(),
        &call(CLAUDE_OPEN, None, &json!({"url": "https://x.example/"})),
    );
    let page = opens
        .updated(turn(), "tc-1", &completed(Some("\u{202A}\u{0001} ")))
        .unwrap();
    assert_eq!(page.title, None);
}

/// The open is matched by its exact name: a name that holds it, or Codex's with more around it,
/// is another tool.
#[test]
fn a_name_holding_the_open_is_not_the_open() {
    for (title, name) in [
        ("x", Some("xmcp__browser__agent_browser_open")),
        ("x", Some("mcp__browser__agent_browser_open_v2")),
        ("mcp__browser__agent_browser_open ", None),
        ("mcp.browser.agent_browser_opener", None),
        ("Run mcp.browser.agent_browser_open", None),
    ] {
        let mut opens = Opens::default();
        let mut open = call(title, name, &json!({"url": "https://x.example/"}));
        open.status = ToolCallStatus::Completed;
        assert_eq!(
            opens.started(turn(), tool_id(), &open),
            None,
            "{title} {name:?}"
        );
    }
}
