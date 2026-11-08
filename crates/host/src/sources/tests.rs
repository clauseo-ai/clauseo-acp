//! The sources as the host feeds them: each tool call's start and updates, through [`Sources`], and
//! the records they give. The parsing edge cases of each agent are in `claude/tests.rs` and
//! `codex/tests.rs`.

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    AgentId, CompactString, DomainEvent, PageVia, TextOrBlob, ToolCallContent, ToolCallId,
    ToolCallPatch, ToolCallStatus, ToolKind, TurnId, WebResult,
};
use serde_json::{Value, json};

use super::Sources;
use crate::pipeline::tests::support::id;

pub(super) fn turn() -> TurnId {
    id(3)
}

pub(super) fn tool_id() -> ToolCallId {
    id(4)
}

pub(super) fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

pub(super) fn inline(value: &Value) -> TextOrBlob {
    TextOrBlob::Inline(value.to_string())
}

/// A tool call as the host gets it from `acp`: `kind` and `name` as the adapter sends them.
pub(super) fn call(
    title: &str,
    kind: ToolKind,
    name: Option<&str>,
    status: ToolCallStatus,
    input: Option<&Value>,
) -> ToolCall {
    ToolCall {
        acp_id: "tc-1".into(),
        title: title.into(),
        kind,
        tool_name: name.map(Into::into),
        status,
        locations: Vec::new(),
        raw_input: input.map(inline),
        raw_output: None,
        content: Vec::new(),
    }
}

/// A completed update with `output` as `rawOutput` and `text` as one text block of `content`.
pub(super) fn completed(output: Option<&Value>, text: &[&str]) -> ToolCallPatch {
    ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        raw_output: output.map(inline),
        content: (!text.is_empty()).then(|| {
            text.iter()
                .map(|text| ToolCallContent::Text {
                    text: TextOrBlob::Inline((*text).to_owned()),
                })
                .collect()
        }),
        ..ToolCallPatch::default()
    }
}

/// A status-only update.
pub(super) fn status(status: ToolCallStatus) -> ToolCallPatch {
    ToolCallPatch {
        status: Some(status),
        ..ToolCallPatch::default()
    }
}

/// The search a record names: its query and results, or `None` when the event is no search.
pub(super) fn search_of(event: &DomainEvent) -> Option<(String, Vec<WebResult>)> {
    let DomainEvent::WebSearchRecorded(search) = event else {
        return None;
    };
    Some((search.query.clone(), search.results.clone()))
}

/// The page a record names: its URL, title and `via`, or `None` when the event is no page.
pub(super) fn page_of(event: &DomainEvent) -> Option<(String, Option<String>, PageVia)> {
    let DomainEvent::PageVisited(page) = event else {
        return None;
    };
    Some((page.url.clone(), page.title.clone(), page.via))
}

/// A Claude `WebSearch` started with an empty input, as the adapter starts it.
fn claude_search_start(sources: &mut Sources) -> Option<DomainEvent> {
    let started = call(
        "Web search",
        ToolKind::Fetch,
        Some("WebSearch"),
        ToolCallStatus::Pending,
        Some(&json!({})),
    );
    sources.started(turn(), tool_id(), &claude(), &started)
}

#[test]
fn a_claude_search_is_one_record_when_it_completes() {
    let mut sources = Sources::default();
    assert_eq!(claude_search_start(&mut sources), None);
    let refined = ToolCallPatch {
        title: Some(CompactString::from("Search \"lease act\"")),
        raw_input: Some(inline(&json!({"query": "lease act"}))),
        ..ToolCallPatch::default()
    };
    assert_eq!(sources.updated(turn(), "tc-1", &claude(), &refined), None);
    let done = completed(
        Some(&json!(
            "Web search results for query: \"lease act\"\n\nLinks: [{\"title\":\"Lease Act (1882)\",\"url\":\"https://example.org/a\"}]"
        )),
        &[],
    );
    let record = sources.updated(turn(), "tc-1", &claude(), &done).unwrap();
    assert_eq!(
        search_of(&record),
        Some((
            "lease act".to_owned(),
            vec![WebResult::new("Lease Act (1882)", "https://example.org/a")]
        ))
    );
    // Recorded once: a second completed update finds no call.
    assert_eq!(sources.updated(turn(), "tc-1", &claude(), &done), None);
}

