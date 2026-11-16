//! The case-law join: the relay's result taken by the agent's completed call with the same
//! arguments (in any key order), in Claude's form and Codex's; no entry, an old entry, a used
//! entry and an error result; the tools that record nothing; the step's charge.

use std::time::Duration;

use clauseo_protocol::{
    DomainEvent, PageVia, SearchSource, ToolCallCharged, ToolCallPatch, ToolCallStatus, ToolKind,
    WebResult,
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
        failed: false,
        charged: None,
    }
}

/// What the relay found for a call whose result said `failed` and `charged`.
fn charged(
    tool: CaseLawTool,
    arguments: &Value,
    failed: bool,
    charged: Option<u32>,
) -> CaseLawFound {
    CaseLawFound {
        failed,
        charged,
        ..found(tool, arguments, Vec::new())
    }
}

/// The charges among `records`, as (credits, refused).
fn charges(records: &[DomainEvent]) -> Vec<(u32, bool)> {
    records
        .iter()
        .filter_map(|record| match record {
            DomainEvent::ToolCallCharged(ToolCallCharged {
                credits, refused, ..
            }) => Some((*credits, *refused)),
            _ => None,
        })
        .collect()
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

/// Other arguments, another tool, or an error result: no join, and the error records no source.
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
    // The first error took the entry (its own result), so a later success finds none.
    let ok = run(&mut case_law, at(2), SEARCH, &args, None);
    assert_eq!(search_of(&ok[0]).unwrap().1, []);
    // A result the relay saw as an error records no source either, whatever the agent said.
    case_law.found(at(3), charged(CaseLawTool::SearchLaw, &args, true, None));
    assert_eq!(run(&mut case_law, at(3), SEARCH, &args, None), []);
}

