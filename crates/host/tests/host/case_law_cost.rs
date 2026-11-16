//! A case-law step's cost through the host, against S28's fake connector: each result's
//! `_meta["chat.clauseo/credits"]` puts a `ToolCallCharged` on the agent's matching step and its
//! balance in Settings. A paid call shows its credits; a free one (charged 0) shows none; a
//! refused one shows that nothing was charged; one with no key (an older server) shows none and
//! leaves the balance; two calls with the same arguments and different charges show none; a call
//! still running shows none. Balances that come back out of order never let an older one
//! overwrite a newer one.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use clauseo_connectors::fake::Fixture;
use clauseo_protocol::{DomainEvent, StopReason, StreamId, ToolCallCharged, ToolCallId};
use serde_json::{Value, json};

use crate::connector_rig::{Rig, Start, signed_in};
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

/// A result of the live server's shape, `isError` as given, with `credits` as its `_meta`
/// credits when some.
fn result(is_error: bool, credits: Option<Value>) -> Fixture {
    let mut value = json!({"content": [{"type": "text", "text": "Found."}],
        "structuredContent": {}, "isError": is_error});
    if let Some(credits) = credits {
        value["_meta"] = json!({"chat.clauseo/credits": credits});
    }
    Fixture::Json(value)
}

fn credits(charged: i64, balance: i64) -> Option<Value> {
    Some(json!({"charged_credits": charged, "balance_credits": balance}))
}

/// A `tools/call` of `tool` with `arguments` through the relay, answered by the next result
/// queued for `tool`; the answer, forwarded as the connector wrote it (its `_meta` included),
/// which the host has heard before the line comes back.
fn call(reader: &mut BufReader<TcpStream>, id: u64, tool: &str, arguments: &Value) -> Value {
    let call = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}});
    reader
        .get_mut()
        .write_all(format!("{call}\n").as_bytes())
        .unwrap();
    line(reader)
}

/// The charges logged, by the agent's own tool call id.
fn charges(events: &Events) -> BTreeMap<String, (u32, bool)> {
    let mut ids: BTreeMap<ToolCallId, String> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for event in events.events() {
        match event {
            DomainEvent::ToolCallStarted(started) => {
                ids.insert(started.tool_call, started.acp_id.to_string());
            }
            DomainEvent::ToolCallCharged(ToolCallCharged {
                tool_call,
                credits,
                refused,
                ..
            }) => {
                let acp = ids.get(tool_call).cloned().unwrap_or_default();
                assert!(
                    out.insert(acp, (*credits, *refused)).is_none(),
                    "one charge per call"
                );
            }
            _ => {}
        }
    }
    out
}

