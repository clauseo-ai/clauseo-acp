//! `WebSearchRecorded` and `PageVisited` (the thread's web events) and `PageVia`, their open
//! enum: each round-trips in JSON and `MessagePack`, the optional fields are left out when
//! `None` and kept when set, and a `via` this version does not know decodes to `Unknown`.

use clauseo_protocol::{
    AgentId, DomainEvent, PageVia, PageVisited, Timestamp, WebResult, WebSearchRecorded,
};
use serde_json::{Value, json};

use crate::fixtures::{thread_id, tool_call_id, turn_id};

fn search(results: Vec<WebResult>) -> DomainEvent {
    WebSearchRecorded {
        thread: thread_id(2),
        turn: turn_id(3),
        tool_call_id: tool_call_id(4),
        agent: AgentId::new("claude-acp").unwrap(),
        query: "limitation period".to_owned(),
        results,
        at: Timestamp::new(1_760_000_000_000).unwrap(),
    }
    .into()
}

fn visit(title: Option<&str>, via: PageVia) -> DomainEvent {
    PageVisited {
        thread: thread_id(2),
        turn: turn_id(3),
        tool_call_id: tool_call_id(4),
        url: "https://example.com/a".to_owned(),
        title: title.map(str::to_owned),
        at: Timestamp::new(1_760_000_000_500).unwrap(),
        via,
    }
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
        WebResult {
            title: "Limitations Act, 2002".to_owned(),
            url: "https://www.ontario.ca/laws/statute/02l24".to_owned(),
        },
        WebResult {
            title: "Second".to_owned(),
            url: "https://example.com/second".to_owned(),
        },
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
            "thread": json["page_visited"]["thread"],
            "turn": json["page_visited"]["turn"],
            "tool_call_id": json["page_visited"]["tool_call_id"],
            "url": "https://example.com/a",
            "at": 1_760_000_000_500_u64,
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
            "thread": serde_json::to_value(thread_id(2)).unwrap(),
            "turn": serde_json::to_value(turn_id(3)).unwrap(),
            "tool_call_id": serde_json::to_value(tool_call_id(4)).unwrap(),
            "url": "https://example.com/a",
            "at": 1_760_000_000_500_u64,
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
            "thread": serde_json::to_value(thread_id(2)).unwrap(),
            "turn": serde_json::to_value(turn_id(3)).unwrap(),
            "tool_call_id": serde_json::to_value(tool_call_id(4)).unwrap(),
            "url": "https://example.com/a",
            "at": 1_760_000_000_500_u64,
            "via": 7,
        }
    });
    assert!(serde_json::from_value::<DomainEvent>(entry).is_err());
}