/// `account`, `recharge` and other servers' tools are not claimed, so the other sources see
/// them; a failed call with no entry records nothing.
#[test]
fn the_other_tools_are_not_claimed() {
    let mut case_law = CaseLaw::default();
    for title in [
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

/// A paid call's charge follows its sources; a free success shows none; an error that charged 0
/// is refused; no `_meta` key, no charge.
#[test]
fn a_completed_call_carries_its_charge() {
    let mut case_law = CaseLaw::default();
    let args = json!({"query": "x"});
    let paid = CaseLawFound {
        charged: Some(40),
        ..found(CaseLawTool::SearchLaw, &args, kesavananda())
    };
    case_law.found(at(0), paid);
    let records = run(&mut case_law, at(1), SEARCH, &args, None);
    assert!(matches!(records[0], DomainEvent::WebSearchRecorded(_)));
    assert_eq!(charges(&records), [(40, false)]);
    let DomainEvent::ToolCallCharged(charge) = &records[1] else {
        panic!("{records:?}");
    };
    assert_eq!((charge.turn, charge.tool_call), (turn(), tool_id()));
    for (failed, credits, want) in [
        (false, Some(1), vec![(1, false)]),
        (false, Some(0), vec![]),
        (true, Some(0), vec![(0, true)]),
        (false, None, vec![]),
        (true, None, vec![]),
    ] {
        case_law.found(
            at(2),
            charged(CaseLawTool::SearchLaw, &args, failed, credits),
        );
        let records = run(&mut case_law, at(2), SEARCH, &args, None);
        assert_eq!(charges(&records), want, "{failed} {credits:?}");
    }
}

/// `citator` and `check_citations` are claimed for their charge only; a call the agent says
/// failed still shows that it was refused; one still running shows nothing.
#[test]
fn charge_only_tools_and_failed_calls() {
    let mut case_law = CaseLaw::default();
    let args = json!({"doc_id": "257876"});
    case_law.found(at(0), charged(CaseLawTool::Citator, &args, false, Some(12)));
    let records = run(&mut case_law, at(0), "mcp__case-law__citator", &args, None);
    assert_eq!(charges(&records), [(12, false)]);
    assert_eq!(records.len(), 1);
    let check = json!({"server": "case-law", "tool": "check_citations", "arguments": args});
    case_law.found(
        at(0),
        charged(CaseLawTool::CheckCitations, &args, true, Some(0)),
    );
    let started = call(
        "mcp.case-law.check_citations",
        ToolKind::Other,
        None,
        ToolCallStatus::InProgress,
        Some(&check),
    );
    assert_eq!(
        case_law.started(at(0), turn(), tool_id(), &claude(), &started),
        Some(Vec::new())
    );
    let failed = ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        ..ToolCallPatch::default()
    };
    let records = case_law
        .updated(at(0), turn(), "tc-1", &claude(), &failed)
        .unwrap();
    assert_eq!(charges(&records), [(0, true)]);
}

/// Two calls with one key in flight together: different charges show on neither; equal ones
/// show on both.
#[test]
fn identical_calls_with_different_charges_show_none() {
    let mut case_law = CaseLaw::default();
    let args = json!({"doc_id": "1939993"});
    let read = "mcp__case-law__read_document";
    case_law.found(
        at(0),
        charged(CaseLawTool::ReadDocument, &args, false, Some(30)),
    );
    case_law.found(
        at(0),
        charged(CaseLawTool::ReadDocument, &args, false, Some(0)),
    );
    assert_eq!(charges(&run(&mut case_law, at(1), read, &args, None)), []);
    assert_eq!(charges(&run(&mut case_law, at(1), read, &args, None)), []);
    case_law.found(
        at(2),
        charged(CaseLawTool::ReadDocument, &args, false, Some(30)),
    );
    case_law.found(
        at(2),
        charged(CaseLawTool::ReadDocument, &args, false, Some(30)),
    );
    assert_eq!(
        charges(&run(&mut case_law, at(3), read, &args, None)),
        [(30, false)]
    );
    assert_eq!(
        charges(&run(&mut case_law, at(3), read, &args, None)),
        [(30, false)]
    );
    // One after the other (the first taken before the second came back): each its own.
    case_law.found(
        at(4),
        charged(CaseLawTool::ReadDocument, &args, false, Some(30)),
    );
    assert_eq!(
        charges(&run(&mut case_law, at(4), read, &args, None)),
        [(30, false)]
    );
    case_law.found(
        at(5),
        charged(CaseLawTool::ReadDocument, &args, false, Some(25)),
    );
    assert_eq!(
        charges(&run(&mut case_law, at(5), read, &args, None)),
        [(25, false)]
    );
}

/// Starts `title` in progress with `input`, then ends it `status` with no output.
fn end(
    case_law: &mut CaseLaw,
    title: &str,
    input: &Value,
    status: ToolCallStatus,
) -> Vec<DomainEvent> {
    let started = call(
        title,
        ToolKind::Other,
        None,
        ToolCallStatus::InProgress,
        Some(input),
    );
    case_law.started(at(0), turn(), tool_id(), &claude(), &started);
    let patch = ToolCallPatch {
        status: Some(status),
        ..ToolCallPatch::default()
    };
    case_law
        .updated(at(0), turn(), "tc-1", &claude(), &patch)
        .expect("a case-law call")
}

/// A call cancelled, or failed with no result waiting, leaves its result behind or gets it
/// late: a later identical call never shows that charge; another key is untouched.
#[test]
fn a_result_no_call_took_never_lends_its_charge() {
    let read = "mcp__case-law__read_document";
    let args = json!({"doc_id": "7"});
    let other = json!({"doc_id": "8"});
    // Cancelled with its result already waiting.
    let mut case_law = CaseLaw::default();
    case_law.found(
        at(0),
        charged(CaseLawTool::ReadDocument, &args, false, Some(30)),
    );
    assert_eq!(
        end(&mut case_law, read, &args, ToolCallStatus::Cancelled),
        []
    );
    assert_eq!(charges(&run(&mut case_law, at(1), read, &args, None)), []);
    // Cancelled before its result came back.
    let mut case_law = CaseLaw::default();
    end(&mut case_law, read, &args, ToolCallStatus::Cancelled);
    case_law.found(
        at(1),
        charged(CaseLawTool::ReadDocument, &args, false, Some(30)),
    );
    case_law.found(
        at(1),
        charged(CaseLawTool::ReadDocument, &other, false, Some(30)),
    );
    assert_eq!(charges(&run(&mut case_law, at(2), read, &args, None)), []);
    assert_eq!(
        charges(&run(&mut case_law, at(2), read, &other, None)),
        [(30, false)]
    );
    // Failed with nothing waiting, its result then late.
    let mut case_law = CaseLaw::default();
    assert_eq!(end(&mut case_law, read, &args, ToolCallStatus::Failed), []);
    case_law.found(
        at(1),
        charged(CaseLawTool::ReadDocument, &args, false, Some(30)),
    );
    assert_eq!(charges(&run(&mut case_law, at(2), read, &args, None)), []);
    // The one after that is its own again.
    case_law.found(
        at(3),
        charged(CaseLawTool::ReadDocument, &args, false, Some(2)),
    );
    assert_eq!(
        charges(&run(&mut case_law, at(3), read, &args, None)),
        [(2, false)]
    );
}
