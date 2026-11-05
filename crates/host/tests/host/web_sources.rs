//! S26's host side through the host: each agent's own web search and page read becomes a record on
//! the thread (`WebSearchRecorded`, `PageVisited`), from scenarios written from the two adapters'
//! source (claude-agent-acp 0.89.1 and codex-acp 2.2.2). The scenarios run under `claude-acp` and
//! `codex-acp` ids, and once under the `fake-agent` id, which has no kind and still records.

use clauseo_protocol::{
    AgentId, DomainEvent, PageVia, PageVisited, StopReason, StreamId, ToolCallId, WebResult,
    WebSearchRecorded,
};

use crate::browser::Sequence;
use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

const CLAUDE: &str = "claude-acp";
const CODEX: &str = "codex-acp";

/// The tool call the host logged for the agent's call `acp`.
fn started(events: &Events, acp: &str) -> ToolCallId {
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::ToolCallStarted(started) if started.acp_id.as_str() == acp => {
                Some(started.tool_call)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{acp} is logged:\n{}", events.dump()))
}

/// The `WebSearchRecorded` events of a thread's log.
fn searches(events: &Events) -> Vec<WebSearchRecorded> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::WebSearchRecorded(search) => Some((**search).clone()),
            _ => None,
        })
        .collect()
}

/// The `PageVisited` events of a thread's log.
fn pages(events: &Events) -> Vec<PageVisited> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PageVisited(page) => Some(page.clone()),
            _ => None,
        })
        .collect()
}

/// Runs one prompt of a fresh thread of `agent` on `scenario` to its end; returns the host, the
/// thread and the events read so far, with the first turn.
fn one_turn(test: &str, agent: &'static str, scenario: &'static str) -> OneTurn {
    let dir = Dir::new(test, &[(agent, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], agent);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the judgment.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    host.settle();
    events.drain_ready();
    OneTurn {
        dir,
        host,
        thread,
        turn,
        events,
    }
}

struct OneTurn {
    dir: Dir,
    host: Running,
    thread: clauseo_protocol::ThreadId,
    turn: clauseo_protocol::TurnId,
    events: Events,
}

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap()
}

/// The records of the Claude search: the query, and the links of both `Links:` arrays in order,
/// with the repeat of `b` dropped and the title with parentheses whole.
fn claude_results() -> Vec<WebResult> {
    vec![
        WebResult::new(
            "Lease Act (1882), section 105",
            "https://main.sci.gov.in/judgment/a",
        ),
        WebResult::new("Second judgment", "https://example.org/b"),
        WebResult::new("Third judgment", "https://indiacode.nic.in/c?id=9"),
    ]
}

/// Check 1, Claude `WebSearch`: one `WebSearchRecorded` at the completion, with the query, the
/// links in order, the agent, the turn and the call; published to the thread's subscriber; and
/// the reopened thread replays the same record.
#[test]
fn a_claude_search_is_one_record_with_its_links() {
    let run = one_turn("web_claude_search", CLAUDE, "claude-web-search");
    let logged = searches(&run.events);
    assert_eq!(logged.len(), 1, "{}", run.events.dump());
    let search = &logged[0];
    assert_eq!(search.query, "section 138 NI Act Supreme Court");
    assert_eq!(search.results, claude_results());
    assert_eq!(search.agent, agent(CLAUDE));
    assert_eq!(search.turn, run.turn);
    assert_eq!(search.tool_call, started(&run.events, "tc-search"));
    assert_eq!(pages(&run.events), [], "{}", run.events.dump());
    // The record comes after the call's completed update.
    let completed = run
        .events
        .position(|event| {
            matches!(event, DomainEvent::ToolCallUpdated(u)
                if u.patch.status == Some(clauseo_protocol::ToolCallStatus::Completed))
        })
        .unwrap();
    let recorded = run
        .events
        .position(|event| matches!(event, DomainEvent::WebSearchRecorded(_)))
        .unwrap();
    assert!(completed < recorded, "{}", run.events.dump());
    run.host.shutdown();

    let host = run.dir.start(1);
    let mut events = host.events(StreamId::Thread(run.thread));
    events.until_caught_up();
    assert_eq!(searches(&events), logged, "the reopened thread replays it");
    host.shutdown();
}

