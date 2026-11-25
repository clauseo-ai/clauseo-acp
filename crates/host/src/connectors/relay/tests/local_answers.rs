//! `initialize`, `tools/list` and `ping` are answered by the relay at once, with no network: with
//! the server held or the account signed out, the tools still list; the server gets its
//! `initialize` from the warm-up, before the first `tools/call`, which carries the credential.
//! And the relay's lifecycle lines.

use std::time::Duration;

use serde_json::{Value, json};

use super::super::super::{CredentialEnd, RelayEvent, texts};
use super::support::*;

/// [`ready_answers`], with what the shipped copy answers asserted.
fn ready(agent: &mut Agent) -> Duration {
    let (init, list, took) = ready_answers(agent);
    assert_eq!(init["result"]["serverInfo"]["name"], "clauseo-case-law");
    let shipped: Value = serde_json::from_str(clauseo_connectors::CASE_LAW_TOOLS_LIST).unwrap();
    assert_eq!(list["result"], shipped);
    assert!(names(&list).contains(&"search_law".to_owned()));
    took
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
    wait_until("the held initialize", || !rig.mcp_requests().is_empty());
    let initialize = &rig.mcp_requests()[0];
    assert_eq!(initialize.json().unwrap()["method"], "initialize");
    assert!(agent.open_and_quiet());
    held.cancel();
    assert_eq!(agent.recv()["result"]["isError"], false);
    // The warm-up's own `tools/list` runs beside the call; it is left out here.
    let requests: Vec<_> = rig
        .mcp_requests()
        .into_iter()
        .filter(|request| request.json().unwrap()["method"] != "tools/list")
        .collect();
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
fn ping_is_answered_here() {
    let rig = rig();
    let held = rig.fake.hold_next("/mcp/case-law");
    let mut agent = rig.agent();
    ready(&mut agent);
    agent.send_value(&request(3, "ping"));
    assert_eq!(
        agent.recv(),
        json!({"jsonrpc": "2.0", "id": 3, "result": {}})
    );
    held.cancel();
    assert!(
        rig.mcp_requests()
            .iter()
            .all(|request| request.json().unwrap()["method"] != "ping")
    );
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
fn the_version_is_negotiated_as_the_server_does() {
    let rig = rig();
    let mut agent = rig.agent();
    for (asked, answered) in [
        ("2030-01-01", "2025-11-25"),
        ("2024-11-05", "2025-11-25"),
        ("2026-07-28", "2025-11-25"),
        ("2025-11-25", "2025-11-25"),
        ("2025-06-18", "2025-06-18"),
        ("2025-03-26", "2025-03-26"),
    ] {
        agent.send_value(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": asked, "capabilities": {}}}));
        assert_eq!(
            agent.recv()["result"]["protocolVersion"],
            answered,
            "{asked}"
        );
    }
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
        // The query text, not a bare "138", which a timestamp or `ms=` can hold.
        assert!(!line.contains("section 138"), "{line}");
        assert!(!line.contains("NI Act"), "{line}");
        assert!(!line.contains('{'), "{line}");
    }
}
