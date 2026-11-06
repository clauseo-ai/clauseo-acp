//! The case-law join: the relay's result taken by the agent's completed call with the same
//! arguments (in any key order), in Claude's form and Codex's; no entry, an old entry, a used
//! entry and an error result; the tools that record nothing.

use std::time::Duration;

use clauseo_protocol::{
    DomainEvent, PageVia, SearchSource, ToolCallPatch, ToolCallStatus, ToolKind, WebResult,
};
use serde_json::{Value, json};

use super::super::tests::{call, claude, inline, page_of, search_of, tool_id, turn};
use super::{CaseLaw, ENTRY_LIFE, canonical, join_key};
use crate::connectors::{CaseLawFound, CaseLawTool, FoundLink};

const SEARCH: &str = "mcp__case-law__search_law";
const CODEX_READ: &str = "mcp.case-law.read_document";

fn at(secs: u64) -> Duration {
    Duration::from_secs(secs)
}

fn link(title: &str, url: &str) -> FoundLink {
    FoundLink {
        title: title.to_owned(),
        url: url.to_owned(),
    }
}

/// What the relay found for `tool` called with `arguments`.
fn found(tool: CaseLawTool, arguments: &Value, links: Vec<FoundLink>) -> CaseLawFound {
    CaseLawFound {
        tool,
        key: join_key(tool, arguments),
        links,
    }
}

fn kesavananda() -> Vec<FoundLink> {
    vec![
        link(
            "Kesavananda Bharati v. State of Kerala (1973)",
            "https://indiankanoon.org/doc/257876/",
        ),
        link(
            "Minerva Mills Ltd. v. Union of India (1980)",
            "https://indiankanoon.org/doc/1939993/",
        ),
    ]
}

/// Starts `title` in progress with `input`, then completes it with `output`; the records.
fn run(
    case_law: &mut CaseLaw,
    now: Duration,
    title: &str,
    input: &Value,
    output: Option<&Value>,
) -> Vec<DomainEvent> {
    let started = call(
        title,
        ToolKind::Other,
        None,
        ToolCallStatus::InProgress,
        Some(input),
    );
    assert_eq!(
        case_law.started(now, turn(), tool_id(), &claude(), &started),
        Some(Vec::new())
    );
    let done = ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        raw_output: output.map(inline),
        ..ToolCallPatch::default()
    };
    case_law
        .updated(now, turn(), "tc-1", &claude(), &done)
        .expect("a case-law call")
}

