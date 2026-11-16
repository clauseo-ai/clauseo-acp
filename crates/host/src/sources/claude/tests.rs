//! Claude's parsing edge cases: `Links:` arrays, the `Title (url)` fallback, the caps, and the
//! title match of a call with no name.

use clauseo_protocol::{
    MAX_SEARCH_RESULTS, MAX_WEB_TITLE_CHARS, ToolCallPatch, ToolCallStatus, ToolKind, WebResult,
};
use serde_json::json;

use super::{Tool, fetch, search, tool};
use crate::sources::Found;
use crate::sources::Sources;
use crate::sources::tests::{claude, completed, inline, tool_id, turn};
use clauseo_acp::ToolCall;

fn results_of(found: Option<Found>) -> Vec<WebResult> {
    match found.expect("a search") {
        Found::Search { results, .. } => results,
        other @ Found::Page { .. } => panic!("not a search: {other:?}"),
    }
}

fn text(lines: &str) -> Vec<String> {
    vec![lines.to_owned()]
}

#[test]
fn links_arrays_give_every_result_in_order_with_duplicates_dropped() {
    let output = json!(
        "Web search results for query: \"x\"\n\n\
         Links: [{\"title\":\"Lease Act (1882) (part 2)\",\"url\":\"https://a.example/1\"},\
                 {\"title\":\"Second\",\"url\":\"https://b.example/2\"}]\n\n\
         Links: [{\"title\":\"Second again\",\"url\":\"https://b.example/2\"},\
                 {\"title\":\"Third\",\"url\":\"https://c.example/3?token=abc\"}]"
    );
    let input = json!({"query": "x"});
    let results = results_of(search(Some(&input), Some(&output), &[]));
    assert_eq!(
        results,
        vec![
            WebResult::new("Lease Act (1882) (part 2)", "https://a.example/1"),
            WebResult::new("Second", "https://b.example/2"),
            WebResult::new("Third", "https://c.example/3?token=REDACTED"),
        ]
    );
}

#[test]
fn a_links_array_wins_over_the_content_lines() {
    let output = json!("Links: [{\"title\":\"From the array\",\"url\":\"https://a.example/\"}]");
    let input = json!({"query": "x"});
    let content = text("From the content (https://content.example/)");
    let results = results_of(search(Some(&input), Some(&output), &content));
    assert_eq!(
        results,
        vec![WebResult::new("From the array", "https://a.example/")]
    );
}

#[test]
fn an_empty_links_array_is_a_parse_and_gives_no_results() {
    let output = json!("Links: []");
    let input = json!({"query": "x"});
    let content = text("From the content (https://content.example/)");
    assert_eq!(
        results_of(search(Some(&input), Some(&output), &content)),
        Vec::new()
    );
}

#[test]
fn content_lines_are_the_fallback_when_no_array_parses_and_split_at_the_last_paren() {
    let output = json!("Links: [not json");
    let input = json!({"query": "section 138"});
    let content = text(
        "Judgment on Section 138 (2024) (https://main.sci.gov.in/a)\n\
         Second result (https://example.org/b)\n\
         not a result line",
    );
    let results = results_of(search(Some(&input), Some(&output), &content));
    assert_eq!(
        results,
        vec![
            WebResult::new(
                "Judgment on Section 138 (2024)",
                "https://main.sci.gov.in/a"
            ),
            WebResult::new("Second result", "https://example.org/b"),
        ]
    );
}

#[test]
fn a_result_url_that_is_not_web_is_dropped() {
    let output = json!(
        "Links: [{\"title\":\"Local\",\"url\":\"file:///etc/passwd\"},\
                 {\"title\":\"Web\",\"url\":\"https://example.org/ok\"}]"
    );
    let input = json!({"query": "x"});
    assert_eq!(
        results_of(search(Some(&input), Some(&output), &[])),
        vec![WebResult::new("Web", "https://example.org/ok")]
    );
}