#[test]
fn a_claude_search_that_fails_or_never_completes_records_nothing() {
    let mut sources = Sources::default();
    claude_search_start(&mut sources);
    let refined = ToolCallPatch {
        raw_input: Some(inline(&json!({"query": "lease act"}))),
        ..ToolCallPatch::default()
    };
    sources.updated(turn(), "tc-1", &claude(), &refined);
    assert_eq!(
        sources.updated(turn(), "tc-1", &claude(), &status(ToolCallStatus::Failed)),
        None
    );
    // The failed call is forgotten: a completion after it has nothing to record.
    assert_eq!(
        sources.updated(
            turn(),
            "tc-1",
            &claude(),
            &status(ToolCallStatus::Completed)
        ),
        None
    );
    // A call still open at the turn's end records nothing either.
    claude_search_start(&mut sources);
    sources.updated(turn(), "tc-1", &claude(), &refined);
    assert_eq!(
        sources.updated(
            turn(),
            "tc-1",
            &claude(),
            &status(ToolCallStatus::InProgress)
        ),
        None
    );
}

#[test]
fn a_no_name_claude_search_is_matched_by_its_title_once_refined() {
    let mut sources = Sources::default();
    // No `name`: the start is a `fetch` titled `Web search`, which is not yet a search.
    let started = call(
        "Web search",
        ToolKind::Fetch,
        None,
        ToolCallStatus::Pending,
        Some(&json!({})),
    );
    assert_eq!(
        sources.started(turn(), tool_id(), &claude(), &started),
        None
    );
    let refined = ToolCallPatch {
        title: Some(CompactString::from("Search \"fee\"")),
        raw_input: Some(inline(&json!({"query": "fee"}))),
        ..ToolCallPatch::default()
    };
    sources.updated(turn(), "tc-1", &claude(), &refined);
    let record = sources
        .updated(turn(), "tc-1", &claude(), &completed(None, &[]))
        .unwrap();
    assert_eq!(search_of(&record), Some(("fee".to_owned(), Vec::new())));
}

#[test]
fn a_second_fetch_of_the_same_url_is_a_second_record() {
    let mut sources = Sources::default();
    let input = json!({"url": "https://sci.gov.in/judgments/a", "prompt": "Summarise"});
    let mut records = Vec::new();
    for n in 0u64..2 {
        let acp = format!("tc-{n}");
        let started = ToolCall {
            acp_id: acp.as_str().into(),
            ..call(
                "Fetch https://sci.gov.in/judgments/a",
                ToolKind::Fetch,
                Some("WebFetch"),
                ToolCallStatus::Pending,
                Some(&input),
            )
        };
        sources.started(turn(), id(10 + n), &claude(), &started);
        let done = sources.updated(turn(), &acp, &claude(), &completed(None, &["Page text"]));
        records.push(done.unwrap());
    }
    assert_eq!(records.len(), 2);
    for record in &records {
        assert_eq!(
            page_of(record),
            Some((
                "https://sci.gov.in/judgments/a".to_owned(),
                None,
                PageVia::WebFetch
            ))
        );
    }
}

#[test]
fn a_call_that_starts_completed_is_recorded_at_once() {
    let mut sources = Sources::default();
    let started = call(
        "Fetch https://sci.gov.in/a",
        ToolKind::Fetch,
        Some("WebFetch"),
        ToolCallStatus::Completed,
        Some(&json!({"url": "https://sci.gov.in/a#top"})),
    );
    let record = sources
        .started(turn(), tool_id(), &claude(), &started)
        .unwrap();
    assert_eq!(
        page_of(&record),
        Some(("https://sci.gov.in/a".to_owned(), None, PageVia::WebFetch))
    );
}