#[test]
fn canonical_json_sorts_keys_at_every_level() {
    let mut one = String::new();
    canonical(
        &json!({"b": 1, "a": {"y": [2, {"q": 1, "p": 0}], "x": "s"}}),
        &mut one,
    );
    assert_eq!(one, r#"{"a":{"x":"s","y":[2,{"p":0,"q":1}]},"b":1}"#);
    let (first, second) = (
        json!({"query": "x", "page": 2}),
        json!({"page": 2, "query": "x"}),
    );
    assert_eq!(
        join_key(CaseLawTool::SearchLaw, &first),
        join_key(CaseLawTool::SearchLaw, &second)
    );
    assert_ne!(
        join_key(CaseLawTool::SearchLaw, &first),
        join_key(CaseLawTool::ReadDocument, &first)
    );
}

/// Claude's `search_law`: the relay's titles and links, under the agent's query, as a case-law
/// record naming the tool; the arguments match in another key order.
#[test]
fn a_search_joins_the_relays_result() {
    let mut case_law = CaseLaw::default();
    let relay_args = json!({"query": "basic structure doctrine", "page": 1});
    case_law.found(
        at(10),
        found(CaseLawTool::SearchLaw, &relay_args, kesavananda()),
    );
    let agent_args = json!({"page": 1, "query": "basic structure doctrine"});
    let records = run(&mut case_law, at(20), SEARCH, &agent_args, None);
    let [DomainEvent::WebSearchRecorded(record)] = records.as_slice() else {
        panic!("one search: {records:?}");
    };
    assert_eq!(record.source, SearchSource::CaseLaw);
    assert_eq!(record.tool.as_deref(), Some("search_law"));
    assert_eq!(record.tool_call, tool_id());
    assert_eq!(
        search_of(&records[0]).unwrap(),
        (
            "basic structure doctrine".to_owned(),
            vec![
                WebResult {
                    title: "Kesavananda Bharati v. State of Kerala (1973)".to_owned(),
                    url: "https://indiankanoon.org/doc/257876/".to_owned(),
                },
                WebResult {
                    title: "Minerva Mills Ltd. v. Union of India (1980)".to_owned(),
                    url: "https://indiankanoon.org/doc/1939993/".to_owned(),
                },
            ]
        )
    );
    // Taken: a second call with the same arguments finds no entry.
    let again = run(&mut case_law, at(30), SEARCH, &agent_args, None);
    assert_eq!(search_of(&again[0]).unwrap().1, []);
}

/// Codex's `read_document` (its title, its arguments under `arguments`): one page per
/// document; a link that is no web URL is dropped, an empty title becomes no title.
#[test]
fn a_codex_read_records_each_document() {
    let mut case_law = CaseLaw::default();
    let args = json!({"doc_id": "1939993"});
    let links = vec![
        link(
            "Minerva Mills Ltd. v. Union of India (1980)",
            "https://indiankanoon.org/doc/1939993/",
        ),
        link("Not a link", "javascript:alert(1)"),
        link("", "https://indiankanoon.org/doc/257876/"),
    ];
    case_law.found(at(0), found(CaseLawTool::ReadDocument, &args, links));
    let input = json!({"server": "case-law", "tool": "read_document", "arguments": args});
    let records = run(
        &mut case_law,
        at(1),
        CODEX_READ,
        &input,
        Some(&json!({"result": {}})),
    );
    let pages: Vec<_> = records.iter().filter_map(page_of).collect();
    assert_eq!(
        pages,
        [
            (
                "https://indiankanoon.org/doc/1939993/".to_owned(),
                Some("Minerva Mills Ltd. v. Union of India (1980)".to_owned()),
                PageVia::CaseLaw
            ),
            (
                "https://indiankanoon.org/doc/257876/".to_owned(),
                None,
                PageVia::CaseLaw
            ),
        ]
    );
}

/// No entry: a search records its query with no results, from each argument form; a read
/// records nothing.
#[test]
fn with_no_entry_a_search_keeps_its_query_and_a_read_records_nothing() {
    let mut case_law = CaseLaw::default();
    for (args, query) in [
        (
            json!({"query": "  section 138\tNI Act "}),
            "section 138 NI Act",
        ),
        (json!({"queries": ["bail", "parity"]}), "bail; parity"),
        (json!({"searches": [{"query": "a"}, "b"]}), "a; b"),
    ] {
        let records = run(&mut case_law, at(0), SEARCH, &args, None);
        assert_eq!(
            search_of(&records[0]).unwrap(),
            (query.to_owned(), vec![]),
            "{args}"
        );
        assert_eq!(records.len(), 1);
    }
    assert_eq!(
        run(&mut case_law, at(0), SEARCH, &json!({"page": 2}), None),
        []
    );
    let read = json!({"server": "case-law", "tool": "read_document", "arguments": {"doc_id": "1"}});
    assert_eq!(run(&mut case_law, at(0), CODEX_READ, &read, None), []);
}

/// An entry older than five minutes is not joined; one at five minutes is.
#[test]
fn an_entry_lives_five_minutes() {
    let mut case_law = CaseLaw::default();
    let args = json!({"query": "x"});
    case_law.found(at(0), found(CaseLawTool::SearchLaw, &args, kesavananda()));
    let late = run(&mut case_law, ENTRY_LIFE + at(1), SEARCH, &args, None);
    assert_eq!(search_of(&late[0]).unwrap().1, []);
    case_law.found(at(100), found(CaseLawTool::SearchLaw, &args, kesavananda()));
    let in_time = run(&mut case_law, at(100) + ENTRY_LIFE, SEARCH, &args, None);
    assert_eq!(search_of(&in_time[0]).unwrap().1.len(), 2);
}

/// Other arguments, another tool, or an error result: no join, and the error records nothing.
#[test]
fn only_the_same_call_joins_and_an_error_records_nothing() {
    let mut case_law = CaseLaw::default();
    let args = json!({"query": "x"});
    case_law.found(at(0), found(CaseLawTool::SearchLaw, &args, kesavananda()));
    let other = run(&mut case_law, at(1), SEARCH, &json!({"query": "y"}), None);
    assert_eq!(search_of(&other[0]).unwrap().1, []);
    for output in [
        json!({"isError": true}),
        json!({"error": "boom", "result": null}),
    ] {
        assert_eq!(
            run(&mut case_law, at(1), SEARCH, &args, Some(&output)),
            [],
            "{output}"
        );
    }
    // The entry is still there for the call that matches and succeeds.
    let ok = run(&mut case_law, at(2), SEARCH, &args, None);
    assert_eq!(search_of(&ok[0]).unwrap().1.len(), 2);
}

/// `citator`, `check_citations`, `account`, `recharge` and other servers' tools are not
/// claimed, so the other sources see them; a failed call records nothing.
#[test]
fn the_other_tools_are_not_claimed() {
    let mut case_law = CaseLaw::default();
    for title in [
        "mcp__case-law__citator",
        "mcp__case-law__check_citations",
        "mcp.case-law.account",
        "mcp__case-law__recharge",
        "mcp__browser__agent_browser_open",
        "WebSearch",
    ] {
        let started = call(
            title,
            ToolKind::Search,
            None,
            ToolCallStatus::InProgress,
            Some(&json!({})),
        );
        assert_eq!(
            case_law.started(at(0), turn(), tool_id(), &claude(), &started),
            None,
            "{title}"
        );
    }
    let started = call(
        SEARCH,
        ToolKind::Other,
        None,
        ToolCallStatus::InProgress,
        Some(&json!({"query": "x"})),
    );
    case_law.started(at(0), turn(), tool_id(), &claude(), &started);
    let failed = ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        ..ToolCallPatch::default()
    };
    assert_eq!(
        case_law.updated(at(0), turn(), "tc-1", &claude(), &failed),
        Some(vec![])
    );
    assert_eq!(
        case_law.updated(at(0), turn(), "tc-1", &claude(), &failed),
        None
    );
}