/// Each kind of result through the relay, then the agent's steps that match them: only the paid
/// one and the refused one get a charge, and the balance shows from the results.
#[test]
fn each_case_law_step_shows_what_its_own_result_said_it_cost() {
    let dir = Dir::new(
        "case_law_cost",
        &[(CLAUDE, Scenario::Local("case-law-cost"))],
    );
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (matter, thread, _) = matter_with_thread(&dir, &rig.host, "m", &[], CLAUDE);
    let (session, token) = rig.relay_for(matter, thread, 1);
    let mut reader = agent(session.addr(), &token);
    // Signed in with the fake's 1000 credits (`Rig::start` waited for them).

    // An older server: no key. The answer passes as it came, and the balance stays.
    rig.fake.push_tool_result("search_law", result(false, None));
    let answer = call(&mut reader, 1, "search_law", &json!({"query": "plain"}));
    assert_eq!(answer["result"].get("_meta"), None, "{answer}");
    // Paid: 40 credits, 960 left. The answer reaches the agent with its `_meta` unchanged.
    rig.fake.set_balance(960);
    rig.fake
        .push_tool_result("search_law", result(false, credits(40, 960)));
    let answer = call(&mut reader, 2, "search_law", &json!({"query": "paid"}));
    assert_eq!(
        answer["result"]["_meta"]["chat.clauseo/credits"],
        json!({"charged_credits": 40, "balance_credits": 960}),
        "{answer}"
    );
    rig.state_until("the paid call's balance", |state| {
        let balance = signed_in(state);
        // Nothing before it moved the balance: the plain call reported none.
        assert!(matches!(balance, Some(1000 | 960)), "{balance:?}");
        balance == Some(960)
    });
    // Free (a repeat open): charged 0, the balance.
    rig.fake
        .push_tool_result("read_document", result(false, credits(0, 960)));
    call(&mut reader, 3, "read_document", &json!({"doc_id": "1"}));
    // Refused (over max_credits, say): isError, charged 0, no balance.
    rig.fake
        .push_tool_result("citator", result(true, Some(json!({"charged_credits": 0}))));
    call(&mut reader, 4, "citator", &json!({"doc_id": "2"}));
    // Two reads with the same arguments in flight together: one paid, one free.
    rig.fake.set_balance(930);
    rig.fake
        .push_tool_result("read_document", result(false, credits(30, 930)));
    rig.fake
        .push_tool_result("read_document", result(false, credits(0, 930)));
    call(&mut reader, 5, "read_document", &json!({"doc_id": "3"}));
    call(&mut reader, 6, "read_document", &json!({"doc_id": "3"}));
    // A check whose step is still running when the turn ends.
    rig.fake
        .push_tool_result("check_citations", result(false, credits(0, 930)));
    let text = json!({"text": "AIR 1973 SC 1461"});
    call(&mut reader, 7, "check_citations", &text);
    rig.state_until("the twins' balance", |state| signed_in(state) == Some(930));

    let mut events = rig.host.events(StreamId::Thread(thread));
    let (turn, _) = rig.host.prompt(thread, "Find the basic structure cases.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    let charged = charges(&events);
    assert_eq!(
        charged,
        BTreeMap::from([
            ("tc-paid".to_owned(), (40, false)),
            ("tc-refused".to_owned(), (0, true)),
        ]),
        "{}",
        events.dump()
    );
    // The charge is on the step's row, and a window that opens the thread again reads it.
    let mut reopened = rig.host.events(StreamId::Thread(thread));
    reopened.until("the charges replayed", |events| {
        events.count(|event| matches!(event, DomainEvent::ToolCallCharged(_))) == 2
    });
    assert_eq!(charges(&reopened), charged);
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// Three paid results, the second from a call the connector booked before the first (so its
/// balance is higher), come back in that order: the balance shows the first, keeps it over the
/// second, then shows the third, and never the stale one.
#[test]
fn a_stale_balance_never_overwrites_a_newer_one() {
    let dir = Dir::new("case_law_cost_order", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    // What a read finds whenever it runs: the newest balance, so a read never shows 950.
    rig.fake.set_balance(880);
    for balance in [900, 950, 880] {
        rig.fake
            .push_tool_result("search_law", result(false, credits(50, balance)));
    }
    for id in 1..=3 {
        call(
            &mut reader,
            id,
            "search_law",
            &json!({"query": format!("q{id}")}),
        );
    }
    rig.state_until("the newest balance", |state| {
        let balance = signed_in(state);
        assert_ne!(balance, Some(950), "a stale balance showed");
        balance == Some(880)
    });
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// The fake connector's own `_meta` (`report_credits`): a paid search spends its price and
/// reports it with the balance after it, which shows in Settings.
#[test]
fn the_fakes_own_report_shows_the_balance() {
    let dir = Dir::new("case_law_cost_fake", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    rig.fake.report_credits(true);
    let answer = call(&mut reader, 1, "search_law", &json!({"query": "x"}));
    let reported = &answer["result"]["_meta"]["chat.clauseo/credits"];
    let balance = rig.fake.balance();
    assert_eq!(reported["balance_credits"], json!(balance), "{answer}");
    assert_eq!(
        reported["charged_credits"],
        json!(1000 - balance),
        "{answer}"
    );
    rig.state_until("the reported balance", |state| {
        signed_in(state).map(i64::from) == Some(balance)
    });
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}
