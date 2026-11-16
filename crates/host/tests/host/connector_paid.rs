//! Paid, and the host shows it at once (the second bug of #526's live row, where the balance
//! stayed 0 with no `last_recharge` until a `RefreshBalance`): the recharge watch's plain GETs
//! read the connector's paid answer, which has no `recharge_id` and no `payment`; and a paid
//! `recharge` tool result through a relay reads the balance at once, inside the read interval;
//! a resumed call's charge shows from its `resumed.balance_credits`, else its charge lines, else
//! a read; and repeats of a paid recharge do not each read the account.

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
pub(crate) fn call(
    reader: &mut BufReader<TcpStream>,
    id: u64,
    tool: &str,
    arguments: &Value,
) -> Value {
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

/// Out of credit holds the search (`pending_call`, `then`, and `next`'s `resume` naming it,
/// never `true`); paid, the watch reads the account, and the agent's `recharge` with that
/// `resume` runs the search, which spends 20: the shown balance is `resumed.balance_credits`
/// (the top-level one less `resumed.charged_credits`), though the top-level one is from before
/// the charge and the connector refuses the account read right after the watch's (its
/// per-account limit). The live R24 run: "Charged 40 credits · balance 1,960" while the host
/// showed 2,000.
#[test]
fn a_resumed_charge_shows_the_resumed_balance_credits() {
    let dir = Dir::new("connector_paid_resumed", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    rig.fake.set_balance(0);
    let held = search(&mut reader, 1);
    let structured = &held["result"]["structuredContent"];
    let pending = structured["pending_call"].as_str().unwrap().to_owned();
    assert!(pending.starts_with("pc_"), "{held}");
    assert!(structured["then"].is_string(), "{held}");
    let next = &structured["next"][0]["arguments"];
    assert_eq!(next["resume"], json!(pending), "{held}");
    let recharge = next["recharge_id"].as_str().unwrap().to_owned();
    assert!(rig.fake.mark_paid(&recharge));
    let paid = rig.fake.balance();
    rig.state_until("paid, read by the watch", |state| {
        case_law(state).is_some_and(|view| view.last_recharge == Some(RechargeOutcome::Paid))
            && signed_in(state).map(i64::from) == Some(paid)
    });
    let refused = br#"{"error": {"code": "rate_limited"}}"#;
    let json_type = [("Content-Type", "application/json")];
    rig.fake
        .answer_next("/v1/case-law/account", 429, &json_type, refused);
    let arguments = json!({"recharge_id": recharge, "resume": pending, "wait": true});
    let answer = call(&mut reader, 2, "recharge", &arguments);
    let structured = &answer["result"]["structuredContent"];
    assert_eq!(structured["recharge_id"], json!(recharge), "{answer}");
    assert_eq!(structured["balance_credits"], json!(paid), "{answer}");
    let resumed = &structured["resumed"];
    assert_eq!(resumed["tool"], "search_law", "{answer}");
    assert_eq!(resumed["in_content_blocks"], true, "{answer}");
    assert!(
        resumed["result"].is_object() && resumed["text"].is_string(),
        "{answer}"
    );
    let after = rig.fake.balance();
    assert_eq!(after, paid - 20);
    assert_eq!(resumed["charged_credits"], 20, "{answer}");
    assert_eq!(resumed["balance_credits"], json!(after), "{answer}");
    assert_eq!(
        resumed["charge_lines"],
        json!([format!("search_law: Charged 20 credits · balance {after}")]),
        "{answer}"
    );
    rig.state_until("the balance after the resumed charge", |state| {
        signed_in(state).map(i64::from) == Some(after)
    });
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// A paid `recharge` answer whose `resumed` is `resumed`, from before its charge at 2000.
fn paid_resumed(resumed: &Value) -> Fixture {
    let status = json!({"recharge_id": "rch_1", "status": "paid", "balance_credits": 2000,
        "resumed": resumed});
    Fixture::Json(
        json!({"content": [{"type": "text", "text": status.to_string()}],
        "structuredContent": status, "isError": false}),
    )
}

/// The connector before `resumed.balance_credits`: the balance its last charge line names
/// shows, with the account read refused.
#[test]
fn without_resumed_balance_credits_the_charge_line_s_balance_shows() {
    let dir = Dir::new("connector_paid_charge_line", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    let resumed = json!({"tool": "search_law", "in_content_blocks": true, "result": null,
        "text": "1. Daryao", "charge_lines": ["search_law: Charged 40 credits · balance 1,960"]});
    rig.fake
        .push_tool_result("recharge", paid_resumed(&resumed));
    let refused = br#"{"error": {"code": "rate_limited"}}"#;
    let json_type = [("Content-Type", "application/json")];
    rig.fake
        .answer_next("/v1/case-law/account", 429, &json_type, refused);
    let answer = call(&mut reader, 1, "recharge", &json!({"recharge_id": "rch_1"}));
    assert_eq!(answer["result"]["structuredContent"]["resumed"], resumed);
    rig.state_until("the charge line's balance", |state| {
        signed_in(state) == Some(1960)
    });
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// A charge line in a format not read, and no `resumed.balance_credits`: the balance from before
/// the charge (2000) never shows; the account is read and its balance (1940) shows.
#[test]
fn an_unread_charge_line_reads_the_account_and_never_shows_the_stale_balance() {
    let dir = Dir::new("connector_paid_unread_line", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    rig.fake.set_balance(1940);
    let resumed = json!({"tool": "search_law", "in_content_blocks": true, "result": null,
        "text": "1. Daryao", "charge_lines": ["search_law: 40 credits, ₹4.00 left"]});
    rig.fake
        .push_tool_result("recharge", paid_resumed(&resumed));
    let reads = rig.account_reads();
    call(&mut reader, 1, "recharge", &json!({"recharge_id": "rch_1"}));
    loop {
        let state = rig.next_state("the balance read after the resumed call");
        assert_ne!(signed_in(&state), Some(2000), "the stale balance showed");
        if signed_in(&state) == Some(1940) {
            break;
        }
    }
    assert_eq!(rig.account_reads(), reads + 1);
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// An agent that repeats `recharge` on a paid recharge: the first answer reads the account at
/// once; the four repeats of the same recharge read nothing more while that read is in flight,
/// and then wait for the one-a-minute slot it took.
#[test]
fn repeats_of_a_paid_recharge_read_the_account_at_most_twice() {
    let dir = Dir::new("connector_paid_repeats", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    let recharge = rig.handle.create_recharge("200").unwrap().recharge_id;
    assert!(rig.fake.mark_paid(&recharge));
    let reads = rig.account_reads();
    for id in 1..=5 {
        let answer = call(
            &mut reader,
            id,
            "recharge",
            &json!({"recharge_id": recharge}),
        );
        let structured = &answer["result"]["structuredContent"];
        assert_eq!(structured["status"], "paid", "{answer}");
        assert_eq!(structured["recharge_id"], json!(recharge), "{answer}");
    }
    wait("the read after the first answer", async {
        while rig.account_reads() == reads {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    let balance = rig.fake.balance();
    rig.state_until("the paid balance", |state| {
        signed_in(state).map(i64::from) == Some(balance)
    });
    drop(reader);
    session.end();
    drop(session);
    let extra = rig.account_reads() - reads;
    rig.finish();
    assert!(extra <= 2, "{extra} account reads for five answers");
}
