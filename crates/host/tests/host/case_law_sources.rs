//! R24's case-law sources through the host, against S28's fake connector: a `search_law` and a
//! `read_document` through the relay, then the agent's completed tool calls with the same
//! arguments, log `WebSearchRecorded { source: CaseLaw }` with the fixture's titles and links and
//! one `PageVisited { via: CaseLaw }` per document; a search the relay never saw keeps its
//! query and no results, as does one whose relay result an earlier turn's end let go; a
//! reopened thread replays the records; a completion in a replay
//! (`Origin::Replay`) adds none, even with the relay's result waiting.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{
    DomainEvent, PageVia, PageVisited, SearchSource, StopReason, StreamId, WebResult,
    WebSearchRecorded,
};
use serde_json::{Value, json};

use crate::browser::Sequence;
use crate::connector_rig::{Rig, Start};
use crate::observe::Events;
use crate::support::{Dir, Scenario, matter_with_thread};

const CLAUDE: &str = "claude-acp";

/// A connection to the relay at `addr`, past the hello with `token`.
fn agent(addr: SocketAddr, token: &str) -> BufReader<TcpStream> {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let hello = json!({"clauseo_relay": 1, "token": token, "connector": "case-law"});
    stream.write_all(format!("{hello}\n").as_bytes()).unwrap();
    let mut reader = BufReader::new(stream);
    assert_eq!(line(&mut reader)["ok"], true);
    reader
}

fn line(reader: &mut BufReader<TcpStream>) -> Value {
    let mut text = String::new();
    reader.read_line(&mut text).unwrap();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

/// A `tools/call` of `tool` with `arguments` through the relay; its result, which the host
/// has heard before the line comes back.
fn call(reader: &mut BufReader<TcpStream>, id: u64, tool: &str, arguments: &Value) -> Value {
    let call = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}});
    reader
        .get_mut()
        .write_all(format!("{call}\n").as_bytes())
        .unwrap();
    let answer = line(reader);
    assert_eq!(answer["result"]["isError"], false, "{answer}");
    answer
}

fn searches(events: &Events) -> Vec<WebSearchRecorded> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::WebSearchRecorded(search) => Some((**search).clone()),
            _ => None,
        })
        .collect()
}

fn pages(events: &Events) -> Vec<PageVisited> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PageVisited(page) => Some(page.clone()),
            _ => None,
        })
        .collect()
}

fn result(title: &str, url: &str) -> WebResult {
    WebResult {
        title: title.to_owned(),
        url: url.to_owned(),
    }
}

