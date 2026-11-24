//! `initialize` and `tools/list` are answered by the relay at once, with no network: with the
//! server held, unreachable or the account signed out, the tools still list; the server gets its
//! `initialize` from the warm-up, before the first `tools/call`, which carries the credential.
//! And the relay's lifecycle lines.

use std::time::Duration;

use serde_json::{Value, json};

use super::super::super::{CredentialEnd, RelayEvent, texts};
use super::support::*;
use crate::clock::{Clock as _, SystemClock};

/// How long `initialize` and `tools/list` may take together, the server held or not.
const READY_WITHIN: Duration = Duration::from_millis(500);

/// The agent's `initialize`.
fn initialize(id: u64) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {"elicitation": {}},
            "clientInfo": {"name": "agent", "version": "1"}}})
}

/// The tools' names in a `tools/list` answer.
fn names(reply: &Value) -> Vec<String> {
    reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect()
}

/// The agent's `initialize`, `notifications/initialized` and `tools/list`, with the time from
/// the first line sent to the `tools/list` answer on the real clock; asserts what they answer.
fn ready(agent: &mut Agent) -> Duration {
    let start = SystemClock.now().mono;
    agent.send_value(&initialize(1));
    let init = agent.recv();
    agent.send_value(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    agent.send_value(&request(2, "tools/list"));
    let list = agent.recv();
    let took = SystemClock.now().mono.saturating_sub(start);
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "clauseo-case-law");
    assert_eq!(
        init["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
    assert_eq!(list["id"], 2);
    let shipped: Value = serde_json::from_str(clauseo_connectors::CASE_LAW_TOOLS_LIST).unwrap();
    assert_eq!(list["result"], shipped);
    assert!(names(&list).contains(&"search_law".to_owned()));
    eprintln!(
        "initialize and tools/list answered in {} ms",
        took.as_millis()
    );
    assert!(took < READY_WITHIN, "ready took {took:?}");
    took
}

/// The MCP requests the fake has, once it has at least `count` (10 s at most).
fn wait_requests(rig: &Rig, count: usize) -> Vec<clauseo_connectors::fake::RecordedRequest> {
    for _ in 0..1000 {
        let requests = rig.mcp_requests();
        if requests.len() >= count {
            return requests;
        }
        let (_hold, wake) = std::sync::mpsc::channel::<()>();
        if wake.recv_timeout(Duration::from_millis(10)).is_err() {
            // The time is up.
        }
    }
    panic!("the fake never had {count} MCP requests");
}

#[test]
fn the_tools_list_at_once_while_the_server_is_held_and_the_call_carries_the_credential() {
    let rig = rig();
    rig.sign_in();
    let held = rig.fake.hold_next("/mcp/case-law");
    let mut agent = rig.agent();
    ready(&mut agent);
    agent.send_value(&call(3, "search_law"));
    // The call waits for the server's `initialize`, still held.
    let initialize = &wait_requests(&rig, 1)[0];
    assert_eq!(initialize.json().unwrap()["method"], "initialize");
    assert!(agent.open_and_quiet());
    held.cancel();
    assert_eq!(agent.recv()["result"]["isError"], false);
    let requests = rig.mcp_requests();
    let methods: Vec<Value> = requests
        .iter()
        .map(|request| request.json().unwrap()["method"].clone())
        .collect();
    assert_eq!(
        methods,
        [
            json!("initialize"),
            json!("notifications/initialized"),
            json!("tools/call")
        ]
    );
    let sent = requests[0].json().unwrap();
    assert_eq!(sent["params"]["capabilities"], json!({}));
    assert_eq!(sent["params"]["protocolVersion"], "2025-06-18");
    assert_eq!(sent["params"]["clientInfo"]["name"], "agent");
    let bearer = format!("Bearer {}", rig.handle.bearer().unwrap().0.expose());
    let tools_call = &requests[2];
    assert_eq!(tools_call.header("authorization"), Some(bearer.as_str()));
    assert_eq!(
        tools_call.header("mcp-protocol-version"),
        Some("2025-06-18")
    );
    assert_eq!(
        tools_call.header("mcp-session-id"),
        Some("fake-session-0001")
    );
    assert_eq!(rig.paid_today(), 1);
}

#[test]
fn with_the_server_unreachable_the_tools_list_and_the_call_is_refused_then_retried() {
    let rig = rig();
    rig.sign_in();
    // The warm-up's `initialize` and the first call's retry of it both fail.
    rig.fake.close_next("/mcp/case-law");
    rig.fake.close_next("/mcp/case-law");
    let mut agent = rig.agent();
    ready(&mut agent);
    agent.send_value(&call(3, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::NO_ANSWER);
    // Nothing went out for it: refunded.
    assert_eq!(rig.paid_today(), 0);
    // The server is back: the next call sends the `initialize` first, then goes through.
    agent.send_value(&call(4, "search_law"));
    assert_eq!(agent.recv()["result"]["isError"], false);
    let methods: Vec<Value> = rig.mcp_requests()[2..]
        .iter()
        .map(|request| request.json().unwrap()["method"].clone())
        .collect();
    assert_eq!(
        methods,
        [
            json!("initialize"),
            json!("notifications/initialized"),
            json!("tools/call")
        ]
    );
    assert_eq!(rig.paid_today(), 1);
}

#[test]
fn signed_out_the_tools_list_and_the_call_gets_the_signed_out_answer() {
    let rig = rig();
    let mut agent = rig.agent();
    ready(&mut agent);
    agent.send_value(&call(3, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::SIGNED_OUT);
    assert_eq!(
        rig.events.take(),
        [RelayEvent::CredentialEnded {
            session: rig.key,
            ended: CredentialEnd::NotAdded,
        }]
    );
    // The server only had its `initialize` (with no credential), never a call.
    rig.session.shared().first_flight().unwrap().wait_idle();
    assert!(
        rig.mcp_requests()
            .iter()
            .all(|request| request.json().unwrap()["method"] != "tools/call")
    );
}

#[test]
fn an_unknown_protocol_version_gets_the_newest_the_relay_speaks() {
    let rig = rig();
    let mut agent = rig.agent();
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2030-01-01", "capabilities": {}}}));
    assert_eq!(agent.recv()["result"]["protocolVersion"], "2025-06-18");
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 2, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26", "capabilities": {}}}));
    assert_eq!(agent.recv()["result"]["protocolVersion"], "2025-03-26");
}

#[test]
fn the_lifecycle_lines_name_the_session_and_hold_no_token() {
    let captured = Captured::default();
    let rig = rig_with(&RigOptions {
        log: Some(captured.dispatch()),
        ..RigOptions::default()
    });
    rig.sign_in();
    let mut agent = rig.agent();
    ready(&mut agent);
    agent.send_value(&call(3, "search_law"));
    assert_eq!(agent.recv()["result"]["isError"], false);
    agent.send_value(&call(4, "search_law"));
    assert_eq!(agent.recv()["result"]["isError"], false);
    let text = captured.text();
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("clauseo_host::lifecycle"))
        .collect();
    let session = format!("session={}", rig.key.id());
    for (index, expected) in [
        "case-law relay opened",
        "case-law relay: initialize answered",
        "case-law relay: tools/list answered",
        "case-law relay: first tools/call",
    ]
    .iter()
    .enumerate()
    {
        let line = lines.get(index).copied().unwrap_or_default();
        assert!(line.contains(expected), "{index}: {lines:?}");
        assert!(line.contains(&session), "{line}");
        if index > 0 {
            assert!(line.contains("ms="), "{line}");
        }
    }
    assert!(
        lines[3].contains("outcome=\"ok\"") || lines[3].contains("outcome=ok"),
        "{}",
        lines[3]
    );
    // One set per session: the second call writes no line.
    assert_eq!(lines.len(), 4, "{lines:?}");
    let bearer = rig.handle.bearer().unwrap().0.expose().to_owned();
    for line in &lines {
        assert!(!line.contains(rig.token.expose()), "{line}");
        assert!(!line.contains(&bearer), "{line}");
        assert!(!line.contains("138"), "{line}");
        assert!(!line.contains('{'), "{line}");
    }
}