#[test]
fn a_named_tool_other_than_the_two_web_tools_records_nothing() {
    let mut sources = Sources::default();
    let input = json!({"url": "https://evil.example/x", "query": "x"});
    for (title, kind, name) in [
        (
            "Fetch https://evil.example/x",
            ToolKind::Fetch,
            Some("mcp__docs__fetch"),
        ),
        ("Search \"x\"", ToolKind::Fetch, Some("WebLookup")),
        ("Web search: x", ToolKind::Search, Some("web_search")),
    ] {
        let started = call(title, kind, name, ToolCallStatus::Pending, Some(&input));
        assert_eq!(
            sources.started(turn(), tool_id(), &claude(), &started),
            None
        );
        assert_eq!(
            sources.updated(turn(), "tc-1", &claude(), &completed(None, &[])),
            None
        );
    }
}

#[test]
fn a_codex_search_with_no_query_key_records_nothing() {
    let mut sources = Sources::default();
    let started = call(
        "Web search",
        ToolKind::Search,
        None,
        ToolCallStatus::InProgress,
        Some(&json!({})),
    );
    sources.started(turn(), tool_id(), &claude(), &started);
    assert_eq!(
        sources.updated(turn(), "tc-1", &claude(), &completed(None, &[])),
        None
    );
}

/// Codex's search in the AIR shape: the start item with an empty query, then the completed item
/// with the query, as codex-acp sends it (`WebSearchReporter.ts:9-30`).
#[test]
fn codex_search_in_the_air_shape_is_one_record_with_no_results() {
    let codex = AgentId::new("codex-acp").unwrap();
    let mut sources = Sources::default();
    let started = call(
        "Web search",
        ToolKind::Search,
        None,
        ToolCallStatus::InProgress,
        Some(&json!({"query": "", "action": null})),
    );
    assert_eq!(sources.started(turn(), tool_id(), &codex, &started), None);
    let done = ToolCallPatch {
        title: Some(CompactString::from("Web search: section 138")),
        raw_input: Some(inline(&json!({
            "query": "section 138",
            "action": {"type": "search", "query": "section 138", "queries": ["section 138"]}
        }))),
        ..status(ToolCallStatus::Completed)
    };
    let record = sources.updated(turn(), "tc-1", &codex, &done).unwrap();
    assert_eq!(
        record,
        DomainEvent::from(clauseo_protocol::WebSearchRecorded::new(
            turn(),
            tool_id(),
            codex,
            "section 138",
            Vec::new()
        ))
    );
}

/// The same search in the standard shape, `{type: "webSearch", id, query, action}`.
#[test]
fn codex_search_in_the_standard_shape_is_the_same_record() {
    let codex = AgentId::new("codex-acp").unwrap();
    let mut air = Sources::default();
    let mut standard = Sources::default();
    let action = json!({"type": "search", "query": "section 138", "queries": ["section 138"]});
    let air_done = ToolCallPatch {
        raw_input: Some(inline(&json!({"query": "section 138", "action": action}))),
        ..status(ToolCallStatus::Completed)
    };
    let standard_done = ToolCallPatch {
        raw_input: Some(inline(&json!({
            "type": "webSearch", "id": "ws_1", "query": "section 138", "action": action
        }))),
        ..status(ToolCallStatus::Completed)
    };
    for sources in [&mut air, &mut standard] {
        let started = call(
            "Web search",
            ToolKind::Search,
            None,
            ToolCallStatus::InProgress,
            Some(&json!({"query": "", "action": null})),
        );
        sources.started(turn(), tool_id(), &codex, &started);
    }
    assert_eq!(
        air.updated(turn(), "tc-1", &codex, &air_done),
        standard.updated(turn(), "tc-1", &codex, &standard_done)
    );
    assert!(air.updated(turn(), "tc-1", &codex, &air_done).is_none());
}

