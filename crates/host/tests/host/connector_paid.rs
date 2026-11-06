//! Paid, and the host shows it at once (the second bug of #526's live row, where the balance
//! stayed 0 with no `last_recharge` until a `RefreshBalance`): the recharge watch's plain GETs
//! read the connector's paid answer, which has no `recharge_id` and no `payment`; and a paid
//! `recharge` tool result through a relay reads the balance at once, inside the read interval.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use clauseo_connectors::fake::Fixture;
use clauseo_protocol::{ConnectorStatus, RechargeOutcome};
use serde_json::{Value, json};

use crate::connector_money::{agent, search};
use crate::connector_rig::{Rig, Start, case_law, signed_in, status};
use crate::support::{Dir, wait};

/// Out of credit through a relay (its recharge watched), then paid at checkout: the balance and
/// the paid outcome show with no `RefreshBalance`, and every poll was a plain GET (no `wait`, no
/// `resume`, which would claim the call the agent's own `recharge` resumes).
#[test]
fn a_checkout_paid_shows_the_balance_and_paid_without_a_refresh() {
    let dir = Dir::new("connector_paid_checkout", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    rig.fake.set_balance(0);
    search(&mut reader, 1);
    rig.state_until("out of credit", |state| {
        matches!(status(state), Some(ConnectorStatus::OutOfCredit { .. }))
    });
    let recharge = rig.fake.recharges().pop().unwrap();
    let polls = format!("/v1/case-law/recharges/{recharge}");
    wait("the watch's first poll", async {
        while rig.fake.requests_to(&polls).is_empty() {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    assert!(rig.fake.mark_paid(&recharge));
    let state = rig.state_until("paid", |state| {
        case_law(state).is_some_and(|view| view.last_recharge == Some(RechargeOutcome::Paid))
    });
    assert_eq!(signed_in(&state).map(i64::from), Some(rig.fake.balance()));
    let sent = rig.fake.requests_to(&polls);
    assert_ne!(sent.len(), 0);
    for poll in sent {
        assert_eq!((poll.method.as_str(), poll.query.as_str()), ("GET", ""));
    }
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// One `tools/call` through the relay, and its answer.
fn call(reader: &mut BufReader<TcpStream>, id: u64, tool: &str, arguments: &Value) -> Value {
    let call = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}});
    let stream = reader.get_mut();
    stream.write_all(format!("{call}\n").as_bytes()).unwrap();
    let mut text = String::new();
    reader.read_line(&mut text).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// A recharge the host does not watch (opened behind its back), paid, then the agent's own
/// `recharge` call, answered in the connector's shape, says so: the balance shows at once, with
/// no `RefreshBalance`.
#[test]
fn a_paid_recharge_result_through_the_relay_shows_the_balance() {
    let dir = Dir::new("connector_paid_relay", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    let recharge = rig.handle.create_recharge("200").unwrap().recharge_id;
    assert!(rig.fake.mark_paid(&recharge));
    let answer = call(
        &mut reader,
        1,
        "recharge",
        &json!({"recharge_id": recharge}),
    );
    assert_eq!(
        answer["result"]["structuredContent"]["status"], "paid",
        "{answer}"
    );
    let balance = rig.fake.balance();
    rig.state_until("the paid balance", |state| {
        signed_in(state).map(i64::from) == Some(balance)
    });
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// A paid `recharge` answer carries the balance before the call it resumed spent 60: the read
/// right after it shows that spend, from the connector, though the connectors crate's copy is
/// younger than a minute and the host's read interval has not passed.
#[test]
fn a_paid_recharge_result_reads_the_balance_from_the_connector_at_once() {
    let dir = Dir::new("connector_paid_fresh", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    rig.fake.set_balance(1940);
    let paid = json!({"status": "paid", "balance_credits": 2000, "resumed": null});
    rig.fake.push_tool_result(
        "recharge",
        Fixture::Json(
            json!({"content": [{"type": "text", "text": paid.to_string()}],
            "structuredContent": paid, "isError": false}),
        ),
    );
    let reads = rig.account_reads();
    let answer = call(&mut reader, 1, "recharge", &json!({"recharge_id": "rch_1"}));
    assert_eq!(answer["result"]["structuredContent"], paid);
    rig.state_until("the balance after the resumed call", |state| {
        signed_in(state) == Some(1940)
    });
    assert_eq!(rig.account_reads(), reads + 1);
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}
