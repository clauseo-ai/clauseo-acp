//! `WebSearchRecorded` and `PageVisited` (the thread's web events) and `PageVia`, their open
//! enum: each round-trips in JSON and `MessagePack`, the optional fields are left out when
//! `None` and kept when set, and a `via` this version does not know decodes to `Unknown`. Their
//! constructors apply the caps (`MAX_*`), which the tests of this file pin.

use clauseo_protocol::{
    AgentId, DomainEvent, MAX_SEARCH_QUERY_CHARS, MAX_SEARCH_RESULTS, MAX_URL_CHARS,
    MAX_WEB_TITLE_CHARS, PageVia, PageVisited, WebResult, WebSearchRecorded,
};
use serde_json::{Value, json};

use crate::fixtures::{tool_call_id, turn_id};

fn search(results: Vec<WebResult>) -> DomainEvent {
    WebSearchRecorded::new(
        turn_id(3),
        tool_call_id(4),
        AgentId::new("claude-acp").unwrap(),
        "limitation period",
        results,
    )
    .into()
}

fn visit(title: Option<&str>, via: PageVia) -> DomainEvent {
    PageVisited::new(
        turn_id(3),
        tool_call_id(4),
        "https://example.com/a",
        title.map(str::to_owned),
        via,
    )
    .into()
}

/// The event as JSON, and as `MessagePack` decoded back to a value: each round trip must
/// give the same event.
fn round_trips(event: &DomainEvent) -> Value {
    let json = serde_json::to_value(event).unwrap();
    let back: DomainEvent = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(&back, event, "JSON round trip: {json}");
    let packed = rmp_serde::to_vec_named(event).unwrap();
    let unpacked: DomainEvent = rmp_serde::from_slice(&packed).unwrap();
    assert_eq!(&unpacked, event, "MessagePack round trip: {json}");
    json
}

#[test]
fn a_search_with_results_round_trips() {
    let results = vec![
        WebResult::new(
            "Limitations Act, 2002",
            "https://www.ontario.ca/laws/statute/02l24",
        ),
        WebResult::new("Second", "https://example.com/second"),
    ];
    let json = round_trips(&search(results));
    assert_eq!(json["web_search_recorded"]["agent"], json!("claude-acp"));
    assert_eq!(
        json["web_search_recorded"]["results"][1],
        json!({"title": "Second", "url": "https://example.com/second"})
    );
}

#[test]
fn a_search_with_no_results_keeps_an_empty_list() {
    let json = round_trips(&search(Vec::new()));
    assert_eq!(json["web_search_recorded"]["results"], json!([]));
}

#[test]
fn a_search_agent_is_any_registry_id() {
    let mut event = search(Vec::new());
    if let DomainEvent::WebSearchRecorded(search) = &mut event {
        search.agent = AgentId::new("fake-agent").unwrap();
    }
    let json = round_trips(&event);
    assert_eq!(json["web_search_recorded"]["agent"], json!("fake-agent"));
}

#[test]
fn a_visit_with_a_title_round_trips() {
    let json = round_trips(&visit(Some("Limitations Act"), PageVia::Browser));
    assert_eq!(json["page_visited"]["title"], json!("Limitations Act"));
    assert_eq!(json["page_visited"]["via"], json!("browser"));
}

#[test]
fn a_visit_without_a_title_leaves_it_out() {
    let json = round_trips(&visit(None, PageVia::WebFetch));
    assert_eq!(json["page_visited"].get("title"), None, "{json}");
    assert_eq!(json["page_visited"]["via"], json!("web_fetch"));
    let entry = json!({
        "page_visited": {
            "turn": json["page_visited"]["turn"],
            "tool_call": json["page_visited"]["tool_call"],
            "url": "https://example.com/a",
            "via": "codex_open_page",
        }
    });
    let read: DomainEvent = serde_json::from_value(entry).unwrap();
    assert_eq!(read, visit(None, PageVia::CodexOpenPage));
}