/// The Codex open-page and find-in-page actions: one page each, and an `other` action none.
#[test]
fn codex_open_and_find_in_page_are_pages_and_other_is_nothing() {
    let codex = AgentId::new("codex-acp").unwrap();
    let mut sources = Sources::default();
    let actions = [
        json!({"type": "openPage", "url": "https://indiacode.nic.in/show?x=1#top"}),
        json!({"type": "findInPage", "url": "https://sci.gov.in/j/1", "pattern": "138"}),
        json!({"type": "other"}),
    ];
    let mut pages = Vec::new();
    for (n, action) in actions.iter().enumerate() {
        let acp = format!("tc-{n}");
        let started = ToolCall {
            acp_id: acp.as_str().into(),
            ..call(
                "Open page",
                ToolKind::Search,
                None,
                ToolCallStatus::InProgress,
                Some(&json!({"query": "", "action": action})),
            )
        };
        sources.started(turn(), id(10 + n as u64), &codex, &started);
        let done = sources.updated(turn(), &acp, &codex, &status(ToolCallStatus::Completed));
        pages.push(done);
    }
    assert_eq!(
        page_of(pages[0].as_ref().unwrap()),
        Some((
            "https://indiacode.nic.in/show?x=1".to_owned(),
            None,
            PageVia::CodexOpenPage
        ))
    );
    assert_eq!(
        page_of(pages[1].as_ref().unwrap()),
        Some((
            "https://sci.gov.in/j/1".to_owned(),
            None,
            PageVia::CodexOpenPage
        ))
    );
    assert_eq!(pages[2], None);
}

/// A call started in turn 3 and completed in turn 9 records nothing: its turn ended before the
/// completion, so no record can name the call's own turn.
#[test]
fn a_call_completed_in_a_later_turn_records_nothing() {
    let mut sources = Sources::default();
    claude_search_start(&mut sources);
    let refined = ToolCallPatch {
        title: Some(CompactString::from("Search \"lease\"")),
        raw_input: Some(inline(&json!({"query": "lease"}))),
        ..ToolCallPatch::default()
    };
    sources.updated(turn(), "tc-1", &claude(), &refined);
    let done = completed(Some(&json!("Links: []")), &[]);
    assert_eq!(sources.updated(id(9), "tc-1", &claude(), &done), None);
    // And the call is forgotten: a second completion in its own turn finds nothing either.
    assert_eq!(sources.updated(turn(), "tc-1", &claude(), &done), None);
}

/// A call that starts in a turn drops the unfinished calls of the turns before it: they can never
/// record now, so the map keeps only the current turn's calls.
#[test]
fn a_new_turn_drops_the_unfinished_calls_of_earlier_turns() {
    let mut sources = Sources::default();
    claude_search_start(&mut sources);
    let refined = ToolCallPatch {
        raw_input: Some(inline(&json!({"query": "lease"}))),
        ..ToolCallPatch::default()
    };
    sources.updated(turn(), "tc-1", &claude(), &refined);
    let other = call(
        "Web search",
        ToolKind::Fetch,
        Some("WebFetch"),
        ToolCallStatus::Pending,
        Some(&json!({"url": "https://sci.gov.in/a"})),
    );
    let other = ToolCall {
        acp_id: "tc-2".into(),
        ..other
    };
    assert_eq!(sources.started(id(9), id(11), &claude(), &other), None);
    let done = completed(Some(&json!("Links: []")), &[]);
    assert_eq!(sources.updated(turn(), "tc-1", &claude(), &done), None);
    let record = sources.updated(id(9), "tc-2", &claude(), &completed(None, &[]));
    assert!(matches!(record, Some(DomainEvent::PageVisited(_))));
}