#[test]
fn at_most_fifty_results_are_kept() {
    let links: Vec<_> = (0..60)
        .map(|n| json!({"title": format!("R{n}"), "url": format!("https://example.org/{n}")}))
        .collect();
    let output = json!(format!("Links: {}", serde_json::Value::Array(links)));
    let input = json!({"query": "x"});
    let results = results_of(search(Some(&input), Some(&output), &[]));
    assert_eq!(results.len(), MAX_SEARCH_RESULTS);
    assert_eq!(MAX_SEARCH_RESULTS, 50);
    assert_eq!(results.last().unwrap().title, "R49");
}

#[test]
fn a_title_is_cut_at_three_hundred_characters() {
    let long = "t".repeat(400);
    let output = json!(format!(
        "Links: [{{\"title\":\"{long}\",\"url\":\"https://a.example/\"}}]"
    ));
    let input = json!({"query": "x"});
    let results = results_of(search(Some(&input), Some(&output), &[]));
    assert_eq!(results[0].title.chars().count(), MAX_WEB_TITLE_CHARS);
    assert_eq!(MAX_WEB_TITLE_CHARS, 300);
}

#[test]
fn a_url_of_over_two_thousand_characters_is_dropped_and_one_of_two_thousand_is_kept() {
    let long_path = "p".repeat(2_000);
    let exact = format!(
        "https://a.example/{}",
        "q".repeat(2_000 - "https://a.example/".len())
    );
    let over = format!("https://b.example/{long_path}");
    assert_eq!(exact.chars().count(), 2_000);
    assert!(over.chars().count() > 2_000);
    let output = json!(format!(
        "Links: [{{\"title\":\"Over\",\"url\":\"{over}\"}},{{\"title\":\"Exact\",\"url\":\"{exact}\"}}]"
    ));
    let input = json!({"query": "x"});
    let results = results_of(search(Some(&input), Some(&output), &[]));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Exact");
}

#[test]
fn a_search_with_no_query_is_no_search() {
    let output = json!("Links: [{\"title\":\"A\",\"url\":\"https://a.example/\"}]");
    assert_eq!(search(Some(&json!({})), Some(&output), &[]), None);
    assert_eq!(search(None, Some(&output), &[]), None);
}

#[test]
fn a_fetch_is_its_url_with_no_title_and_a_non_web_url_is_no_page() {
    let input = json!({"url": "https://sci.gov.in/a#top", "prompt": "p"});
    assert_eq!(
        fetch(Some(&input)),
        Some(Found::Page {
            // The URL rule (the fragment) applies when the event is built, not here.
            url: "https://sci.gov.in/a#top".to_owned(),
            title: None,
            via: clauseo_protocol::PageVia::WebFetch,
        })
    );
    assert_eq!(fetch(Some(&json!({"url": "file:///x"}))), None);
    assert_eq!(fetch(Some(&json!({}))), None);
}

#[test]
fn the_tool_is_its_name_and_a_nameless_fetch_is_matched_by_its_title() {
    let query = json!({"query": "x"});
    let url = json!({"url": "https://a.example/"});
    assert_eq!(
        tool(ToolKind::Fetch, Some("WebSearch"), "Web search", None),
        Some(Tool::Search)
    );
    assert_eq!(
        tool(ToolKind::Fetch, Some("WebFetch"), "Fetch x", None),
        Some(Tool::Fetch)
    );
    assert_eq!(
        tool(
            ToolKind::Fetch,
            Some("mcp__a__b"),
            "Search \"x\"",
            Some(&query)
        ),
        None
    );
    // No name: the kind and the title say which, with the input's field.
    assert_eq!(
        tool(ToolKind::Fetch, None, "Search \"x\"", Some(&query)),
        Some(Tool::Search)
    );
    assert_eq!(
        tool(
            ToolKind::Fetch,
            None,
            "Fetch https://a.example/",
            Some(&url)
        ),
        Some(Tool::Fetch)
    );
    assert_eq!(
        tool(ToolKind::Search, None, "Search \"x\"", Some(&query)),
        None
    );
    assert_eq!(
        tool(ToolKind::Fetch, None, "Web search", Some(&query)),
        None
    );
    assert_eq!(tool(ToolKind::Fetch, None, "Search \"x\"", None), None);
}

