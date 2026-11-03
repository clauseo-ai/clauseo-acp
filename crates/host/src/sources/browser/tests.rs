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