/// At the cap a new unfinished call is refused, and the calls already open keep their records:
/// completing the last of the 64 still records (D4).
#[test]
fn the_unfinished_call_cap_refuses_a_new_call_and_keeps_the_open_ones() {
    let mut sources = Sources::default();
    let search = |n: usize| ToolCall {
        acp_id: format!("tc-{n}").into(),
        ..call(
            "Web search",
            ToolKind::Search,
            None,
            ToolCallStatus::InProgress,
            Some(&json!({"query": "", "action": null})),
        )
    };
    for n in 0..64 {
        assert_eq!(
            sources.started(turn(), id(100 + n as u64), &codex(), &search(n)),
            None
        );
    }
    // The 65th is refused: it is not tracked, so its completion records nothing.
    assert_eq!(
        sources.started(turn(), id(164), &codex(), &search(64)),
        None
    );
    let refused = ToolCallPatch {
        raw_input: Some(inline(&json!({"query": "late", "action": null}))),
        ..status(ToolCallStatus::Completed)
    };
    assert_eq!(sources.updated(turn(), "tc-64", &codex(), &refused), None);
    // The 64 open before it are kept: the last one still records.
    let done = ToolCallPatch {
        raw_input: Some(inline(&json!({"query": "live", "action": null}))),
        ..status(ToolCallStatus::Completed)
    };
    let record = sources
        .updated(turn(), "tc-63", &codex(), &done)
        .expect("tc-63 records");
    assert_eq!(search_of(&record), Some(("live".to_owned(), Vec::new())));
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

#[test]
fn a_url_with_a_space_a_control_or_a_bidi_control_is_unsafe() {
    use super::is_safe_url_text;
    assert!(is_safe_url_text("https://sci.gov.in/a?b=c%20d"));
    for url in [
        "https://a b.example/",
        "https://a.example/\u{7}",
        "https://a.example/\u{7f}",
        "https://a.example/\u{85}",
        "https://a.example/\u{200E}",
        "https://a.example/\u{200F}",
        "https://a.example/\u{061C}",
        "https://a.example/\u{202A}",
        "https://a.example/\u{202E}",
        "https://a.example/\u{2066}",
        "https://a.example/\u{2069}",
        "https://a.example/\n",
    ] {
        assert!(!is_safe_url_text(url), "{url:?}");
    }
}

/// The 2,000-character cap is on the URL the log keeps: the URL rule runs first, so a URL that the
/// rule lengthens (a secret value redacted to `REDACTED`) can go over, and one it shortens can
/// come under.
#[test]
fn the_url_cap_is_on_the_logged_url() {
    use super::record_url;
    // "https://a.example/" is 18 characters.
    let plain = |len: usize| format!("https://a.example/{}", "q".repeat(len - 18));
    assert_eq!(plain(2_000).chars().count(), 2_000);
    assert_eq!(
        record_url(&plain(2_000)).as_deref(),
        Some(plain(2_000).as_str())
    );
    assert_eq!(record_url(&plain(2_001)), None);
    // `?key=abc` is 8 characters, 2,000 in all; logged it is `?key=REDACTED`, 5 more: 2,005.
    let lengthened = format!("https://a.example/{}?key=abc", "q".repeat(2_000 - 26));
    assert_eq!(lengthened.chars().count(), 2_000);
    assert_eq!(record_url(&lengthened), None);
}

#[test]
fn a_title_drops_every_format_character_and_folds_whitespace_like_an_agent_name() {
    use super::clean_title;
    // Zero-width space, word joiner, BOM, soft hyphen and a tag character go, as in agent names.
    assert_eq!(
        clean_title("Sup\u{200B}reme\u{2060} Court\u{FEFF} Re\u{AD}port\u{E0041}"),
        "Supreme Court Report"
    );
    // The bidi controls go too, and whitespace runs, tabs and line breaks become one space.
    assert_eq!(clean_title("  a\u{202E}b\t\n\n c\u{2066}  "), "ab c");
    // The zero width joiner stays, as emoji and scripts need it.
    assert_eq!(clean_title("a\u{200D}b"), "a\u{200D}b");
}

#[test]
fn a_url_with_any_format_character_is_refused() {
    use super::is_safe_url_text;
    assert!(is_safe_url_text("https://sci.gov.in/judgment"));
    for bad in [
        "https://sci.gov.in/\u{200B}x",
        "https://sci.gov.in/\u{2060}x",
        "https://sci.gov.in/\u{FEFF}x",
        "https://sci.gov.in/\u{E0041}x",
        "https://sci.gov.in/\u{202E}x",
    ] {
        assert!(!is_safe_url_text(bad), "{bad:?}");
    }
}