/// A call with no `name`, started and refined, completes as a search of the query it names.
#[test]
fn a_nameless_claude_search_completes_as_a_search_of_its_refined_query() {
    let mut sources = Sources::default();
    let start = ToolCall {
        acp_id: "tc-1".into(),
        title: "Web search".into(),
        kind: ToolKind::Fetch,
        tool_name: None,
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: Some(inline(&json!({}))),
        raw_output: None,
        content: Vec::new(),
    };
    sources.started(turn(), tool_id(), &claude(), &start);
    let refine = ToolCallPatch {
        title: Some("Search \"lease\"".into()),
        raw_input: Some(inline(&json!({"query": "lease"}))),
        ..ToolCallPatch::default()
    };
    sources.updated(turn(), "tc-1", &claude(), &refine);
    let done = completed(Some(&json!("Links: []")), &[]);
    assert!(sources.updated(turn(), "tc-1", &claude(), &done).is_some());
}

/// D2: a `Title (url)`-shaped line whose "url" is no URL is no result: no scheme is added to it.
#[test]
fn text_in_parentheses_is_no_result_when_it_is_not_a_url() {
    let input = json!({"query": "x"});
    let content = text(
        "Supreme Court judgments (2024)\nNote (see below)\nSection 138 (a web address with a dot.example)",
    );
    assert_eq!(results_of(search(Some(&input), None, &content)), Vec::new());
    // The same goes for a `Links:` URL with no scheme.
    let output = json!("Links: [{\"title\":\"Bare\",\"url\":\"sci.gov.in/a\"}]");
    assert_eq!(
        results_of(search(Some(&input), Some(&output), &[])),
        Vec::new()
    );
}

/// D2: the record URL rule takes an explicit `http://` or `https://` and adds nothing.
#[test]
fn a_record_url_needs_an_explicit_web_scheme() {
    use crate::sources::record_url;
    assert_eq!(
        record_url("https://sci.gov.in/a").as_deref(),
        Some("https://sci.gov.in/a")
    );
    assert_eq!(
        record_url("HTTP://sci.gov.in/a").as_deref(),
        Some("HTTP://sci.gov.in/a")
    );
    for text in [
        "sci.gov.in/a",
        "https:/x",
        "https://",
        "ftp://a.example/",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "",
    ] {
        assert_eq!(record_url(text), None, "{text:?}");
    }
}

/// D3: a URL holding a bidi control, a space or a control character is dropped, not cleaned; the
/// URL beside it is kept.
#[test]
fn a_link_with_an_unsafe_character_is_dropped() {
    let links = json!([
        {"title": "Spoof", "url": "https://evil.example/\u{202E}fdp.exe"},
        {"title": "Space", "url": "https://a b.example/"},
        {"title": "Bell", "url": "https://c.example/\u{7}x"},
        {"title": "Fine", "url": "https://d.example/ok"},
    ]);
    let output = json!(format!("Links: {links}"));
    let input = json!({"query": "x"});
    assert_eq!(
        results_of(search(Some(&input), Some(&output), &[])),
        vec![WebResult::new("Fine", "https://d.example/ok")]
    );
}

/// D3: the same check on the fallback's `Title (url)` lines, and on a fetch's URL.
#[test]
fn an_unsafe_url_in_content_or_a_fetch_is_no_result_and_no_page() {
    let input = json!({"query": "x"});
    let content =
        text("Spoof (https://evil.example/\u{202E}fdp.exe)\nBell (https://c.example/\u{7}x)");
    assert_eq!(results_of(search(Some(&input), None, &content)), Vec::new());
    assert_eq!(
        fetch(Some(&json!({"url": "https://evil.example/\u{2066}x"}))),
        None
    );
}
