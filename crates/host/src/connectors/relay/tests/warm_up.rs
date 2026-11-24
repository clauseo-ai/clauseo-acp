//! The upstream `initialize` the warm-up owes the server: one shared attempt, its deadline, a
//! credential that ends during it, a session that ends while a call waits on it, and a server that
//! refuses it.

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{Value, json};

use super::super::super::{CredentialEnd, RelayEvent, texts};
use super::support::*;

/// How many times a message has waited for the upstream `initialize`.
fn waits(rig: &Rig) -> u64 {
    rig.session.shared().warm_waits.load(Ordering::SeqCst)
}

/// How many `initialize` requests reached the fake.
fn initializes(rig: &Rig) -> usize {
    rig.mcp_requests()
        .iter()
        .filter(|request| request.json().unwrap()["method"] == "initialize")
        .count()
}

/// The agent's `initialize`, answered.
fn initialized(rig: &Rig) -> Agent {
    let mut agent = rig.agent();
    agent.send_value(&initialize(1));
    assert_eq!(agent.recv()["id"], 1);
    agent
}

#[test]
fn calls_waiting_on_a_failed_attempt_share_its_failure_and_one_initialize_was_sent() {
    let rig = rig();
    rig.sign_in();
    let held = rig.fake.hold_next("/mcp/case-law");
    rig.fake.close_next("/mcp/case-law");
    let mut agent = initialized(&rig);
    for id in [3, 4, 5] {
        agent.send_value(&call(id, "search_law"));
    }
    wait_until("three calls waiting", || waits(&rig) >= 3);
    held.cancel();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let reply = agent.recv();
        assert_eq!(error_text(&reply), texts::NO_ANSWER);
        ids.push(reply["id"].as_u64().unwrap());
    }
    ids.sort_unstable();
    assert_eq!(ids, [3, 4, 5]);
    assert_eq!(initializes(&rig), 1);
    assert_eq!(rig.paid_today(), 0);
    // The next call starts a new attempt, which goes through, then the call.
    agent.send_value(&call(6, "search_law"));
    assert_eq!(agent.recv()["result"]["isError"], false);
    assert_eq!(initializes(&rig), 2);
    assert_eq!(rig.paid_today(), 1);
}

#[test]
fn an_initialize_past_the_deadline_answers_the_call_as_unreachable() {
    let rig = rig();
    rig.relays
        .set_warm_deadline_for_test(Duration::from_millis(300));
    rig.sign_in();
    let held = rig.fake.hold_next("/mcp/case-law");
    let mut agent = initialized(&rig);
    agent.send_value(&call(3, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::NO_ANSWER);
    assert_eq!(rig.paid_today(), 0);
    assert!(
        rig.mcp_requests()
            .iter()
            .all(|request| request.json().unwrap()["method"] != "tools/call")
    );
    held.cancel();
}

#[test]
fn a_credential_that_ends_during_the_warm_up_is_reported_at_once() {
    let rig = rig();
    rig.add_api_key();
    rig.fake.fail_next("/mcp/case-law", 401);
    let _agent = initialized(&rig);
    // No call yet: the warm-up's `initialize` met the 401, and the key is revoked.
    wait_until("the credential-ended event", || {
        !rig.events.events.lock().unwrap().is_empty()
    });
    assert_eq!(
        rig.events.take(),
        [RelayEvent::CredentialEnded {
            session: rig.key,
            ended: CredentialEnd::KeyRevoked,
        }]
    );
}

#[test]
fn a_session_that_ends_while_a_call_waits_counts_nothing() {
    let rig = rig();
    rig.sign_in();
    let held = rig.fake.hold_next("/mcp/case-law");
    let mut agent = initialized(&rig);
    agent.send_value(&call(3, "search_law"));
    wait_until("the call waiting", || waits(&rig) >= 1);
    assert_eq!(rig.paid_today(), 1);
    rig.session.end();
    held.cancel();
    wait_until("the refund", || rig.paid_today() == 0);
    // The warm-up's own requests may still run; no call went out.
    let calls = rig
        .mcp_requests()
        .iter()
        .filter(|request| request.json().unwrap()["method"] == "tools/call")
        .count();
    assert_eq!(calls, 0);
    drop(agent);
}

#[test]
fn a_server_that_refuses_initialize_gives_each_call_a_clear_error_and_a_lifecycle_line() {
    let captured = Captured::default();
    let rig = rig_with(&RigOptions {
        log: Some(captured.dispatch()),
        ..RigOptions::default()
    });
    rig.sign_in();
    let held = rig.fake.hold_next("/mcp/case-law");
    let refusal = json!({"jsonrpc": "2.0", "id": 1,
        "error": {"code": -32603, "message": "secret-refusal-text"}});
    rig.fake.answer_next(
        "/mcp/case-law",
        200,
        &[("Content-Type", "application/json")],
        refusal.to_string().as_bytes(),
    );
    let mut agent = initialized(&rig);
    agent.send_value(&call(3, "search_law"));
    wait_until("the call waiting", || waits(&rig) >= 1);
    held.cancel();
    let reply: Value = agent.recv();
    assert_eq!(error_text(&reply), texts::SESSION_REFUSED);
    assert_eq!(rig.paid_today(), 0);
    let text = captured.text();
    let line = text
        .lines()
        .find(|line| line.contains("the server refused initialize"))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(line.contains("clauseo_host::lifecycle"), "{line}");
    assert!(!text.contains("secret-refusal-text"), "{text}");
}
