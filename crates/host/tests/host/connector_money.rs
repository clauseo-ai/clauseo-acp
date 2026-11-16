//! R24 PR5: the money side of the connector state at the host, against S28's fake connector:
//! the account read at most once a minute (a paid result and `RefreshBalance`, by the host's
//! manual clock), and out of credit through a session's relay, with the pay link on our host
//! and on another, and the recharge's payment bringing the balance back.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use clauseo_host::SessionKey;
use clauseo_host::connectors::{RelayEvent, RelayEvents as _};
use clauseo_protocol::{ConnectorStatus, ConnectorsState, MatterId, ThreadId};
use serde_json::{Value, json};

use crate::connector_rig::{Rig, Start, case_law, refresh, signed_in, status};
use crate::support::{Dir, foreign_uuid};

/// A paid result as a relay reports it.
fn paid() -> RelayEvent {
    RelayEvent::PaidResult {
        session: SessionKey {
            matter: MatterId::new(foreign_uuid(1)).unwrap(),
            thread: ThreadId::new(foreign_uuid(2)).unwrap(),
            launch: 1,
        },
    }
}

/// Whether `state` shows a balance of `credits`.
fn balance_is(credits: i32) -> impl Fn(&ConnectorsState) -> bool {
    move |state| signed_in(state) == Some(credits)
}

/// The done-when's balance reads: a paid result reads the account; another within 60 s does
/// not, nor does `RefreshBalance`, at 0 s or at 60 s; one at 61 s does; `RefreshBalance` 61 s
/// after that does. Each read shows the fake's balance of the moment, so a read that should not
/// have happened would publish a balance the test never waits for, and hold the read slot so the
/// due one could not run.
#[test]
fn the_balance_is_read_at_most_once_a_minute() {
    let dir = Dir::new("connector_money_reads", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let clock = rig.host.clock.clone();
    // The check at start read the account from the connectors crate's copy, which adding the
    // key made: no request since, and the read slot taken at 0 s.
    let base = rig.account_reads();
    clock.advance(Duration::from_secs(61));
    rig.fake.set_balance(990);
    rig.events.event(paid());
    rig.state_until("the paid result's read", balance_is(990));
    assert_eq!(rig.account_reads(), base + 1);
    rig.fake.set_balance(985);
    rig.events.event(paid());
    rig.ok(refresh());
    clock.advance(Duration::from_secs(60));
    rig.events.event(paid());
    rig.ok(refresh());
    clock.advance(Duration::from_secs(1));
    rig.fake.set_balance(980);
    rig.events.event(paid());
    let seen = rig.state_until("the read at 61 s", |state| signed_in(state) != Some(990));
    assert_eq!(
        signed_in(&seen),
        Some(980),
        "a read inside the minute: {seen:?}"
    );
    assert_eq!(rig.account_reads(), base + 2);
    clock.advance(Duration::from_secs(61));
    rig.fake.set_balance(970);
    rig.ok(refresh());
    rig.state_until("the refresh at 122 s", balance_is(970));
    assert_eq!(rig.account_reads(), base + 3);
    rig.finish();
}

/// A connection to the relay at `addr`, past the hello with `token`.
pub(crate) fn agent(addr: SocketAddr, token: &str) -> BufReader<TcpStream> {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let hello = json!({"clauseo_relay": 1, "token": token, "connector": "case-law"});
    stream.write_all(format!("{hello}\n").as_bytes()).unwrap();
    let mut reader = BufReader::new(stream);
    let answer = line(&mut reader);
    assert_eq!(answer["ok"], true, "{answer}");
    reader
}

fn line(reader: &mut BufReader<TcpStream>) -> Value {
    let mut text = String::new();
    reader.read_line(&mut text).unwrap();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

/// A `search_law` call through the relay, and its result.
pub(crate) fn search(reader: &mut BufReader<TcpStream>, id: u64) -> Value {
    let call = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": "search_law", "arguments": {"query": "section 138 NI Act"}}});
    reader
        .get_mut()
        .write_all(format!("{call}\n").as_bytes())
        .unwrap();
    line(reader)
}

/// The done-when's out of credit, through a session's relay to the fake: the agent gets the
/// fake's result unchanged, the state shows the pay link mapped to our own host; paying that
/// recharge brings the state back to signed in with the new balance. A pay link on another host
/// shows as none.
#[test]
fn out_of_credit_shows_our_pay_link_and_payment_brings_the_balance() {
    let dir = Dir::new("connector_money_out_of_credit", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let (session, token) = rig.relay(1);
    let mut reader = agent(session.addr(), &token);
    rig.fake.set_balance(0);
    let result = search(&mut reader, 1);
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"], "insufficient_credits",
        "{result}"
    );
    let state = rig.state_until("out of credit", |state| {
        matches!(status(state), Some(ConnectorStatus::OutOfCredit { .. }))
    });
    let recharge = rig.fake.recharges().pop().unwrap();
    assert_eq!(
        status(&state),
        Some(&ConnectorStatus::OutOfCredit {
            pay_url: Some(format!("https://data.clauseo.chat/pay/{recharge}"))
        })
    );
    assert!(rig.fake.mark_paid(&recharge));
    let state = rig.state_until("paid", |state| signed_in(state).is_some_and(|c| c > 0));
    assert_eq!(case_law(&state).unwrap().pay_url, None);
    assert_eq!(signed_in(&state).map(i64::from), Some(rig.fake.balance()));

    rig.fake
        .set_pay_url(Some("https://pay.example.com/pay/rch_elsewhere"));
    rig.fake.set_balance(0);
    let result = search(&mut reader, 2);
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"], "insufficient_credits",
        "{result}"
    );
    rig.state_until("out of credit, paid elsewhere", |state| {
        status(state) == Some(&ConnectorStatus::OutOfCredit { pay_url: None })
    });
    drop(reader);
    session.end();
    drop(session);
    for json in rig.finish() {
        assert!(!json.contains(&token), "the relay token in {json}");
    }
}