#[test]
fn every_known_via_has_its_wire_name() {
    let cases = [
        (PageVia::WebFetch, "web_fetch"),
        (PageVia::CodexOpenPage, "codex_open_page"),
        (PageVia::Browser, "browser"),
    ];
    for (via, name) in cases {
        assert_eq!(serde_json::to_value(via).unwrap(), json!(name));
        assert_eq!(serde_json::from_value::<PageVia>(json!(name)).unwrap(), via);
    }
}

#[test]
fn a_via_from_a_newer_clauseo_decodes_to_unknown_and_writes_back() {
    let entry = json!({
        "page_visited": {
            "turn": serde_json::to_value(turn_id(3)).unwrap(),
            "tool_call": serde_json::to_value(tool_call_id(4)).unwrap(),
            "url": "https://example.com/a",
            "via": "some_future_way",
        }
    });
    let read: DomainEvent = serde_json::from_value(entry).unwrap();
    let DomainEvent::PageVisited(visited) = &read else {
        panic!("not a page visit: {read:?}");
    };
    assert_eq!(visited.via, PageVia::Unknown);
    // The unit catch-all writes as `unknown`, which every version reads back as `Unknown`.
    let written = serde_json::to_value(&read).unwrap();
    assert_eq!(written["page_visited"]["via"], json!("unknown"));
    let again: DomainEvent = serde_json::from_value(written).unwrap();
    assert_eq!(again, read);
    let packed = rmp_serde::to_vec_named(&read).unwrap();
    let unpacked: DomainEvent = rmp_serde::from_slice(&packed).unwrap();
    assert_eq!(unpacked, read);
}

#[test]
fn a_via_that_is_not_a_string_is_refused() {
    let entry = json!({
        "page_visited": {
            "turn": serde_json::to_value(turn_id(3)).unwrap(),
            "tool_call": serde_json::to_value(tool_call_id(4)).unwrap(),
            "url": "https://example.com/a",
            "via": 7,
        }
    });
    assert!(serde_json::from_value::<DomainEvent>(entry).is_err());
}

/// The payloads hold no thread and no time: the envelope's `stream` and `ts` say both.
#[test]
fn the_payloads_leave_out_the_envelopes_thread_and_time() {
    let search = serde_json::to_value(search(Vec::new())).unwrap();
    let visited = serde_json::to_value(visit(Some("T"), PageVia::Browser)).unwrap();
    for (value, name) in [(&search, "web_search_recorded"), (&visited, "page_visited")] {
        let keys: Vec<&str> = value[name]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            !keys.contains(&"thread") && !keys.contains(&"at"),
            "{name}: {keys:?}"
        );
        assert!(
            keys.contains(&"turn") && keys.contains(&"tool_call"),
            "{name}: {keys:?}"
        );
    }
    assert_eq!(
        visited["page_visited"].as_object().unwrap().len(),
        5,
        "{visited}"
    );
}

/// The caps: each constant's value, and that each constructor cuts at it, on a char boundary.
#[test]
fn the_caps_are_the_documented_values() {
    assert_eq!(MAX_SEARCH_QUERY_CHARS, 1_000);
    assert_eq!(MAX_WEB_TITLE_CHARS, 300);
    assert_eq!(MAX_URL_CHARS, 4_096);
    assert_eq!(MAX_SEARCH_RESULTS, 50);
}

fn query_of(query: &str) -> String {
    let event = WebSearchRecorded::new(
        turn_id(3),
        tool_call_id(4),
        AgentId::new("claude-acp").unwrap(),
        query,
        Vec::new(),
    );
    event.query
}

#[test]
fn a_query_longer_than_the_cap_is_cut_to_it() {
    let long = "a".repeat(1_500);
    assert_eq!(query_of(&long).chars().count(), 1_000);
    assert_eq!(
        query_of(&"a".repeat(1_000)).len(),
        1_000,
        "at the cap: kept"
    );
    assert_eq!(query_of("short"), "short");
}