/// Check 1, Claude `WebSearch` with no parseable `Links:` array: the results are the `Title (url)`
/// lines of `content`, split at the last ` (`.
#[test]
fn a_claude_search_without_links_takes_its_results_from_the_content() {
    let run = one_turn("web_claude_fallback", CLAUDE, "claude-web-search-fallback");
    let logged = searches(&run.events);
    assert_eq!(logged.len(), 1, "{}", run.events.dump());
    assert_eq!(logged[0].query, "section 138");
    assert_eq!(
        logged[0].results,
        vec![
            WebResult::new(
                "Judgment on Section 138 (2024)",
                "https://main.sci.gov.in/a"
            ),
            WebResult::new("Second result", "https://example.org/b"),
        ]
    );
    run.host.shutdown();
}

/// Check 1, Claude `WebSearch` with no `name`: matched by kind `fetch` and the title
/// `Search "<q>"`.
#[test]
fn a_nameless_claude_search_is_matched_by_its_title() {
    let run = one_turn("web_claude_no_name", CLAUDE, "claude-web-search-no-name");
    let logged = searches(&run.events);
    assert_eq!(logged.len(), 1, "{}", run.events.dump());
    assert_eq!(logged[0].query, "fee");
    assert_eq!(
        logged[0].results,
        vec![WebResult::new("Fee", "https://example.org/fee")]
    );
    run.host.shutdown();
}

/// Check 1, Claude `WebFetch`: a page per fetch, with no title, even when the page is the same.
#[test]
fn each_claude_fetch_is_one_page_with_no_title() {
    let run = one_turn("web_claude_fetch", CLAUDE, "claude-web-fetch");
    let logged = pages(&run.events);
    assert_eq!(logged.len(), 2, "{}", run.events.dump());
    for (page, acp) in logged.iter().zip(["tc-fetch-1", "tc-fetch-2"]) {
        assert_eq!(page.url, "https://sci.gov.in/judgments/a");
        assert_eq!(page.title, None);
        assert_eq!(page.via, PageVia::WebFetch);
        assert_eq!(page.turn, run.turn);
        assert_eq!(page.tool_call, started(&run.events, acp));
    }
    assert_eq!(searches(&run.events), [], "{}", run.events.dump());
    run.host.shutdown();
}

/// Check 2, Codex web search in the AIR shape: a search with its query and no results.
#[test]
fn a_codex_search_has_its_query_and_no_results() {
    let run = one_turn("web_codex_search", CODEX, "codex-web-search");
    let logged = searches(&run.events);
    assert_eq!(logged.len(), 1, "{}", run.events.dump());
    assert_eq!(logged[0].query, "section 138 NI Act judgment");
    assert_eq!(logged[0].results, []);
    assert_eq!(logged[0].agent, agent(CODEX));
    assert_eq!(logged[0].tool_call, started(&run.events, "tc-ws"));
    run.host.shutdown();
}

/// Check 2, the same search in the standard shape: the same record.
#[test]
fn a_codex_search_in_the_standard_shape_is_the_same_record() {
    let air = one_turn("web_codex_air", CODEX, "codex-web-search");
    let standard = one_turn("web_codex_standard", CODEX, "codex-web-search-standard");
    let air_search = searches(&air.events);
    let standard_search = searches(&standard.events);
    assert_eq!(air_search.len(), 1);
    assert_eq!(standard_search.len(), 1);
    assert_eq!(air_search[0].query, standard_search[0].query);
    assert_eq!(air_search[0].results, standard_search[0].results);
    air.host.shutdown();
    standard.host.shutdown();
}

