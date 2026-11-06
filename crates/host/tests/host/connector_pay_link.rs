//! `OpenPayLink` (R24, from the app review of #505): the host opens the pay link it holds, a
//! checked `PayUrl` from `AddCredits`, through the confirm path
//! (`ConnectorOpener::open_confirmed`), never the unconfirmed opener; with no link it refuses.
//! The command carries the connector's id and nothing else, so no URL can come from a window.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

use clauseo_protocol::{ConnectorCommand, ConnectorStatus, ErrorCode};
use serde_json::{Value, json};

use crate::connector_rig::{Rig, Start, add_credits, case_law, status};
use crate::support::Dir;

fn open_pay_link(id: &str) -> ConnectorCommand {
    ConnectorCommand::OpenPayLink { id: id.to_owned() }
}

#[test]
fn the_pay_link_opens_through_the_confirm_and_only_when_there_is_one() {
    let dir = Dir::new("connector_pay_link", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    // No link yet: refused, and nothing opens.
    let refused = rig.dispatch(open_pay_link("case-law")).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{refused}");
    assert_eq!(rig.confirmed.try_recv(), Err(TryRecvError::Empty));
    // An unknown connector: refused.
    let unknown = rig.dispatch(open_pay_link("statutes")).unwrap_err();
    assert_eq!(unknown.code(), ErrorCode::CommandInvalid, "{unknown}");

    rig.ok(add_credits("200"));
    let state = rig.state_until("the pay link", |state| {
        case_law(state).is_some_and(|view| view.pay_url.is_some())
    });
    let pay_url = case_law(&state).unwrap().pay_url.clone().unwrap();
    rig.ok(open_pay_link("case-law"));
    let opened = rig.confirmed.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(opened, pay_url);
    assert!(
        opened.starts_with("https://data.clauseo.chat/pay/"),
        "{opened}"
    );
    // The unconfirmed opener (the sign-in page's) was never used.
    assert_eq!(rig.pages.try_recv(), Err(TryRecvError::Empty));

    // The link gone (paid): refused again.
    let recharge = rig.fake.recharges().pop().unwrap();
    assert!(rig.fake.mark_paid(&recharge));
    rig.state_until("paid", |state| {
        case_law(state).is_some_and(|view| view.pay_url.is_none())
    });
    let refused = rig.dispatch(open_pay_link("case-law")).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{refused}");
    rig.finish();
}

/// One `tools/call` of `tool` through a fresh relay session; its answer.
fn relay_call(rig: &Rig, tool: &str, arguments: &Value) -> Value {
    let (session, token) = rig.relay(1);
    let mut stream = TcpStream::connect(session.addr()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let hello = json!({"clauseo_relay": 1, "token": token, "connector": "case-law"});
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}});
    stream
        .write_all(format!("{hello}\n{call}\n").as_bytes())
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut lines = Vec::new();
    for _ in 0..2 {
        let mut text = String::new();
        reader.read_line(&mut text).unwrap();
        lines.push(serde_json::from_str::<Value>(&text).unwrap());
    }
    assert_eq!(lines[0]["ok"], true, "{}", lines[0]);
    lines.remove(1)
}

/// A URL that reached the host in relay output, a search's links or an out-of-credit result's
/// `payment.url` (shown in the state as `OutOfCredit`'s pay link), never becomes what
/// `OpenPayLink` opens: it is refused, and nothing opens.
#[test]
fn no_url_from_a_tool_result_is_opened() {
    let dir = Dir::new("connector_pay_link_results", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let found = relay_call(&rig, "search_law", &json!({"query": "basic structure"}));
    assert!(
        found.to_string().contains("https://indiankanoon.org/"),
        "{found}"
    );
    rig.fake.set_balance(0);
    let out = relay_call(&rig, "search_law", &json!({"query": "x"}));
    assert_eq!(out["result"]["isError"], true, "{out}");
    let state = rig.state_until("out of credit", |state| {
        matches!(
            status(state),
            Some(ConnectorStatus::OutOfCredit { pay_url: Some(_) })
        )
    });
    assert!(case_law(&state).unwrap().pay_url.is_none(), "{state:?}");
    let refused = rig.dispatch(open_pay_link("case-law")).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{refused}");
    assert_eq!(rig.confirmed.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(rig.pages.try_recv(), Err(TryRecvError::Empty));
    rig.finish();
}
