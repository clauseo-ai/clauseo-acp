//! A resumed charge's balance and the reads around it, finishing out of order: a recharge answer
//! from a watch begun before the charge never replaces the balance after it, though a read the
//! watch begins after the charge does; every resumed call whose balance does not read reads the
//! account, and one that comes while the paid result's read is in flight reads once more after.

use std::time::Duration;

use clauseo_connectors::fake::Fixture;
use clauseo_host::connectors::RelayEvents as _;
use clauseo_protocol::{ConnectorStatus, ConnectorsState, HostEvent, RechargeOutcome};
use serde_json::{Value, json};

use crate::connector_money::{agent, search};
use crate::connector_paid::call;
use crate::connector_rig::{Rig, Start, add_credits, case_law, signed_in, status};
use crate::support::{Dir, wait};

const ACCOUNT: &str = "/v1/case-law/account";
const MCP: &str = "/mcp/case-law";

/// A paid `recharge` answer for `recharge`, at 2000 before the call it resumed, if any.
fn paid(recharge: &str, resumed: &Value) -> Fixture {
    let status = json!({"recharge_id": recharge, "status": "paid", "balance_credits": 2000,
        "resumed": resumed});
    Fixture::Json(
        json!({"content": [{"type": "text", "text": status.to_string()}],
        "structuredContent": status, "isError": false}),
    )
}

/// The resumed search, with these fields.
fn resumed(fields: &Value) -> Value {
    let mut resumed = json!({"tool": "search_law", "in_content_blocks": true, "result": null,
        "text": "1. Daryao"});
    for (key, value) in fields.as_object().unwrap() {
        resumed[key] = value.clone();
    }
    resumed
}