#[test]
fn case_law_calls_through_the_relay_become_the_threads_sources() {
    let dir = Dir::new(
        "case_law_sources",
        &[(CLAUDE, Scenario::Local("case-law-sources"))],
    );
    let rig = Rig::start(&dir, Start { signed_in: true });
    let (matter, thread, _) = matter_with_thread(&dir, &rig.host, "m", &[], CLAUDE);
    let (session, token) = rig.relay_for(matter, thread, 1);
    let mut reader = agent(session.addr(), &token);
    // The relay's key order differs from the agent's: the join is on canonical JSON.
    call(
        &mut reader,
        1,
        "search_law",
        &json!({"query": "basic structure doctrine", "page": 1}),
    );
    call(
        &mut reader,
        2,
        "read_document",
        &json!({"doc_id": "1939993"}),
    );
    let mut events = rig.host.events(StreamId::Thread(thread));
    let (turn, _) = rig.host.prompt(thread, "Find the basic structure cases.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    let found = searches(&events);
    assert_eq!(found.len(), 2, "{}", events.dump());
    assert!(
        found
            .iter()
            .all(|search| search.source == SearchSource::CaseLaw
                && search.tool.as_deref() == Some("search_law"))
    );
    assert_eq!(found[0].query, "basic structure doctrine");
    assert_eq!(
        found[0].results,
        [
            result(
                "Kesavananda Bharati Sripadagalvaru v. State of Kerala (1973)",
                "https://indiankanoon.org/doc/257876/"
            ),
            result(
                "Minerva Mills Ltd. v. Union of India (1980)",
                "https://indiankanoon.org/doc/1939993/"
            ),
        ]
    );
    // The relay never saw this one: its query, no results.
    assert_eq!(found[1].query, "section 138 NI Act");
    assert_eq!(found[1].results, []);
    let read = pages(&events);
    assert_eq!(read.len(), 1, "{}", events.dump());
    assert_eq!(read[0].url, "https://indiankanoon.org/doc/1939993/");
    assert_eq!(read[0].via, PageVia::CaseLaw);
    assert_eq!(
        read[0].title.as_deref(),
        Some("Minerva Mills Ltd. v. Union of India (1980)")
    );
    // A window that opens the thread again reads the same records from the log.
    let mut reopened = rig.host.events(StreamId::Thread(thread));
    reopened.until("the records replayed", |events| {
        events.count(|event| matches!(event, DomainEvent::PageVisited(_))) == 1
    });
    assert_eq!(searches(&reopened), found);
    assert_eq!(pages(&reopened), read);
    drop(session);
    rig.finish();
}

/// A relay result no call of the turn took is let go when the turn ends: the next turn's
/// search with the same arguments keeps its query and no results.
#[test]
fn a_turns_end_lets_go_of_the_relays_results() {
    let dir = Dir::new(
        "case_law_cleared",
        &[(CLAUDE, Scenario::Local("case-law-cleared"))],
    );
    let rig = Rig::start(&dir, Start { signed_in: true });
    let (matter, thread, _) = matter_with_thread(&dir, &rig.host, "m", &[], CLAUDE);
    let (session, token) = rig.relay_for(matter, thread, 1);
    let mut reader = agent(session.addr(), &token);
    let arguments = json!({"query": "basic structure doctrine"});
    call(&mut reader, 1, "search_law", &arguments);
    let mut events = rig.host.events(StreamId::Thread(thread));
    let (first, _) = rig.host.prompt(thread, "Hello.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    let (second, _) = rig.host.prompt(thread, "Find the basic structure cases.");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    let found = searches(&events);
    assert_eq!(found.len(), 1, "{}", events.dump());
    assert_eq!(found[0].query, "basic structure doctrine");
    assert_eq!(found[0].results, [], "{}", events.dump());
    drop(session);
    rig.finish();
}

/// A case-law call left running when its agent exited, completed in the relaunched agent's
/// replay on `session/load` (`Origin::Replay`), with the relay's result for the same arguments
/// waiting: no record.
#[test]
fn a_replayed_case_law_completion_writes_nothing() {
    let dir = Dir::new(
        "case_law_replay",
        &[(CLAUDE, Scenario::Local("case-law-replay-seed"))],
    );
    let launcher = Arc::new(Sequence {
        inner: dir.launcher(),
        plays: std::sync::Mutex::new(vec![
            Scenario::Local("case-law-replay-seed"),
            Scenario::Local("case-law-replay-load"),
        ]),
    });
    let rig = Rig::start_launching(&dir, Start { signed_in: true }, launcher);
    let (matter, thread, _) = matter_with_thread(&dir, &rig.host, "m", &[], CLAUDE);
    let mut events = rig.host.events(StreamId::Thread(thread));
    let (first, _) = rig.host.prompt(thread, "Search.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    events.until("the first agent's session ended", |events| {
        events.count(|event| matches!(event, DomainEvent::AgentSessionDetached(_))) > 0
    });
    let (session, token) = rig.relay_for(matter, thread, 1);
    let mut reader = agent(session.addr(), &token);
    call(
        &mut reader,
        1,
        "search_law",
        &json!({"query": "basic structure doctrine"}),
    );
    let (second, _) = rig.host.prompt(thread, "And then?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    rig.host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(searches(&events), [], "{}", events.dump());
    drop(session);
    rig.finish();
}