/// Check 2, Codex open page and find in page: one `CodexOpenPage` page each, with the fragment
/// dropped; no search; and the `other` action records nothing.
#[test]
fn codex_open_and_find_in_page_are_one_page_each() {
    let run = one_turn("web_codex_open", CODEX, "codex-web-open");
    let logged = pages(&run.events);
    assert_eq!(logged.len(), 2, "{}", run.events.dump());
    assert_eq!(logged[0].url, "https://indiacode.nic.in/show?x=1");
    assert_eq!(logged[0].via, PageVia::CodexOpenPage);
    assert_eq!(logged[0].tool_call, started(&run.events, "tc-open"));
    assert_eq!(logged[1].url, "https://sci.gov.in/j/1");
    assert_eq!(logged[1].via, PageVia::CodexOpenPage);
    assert_eq!(logged[1].tool_call, started(&run.events, "tc-find"));
    assert_eq!(searches(&run.events), [], "{}", run.events.dump());
    run.host.shutdown();
}

/// Only the two web tools count: an MCP fetch, a named search, and a search with no or an empty
/// query record nothing.
#[test]
fn other_tools_and_empty_searches_record_nothing() {
    let run = one_turn("web_other_tools", CLAUDE, "web-other-tools");
    assert_eq!(searches(&run.events), [], "{}", run.events.dump());
    assert_eq!(pages(&run.events), [], "{}", run.events.dump());
    run.host.shutdown();
}

/// A failed Claude search, and a Codex search still open when the turn ends: nothing recorded.
#[test]
fn failed_and_unfinished_calls_record_nothing() {
    let run = one_turn("web_failed", CLAUDE, "web-failed-and-open");
    assert_eq!(searches(&run.events), [], "{}", run.events.dump());
    assert_eq!(pages(&run.events), [], "{}", run.events.dump());
    run.host.shutdown();
}

/// The same records with no kind: the `fake-agent` id has no `AgentKind`, and the record names
/// the agent by its id, `fake-agent`.
#[test]
fn an_agent_with_no_kind_records_under_its_id() {
    let dir = Dir::new(
        "web_no_kind",
        &[(FAKE, Scenario::Local("claude-web-search"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find it.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    let logged = searches(&events);
    assert_eq!(logged.len(), 1, "{}", events.dump());
    assert_eq!(logged[0].agent, agent(FAKE));
    assert_eq!(logged[0].results, claude_results());
    host.shutdown();
}

/// `web-late`: a Claude fetch left running; the lawyer cancels; its completion arrives after the
/// cancel (`Origin::Late`) and records nothing.
#[test]
fn a_late_completion_records_nothing() {
    let dir = Dir::new("web_late", &[(CLAUDE, Scenario::Local("web-late"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Read it.");
    events.until("the fetch is logged", |events| {
        events.count(|event| matches!(event, DomainEvent::ToolCallStarted(s) if s.acp_id.as_str() == "tc-late")) == 1
    });
    assert_eq!(
        host.ok(clauseo_protocol::HostCommand::Cancel { thread }),
        clauseo_protocol::CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    let (second, _) = host.prompt(thread, "And then?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(pages(&events), [], "{}", events.dump());
    host.shutdown();
}

/// `web-replay-seed` then `web-replay-load`: the search is recorded in the first launch; the
/// relaunch's `session/load` replays the call and its completion (`Origin::Replay`), which adds no
/// second record.
#[test]
fn a_replayed_search_adds_no_second_record() {
    let dir = Dir::new(
        "web_replay",
        &[(CLAUDE, Scenario::Local("web-replay-seed"))],
    );
    let launcher = std::sync::Arc::new(Sequence {
        inner: dir.launcher(),
        plays: std::sync::Mutex::new(vec![
            Scenario::Local("web-replay-seed"),
            Scenario::Local("web-replay-load"),
        ]),
    });
    let host = dir.start_launching(0, launcher, |config| config);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Search.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    events.until("the first agent's session ended", |events| {
        events.count(|event| matches!(event, DomainEvent::AgentSessionDetached(_))) > 0
    });
    let (second, _) = host.prompt(thread, "And then?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(searches(&events).len(), 1, "{}", events.dump());
    host.shutdown();
}