/// Waits until the fake has had `count` account reads.
fn reads_reach(rig: &Rig, count: usize) {
    wait("the account read", async {
        while rig.account_reads() < count {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
}

/// The connectors' state as the host holds it now: a fresh subscription's first frames.
fn current(rig: &Rig) -> ConnectorsState {
    let (_sub, first) = crate::settings::subscribe_app(&rig.host);
    first
        .into_iter()
        .find_map(|event| match event {
            HostEvent::ConnectorsChanged(state) => Some(state),
            _ => None,
        })
        .expect("no connectors state in the catch-up")
}

/// Out of credit, the recharge watched; the agent's resume answers with
/// `resumed.balance_credits` 1955 before the watch's poll sees the payment; then the watch sees
/// it, with the paid answer's balance (200: the fake did not book the charge), and its account
/// read is refused. That answer may be from a poll before the resumed charge (the watch began
/// before it), so it never shows; the paid outcome does, and one more read brings the paid
/// balance (2155, the charge now booked). (The review's repro: the lawyer pays, and the resume
/// answers before the 3 s poll.)
#[test]
fn a_watch_older_than_the_resumed_charge_never_shows_its_balance_and_reads_again() {
    let dir = Dir::new("connector_resume_older_watch", &[]);
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
    let fields = json!({"balance_credits": 1955, "charged_credits": 45});
    rig.fake
        .push_tool_result("recharge", paid(&recharge, &resumed(&fields)));
    let arguments = json!({"recharge_id": recharge, "resume": "pc_1", "wait": true});
    call(&mut reader, 2, "recharge", &arguments);
    rig.state_until("the resumed balance", |state| {
        signed_in(state) == Some(1955)
    });
    let reads = rig.account_reads();
    let refused = br#"{"error": {"code": "rate_limited"}}"#;
    let json_type = [("Content-Type", "application/json")];
    let release = rig.fake.hold_next(ACCOUNT);
    rig.fake.answer_next(ACCOUNT, 429, &json_type, refused);
    assert!(rig.fake.mark_paid(&recharge));
    reads_reach(&rig, reads + 1);
    rig.fake.set_balance(2155);
    release.cancel();
    let mut paid = false;
    loop {
        let state = rig.next_state("paid, then the read after it");
        assert_ne!(signed_in(&state), Some(200), "the watch's balance showed");
        paid |=
            case_law(&state).is_some_and(|view| view.last_recharge == Some(RechargeOutcome::Paid));
        if signed_in(&state) == Some(2155) {
            break;
        }
    }
    assert!(paid, "the paid outcome showed");
    assert_eq!(rig.account_reads(), reads + 2);
    assert_eq!(signed_in(&current(&rig)), Some(2155));
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// `AddCredits` paid; the watch's account read is answered at 2155 and held, and meanwhile a
/// relay reports a resumed charge (the epoch moves) that brings the balance to 2100. The held
/// read lands but is from before that charge, so it is dropped, with the paid answer's balance;
/// one more read follows and shows 2100.
#[test]
fn a_watch_read_older_than_a_resumed_charge_mid_read_reads_again() {
    let dir = Dir::new("connector_resume_mid_read", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    rig.ok(add_credits("200"));
    rig.state_until("the pay link", |state| {
        case_law(state).is_some_and(|view| view.pay_url.is_some())
    });
    let recharge = rig.fake.recharges().pop().unwrap();
    rig.fake.set_balance(1955);
    let reads = rig.account_reads();
    let release = rig.fake.hold_next(ACCOUNT);
    assert!(rig.fake.mark_paid(&recharge));
    reads_reach(&rig, reads + 1);
    rig.events.resumed_charge();
    rig.fake.set_balance(2100);
    release.cancel();
    let mut paid = false;
    loop {
        let state = rig.next_state("paid, then the read after it");
        assert_ne!(
            signed_in(&state),
            Some(2155),
            "the read from before the charge showed"
        );
        paid |=
            case_law(&state).is_some_and(|view| view.last_recharge == Some(RechargeOutcome::Paid));
        if signed_in(&state) == Some(2100) {
            break;
        }
    }
    assert!(paid, "the paid outcome showed");
    assert_eq!(rig.account_reads(), reads + 2);
    assert_eq!(signed_in(&current(&rig)), Some(2100));
    rig.finish();
}

/// The lawyer opens `AddCredits` (its watch begins); the agent's resume of an earlier recharge
/// answers with `resumed.balance_credits` 1955; then the lawyer pays the new one, and the watch
/// reads the account: 1955 plus the pack's 200. That read began after the resumed charge, so it
/// shows, though the watch began before it.
#[test]
fn a_payment_after_the_resumed_charge_shows_though_its_watch_began_before() {
    let dir = Dir::new("connector_resume_newer_watch", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    rig.ok(add_credits("200"));
    rig.state_until("the pay link", |state| {
        case_law(state).is_some_and(|view| view.pay_url.is_some())
    });
    let recharge = rig.fake.recharges().pop().unwrap();
    let fields = json!({"balance_credits": 1955, "charged_credits": 45});
    rig.fake
        .push_tool_result("recharge", paid("rch_1", &resumed(&fields)));
    let arguments = json!({"recharge_id": "rch_1", "resume": "pc_1", "wait": true});
    call(&mut reader, 1, "recharge", &arguments);
    rig.state_until("the resumed balance", |state| {
        signed_in(state) == Some(1955)
    });
    rig.fake.set_balance(1955);
    assert!(rig.fake.mark_paid(&recharge));
    let state = rig.state_until("paid, seen by the watch", |state| {
        case_law(state).is_some_and(|view| view.last_recharge == Some(RechargeOutcome::Paid))
    });
    assert_eq!(signed_in(&state), Some(2155));
    assert_eq!(signed_in(&current(&rig)), Some(2155));
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// Two resumed calls on one recharge, neither balance read: each is new spend, so each reads
/// the account, though the one-a-minute slot is taken.
#[test]
fn a_second_resume_on_a_recharge_reads_again() {
    let dir = Dir::new("connector_resume_second", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    let reads = rig.account_reads();
    let lines = json!({"charge_lines": ["search_law: 60 credits, ₹6.00 left"]});
    let arguments = json!({"recharge_id": "rch_1", "resume": "pc_1", "wait": true});
    for (n, balance) in [(1, 1940), (2, 1880)] {
        rig.fake.set_balance(balance);
        rig.fake
            .push_tool_result("recharge", paid("rch_1", &resumed(&lines)));
        call(&mut reader, n, "recharge", &arguments);
        rig.state_until("the balance after the resumed call", |state| {
            signed_in(state).map(i64::from) == Some(balance)
        });
    }
    assert_eq!(rig.account_reads(), reads + 2);
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}

/// A plain paid result starts a read, answered at 2000 and held; the agent's resume charges 60
/// and its charge line does not read; the held answer lands. One more read follows it, and its
/// 1940 replaces the balance from before the charge.
#[test]
fn an_unread_resume_while_a_read_is_in_flight_reads_once_more_after_it() {
    let dir = Dir::new("connector_resume_read_again", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    rig.fake.set_balance(2000);
    let release = rig.fake.hold_next(ACCOUNT);
    let reads = rig.account_reads();
    rig.fake
        .push_tool_result("recharge", paid("rch_1", &Value::Null));
    call(&mut reader, 1, "recharge", &json!({"recharge_id": "rch_1"}));
    reads_reach(&rig, reads + 1);
    rig.fake.set_balance(1940);
    let lines = json!({"charge_lines": ["search_law: 60 credits, ₹6.00 left"]});
    rig.fake
        .push_tool_result("recharge", paid("rch_1", &resumed(&lines)));
    let posts = rig.fake.requests_to(MCP).len();
    std::thread::scope(|scope| {
        let resume = scope.spawn(|| {
            let arguments = json!({"recharge_id": "rch_1", "resume": "pc_1", "wait": true});
            call(&mut reader, 2, "recharge", &arguments)
        });
        wait("the resume's POST", async {
            while rig.fake.requests_to(MCP).len() == posts {
                async_io::Timer::after(Duration::from_millis(5)).await;
            }
        });
        release.cancel();
        resume.join().unwrap();
    });
    rig.state_until("the balance read after the resumed call", |state| {
        signed_in(state) == Some(1940)
    });
    assert_eq!(rig.account_reads(), reads + 2);
    assert_eq!(signed_in(&current(&rig)), Some(1940));
    drop(reader);
    session.end();
    drop(session);
    rig.finish();
}