#[test]
fn a_multibyte_query_is_cut_at_a_char_boundary() {
    // Each `é` is two bytes: the cut is at 1,000 characters, which is 2,000 bytes, not at a byte.
    let cut = query_of(&"é".repeat(1_001));
    assert_eq!(cut.chars().count(), 1_000);
    assert_eq!(cut.len(), 2_000);
    assert!(cut.chars().all(|c| c == 'é'));
    // A four-byte character at the end of the cut is dropped whole, not split.
    let mixed = format!("{}{}", "a".repeat(999), "😀😀");
    let cut = query_of(&mixed);
    assert_eq!(cut.chars().count(), 1_000);
    assert_eq!(cut, format!("{}😀", "a".repeat(999)));
}

#[test]
fn a_search_keeps_the_first_fifty_results_in_order() {
    let results: Vec<WebResult> = (0..51)
        .map(|n| WebResult::new(format!("r{n}"), "https://example.com/x"))
        .collect();
    let DomainEvent::WebSearchRecorded(search) = search(results) else {
        panic!("not a search");
    };
    assert_eq!(search.results.len(), 50);
    assert_eq!(search.results[0].title, "r0");
    assert_eq!(search.results[49].title, "r49");
}

#[test]
fn a_result_title_is_cut_to_300_characters() {
    let long = WebResult::new("字".repeat(301), "https://example.com/x");
    assert_eq!(long.title.chars().count(), 300);
    assert!(long.title.chars().all(|c| c == '字'));
    let at_cap = WebResult::new("字".repeat(300), "https://example.com/x");
    assert_eq!(at_cap.title.chars().count(), 300, "at the cap: kept");
    // A visited page's title is capped the same way.
    let visited = PageVisited::new(
        turn_id(3),
        tool_call_id(4),
        "https://example.com/a",
        Some("字".repeat(301)),
        PageVia::Browser,
    );
    assert_eq!(visited.title.unwrap().chars().count(), 300);
}

#[test]
fn a_url_is_cut_to_4096_characters() {
    let long = format!("https://example.com/{}", "a".repeat(5_000));
    let result = WebResult::new("t", &long);
    assert_eq!(result.url.chars().count(), 4_096);
    assert!(result.url.starts_with("https://example.com/aaa"));
    let visited = PageVisited::new(turn_id(3), tool_call_id(4), &long, None, PageVia::WebFetch);
    assert_eq!(visited.url.chars().count(), 4_096);
}

#[test]
fn a_multibyte_url_is_cut_at_a_char_boundary() {
    // The query value is not a secret (it is not hex or base64url), so it is kept, and the cut
    // falls among the two-byte characters.
    let url = format!("https://example.com/?q={}", "é".repeat(5_000));
    let result = WebResult::new("t", &url);
    assert_eq!(result.url.chars().count(), 4_096);
    assert!(result.url.ends_with('é'));
    assert_eq!(result.url.len(), 23 + (4_096 - 23) * 2);
}

#[test]
fn a_secret_past_the_url_cap_is_redacted_before_the_cut() {
    // The 40-character hex value sits after 4,096 characters: the rule runs first, so the value
    // is gone whole, and the cut removes only what is left of its placeholder.
    let hex = "0123456789abcdef".repeat(2) + "01234567";
    // The filler is not a hex or base64url value (its `.`s), so it is kept and pushes the hex past
    // the cap.
    let url = format!("https://example.com/?x={}&ref={hex}", "a.".repeat(2_040));
    let result = WebResult::new("t", &url);
    assert!(!result.url.contains("0123456789abcdef"), "{}", result.url);
    assert_eq!(result.url.chars().count(), 4_096);
    assert!(result.url.starts_with("https://example.com/?x=a.a."));
}

#[test]
fn a_constructed_visit_logs_its_url_and_caps_its_title() {
    let visited = PageVisited::new(
        turn_id(3),
        tool_call_id(4),
        "https://user:pw@example.com/a?b=1&token=abc#frag",
        Some("字".repeat(301)),
        PageVia::Browser,
    );
    assert_eq!(visited.url, "https://example.com/a?b=1&token=REDACTED");
    assert_eq!(visited.title.unwrap().chars().count(), 300);
}
