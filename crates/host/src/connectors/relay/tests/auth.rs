//! Auth in the relay: one refresh and one retry after an expired access token, the signed-out
//! result after a revoke (and nothing sent on the next call), the key-revoked result for a
//! firm's API key, and the not-added case.

use clauseo_connectors::{CredentialStatus, PaymentRef};
use serde_json::json;

use super::super::super::{CredentialEnd, RelayEvent, texts};
use super::support::*;

#[test]
fn an_expired_access_token_is_refreshed_once_and_the_call_retried() {
    let rig = rig();
    rig.sign_in();
    let mut agent = rig.agent();
    rig.fake.expire_access_tokens();
    rig.fake.clear_requests();
    agent.send_value(&call(1, "search_law"));
    let reply = agent.recv();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let paths: Vec<String> = rig
        .fake
        .requests()
        .into_iter()
        .map(|request| request.path)
        .collect();
    assert_eq!(paths, ["/mcp/case-law", "/token", "/mcp/case-law"]);
    assert_eq!(
        rig.handle.status(),
        CredentialStatus::SignedIn(clauseo_connectors::CredentialKind::OAuth)
    );
}

#[test]
fn after_a_revoke_the_agent_gets_signed_out_and_nothing_more_is_sent() {
    let rig = rig();
    rig.sign_in();
    let mut agent = rig.agent();
    rig.fake.revoke_all();
    agent.send_value(&call(1, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::SIGNED_OUT);
    assert_eq!(rig.handle.status(), CredentialStatus::SignedOut);
    assert_eq!(
        rig.events.take(),
        vec![RelayEvent::CredentialEnded {
            session: rig.key,
            ended: CredentialEnd::SignedOut
        }]
    );
    rig.fake.clear_requests();
    agent.send_value(&call(2, "search_law"));
    let reply = agent.recv();
    assert_eq!(reply["id"], 2);
    assert_eq!(error_text(&reply), texts::SIGNED_OUT);
    assert!(rig.fake.requests().is_empty(), "nothing reached the fake");
}

#[test]
fn a_revoked_api_key_gives_key_revoked() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    rig.fake.revoke_all();
    agent.send_value(&call(1, "read_document"));
    assert_eq!(error_text(&agent.recv()), texts::KEY_REVOKED);
    assert_eq!(rig.handle.status(), CredentialStatus::KeyRevoked);
    assert_eq!(
        rig.events.take(),
        vec![RelayEvent::CredentialEnded {
            session: rig.key,
            ended: CredentialEnd::KeyRevoked
        }]
    );
}

#[test]
fn not_added_sends_no_call_but_lists_tools_without_authorization() {
    let rig = rig();
    let mut agent = rig.agent();
    agent.send_value(&call(1, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::SIGNED_OUT);
    assert_eq!(rig.mcp_requests().len(), 0);
    agent.send_value(&request(2, "tools/list"));
    assert_eq!(agent.recv()["id"], 2);
    assert_eq!(rig.mcp_requests()[0].header("authorization"), None);
    assert_eq!(
        rig.events.take(),
        vec![RelayEvent::CredentialEnded {
            session: rig.key,
            ended: CredentialEnd::NotAdded
        }]
    );
}

#[test]
fn out_of_credit_passes_the_result_on_and_reports_the_payment() {
    let rig = rig();
    rig.add_api_key();
    rig.fake.set_balance(0);
    let mut agent = rig.agent();
    agent.send_value(&call(1, "search_law"));
    let reply = agent.recv();
    assert_eq!(
        reply["result"]["structuredContent"]["error"]["code"],
        "insufficient_credits"
    );
    let events = rig.events.take();
    let [RelayEvent::OutOfCredit { session, payment }] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(*session, rig.key);
    let PaymentRef { recharge_id, url } = payment;
    let id = recharge_id.clone().unwrap();
    assert_eq!(
        url.as_ref().unwrap().as_str(),
        format!("https://data.clauseo.chat/pay/{id}")
    );
    // An error result of another kind tells nothing; `account` is free and says nothing.
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "account", "arguments": {}}}));
    agent.recv();
    assert_eq!(rig.events.take(), Vec::new());
}
