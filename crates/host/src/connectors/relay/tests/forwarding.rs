//! Forwarding through the relay to the fake: the headers, `capabilities: {}`, the re-serialised
//! body, server-to-client requests dropped, batches, parse errors and notifications, event
//! streams as lines, and answers in completion order.

use serde_json::{Value, json};

use super::super::super::{RelayEvent, texts};
use super::support::*;

#[test]
fn initialize_list_and_call_carry_the_headers() {
    let rig = rig();
    rig.sign_in();
    let mut agent = rig.agent();
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {"elicitation": {}, "roots": {}},
            "clientInfo": {"name": "agent", "version": "1"}}}));
    assert_eq!(agent.recv()["result"]["protocolVersion"], "2025-06-18");
    agent.send_value(&request(2, "tools/list"));
    assert_eq!(agent.recv()["id"], 2);
    agent.send_value(&call(3, "search_law"));
    assert_eq!(agent.recv()["result"]["isError"], false);
    let bearer = format!("Bearer {}", rig.handle.bearer().unwrap().0.expose());
    let requests = rig.mcp_requests();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        assert_eq!(request.header("authorization"), Some(bearer.as_str()));
        assert_eq!(request.header("user-agent"), Some("Clauseo/0.0.0-test"));
    }
    let initialize = &requests[0];
    assert_eq!(initialize.header("mcp-protocol-version"), None);
    assert_eq!(initialize.header("mcp-session-id"), None);
    assert_eq!(
        initialize.json().unwrap()["params"]["capabilities"],
        json!({})
    );
    assert_eq!(
        initialize.json().unwrap()["params"]["clientInfo"]["name"],
        "agent"
    );
    for request in &requests[1..] {
        assert_eq!(request.header("mcp-protocol-version"), Some("2025-06-18"));
        assert_eq!(request.header("mcp-session-id"), Some("fake-session-0001"));
    }
    // One overhead figure per forwarded message.
    rig.session.shared().first_flight().unwrap().wait_idle();
    assert_eq!(rig.events.overheads.lock().unwrap().len(), 3);
}

#[test]
fn server_discover_carries_empty_capabilities_whatever_was_sent() {
    let rig = rig();
    let mut agent = rig.agent();
    agent.send_value(
        &json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover",
        "params": {"capabilities": {"sampling": {}}}}),
    );
    assert_eq!(agent.recv()["id"], 1);
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 2, "method": "server/discover"}));
    assert_eq!(agent.recv()["id"], 2);
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 3, "method": "initialize", "params": [1]}));
    assert_eq!(agent.recv()["error"]["code"], -32602);
    let requests = rig.mcp_requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.json().unwrap()["params"]["capabilities"], json!({}));
    }
    // No credential: nothing to send but the message.
    assert_eq!(requests[0].header("authorization"), None);
}

#[test]
fn the_fake_gets_the_re_serialised_value_and_the_count_follows_it() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    // Extra whitespace and a duplicate key: the parsed value has the last `method`.
    let line = r#"{ "jsonrpc" : "2.0",  "id" : 1, "method" : "tools/list", "method":"tools/call",
        "params" : { "name" : "search_law", "arguments" : { "query" : "cheque  bounce" } } }"#
        .replace('\n', " ");
    agent.send(&line);
    assert_eq!(agent.recv()["result"]["isError"], false);
    let sent = &rig.mcp_requests()[0];
    let parsed: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(sent.body, serde_json::to_vec(&parsed).unwrap());
    assert_eq!(parsed["method"], "tools/call");
    assert!(!sent.body.contains(&b'\n'));
    assert_eq!(
        std::fs::read_to_string(rig.day_file()).unwrap(),
        r#"{"day":"2026-11-02","paid":1}"#
    );
    assert_eq!(
        rig.events.take(),
        vec![RelayEvent::PaidResult { session: rig.key }]
    );
}

#[test]
fn a_server_to_client_request_is_dropped_and_logged() {
    let captured = Captured::default();
    let rig = rig_with(&RigOptions {
        log: Some(captured.dispatch()),
        ..RigOptions::default()
    });
    let mut agent = rig.agent();
    let json_type = [("Content-Type", "application/json")];
    let ask = json!({"jsonrpc": "2.0", "id": 77, "method": "elicitation/create", "params": {"message": "secret-ask"}});
    rig.fake
        .answer_next("/mcp/case-law", 200, &json_type, ask.to_string().as_bytes());
    agent.send_value(&request(1, "tools/list"));
    // The request still gets its one answer.
    assert_eq!(
        agent.recv(),
        json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32603, "message": texts::NO_ANSWER}})
    );
    // In an event stream, the request is dropped and the answer passes.
    let answer = json!({"jsonrpc": "2.0", "id": 2, "result": {"tools": []}});
    let stream = format!("event: message\ndata: {ask}\n\nevent: message\ndata: {answer}\n\n");
    let sse_type = [("Content-Type", "text/event-stream")];
    rig.fake
        .answer_next("/mcp/case-law", 200, &sse_type, stream.as_bytes());
    agent.send_value(&request(2, "tools/list"));
    assert_eq!(agent.recv(), answer);
    let log = captured.text();
    assert!(log.contains("elicitation/create"), "{log}");
    assert!(!log.contains("secret-ask"), "{log}");
}

#[test]
fn batches_parse_errors_and_notifications() {
    let rig = rig();
    let mut agent = rig.agent();
    agent.send("[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}]");
    assert_eq!(
        agent.recv(),
        json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": texts::NO_BATCHES}})
    );
    agent.send("{not json");
    assert_eq!(
        agent.recv(),
        json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}})
    );
    agent.send("   ");
    agent.send_value(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 5, "result": {}}));
    agent.send_value(&request(6, "ping"));
    // The notification and the agent's own response got nothing back: the next line is the
    // ping's.
    assert_eq!(
        agent.recv(),
        json!({"jsonrpc": "2.0", "id": 6, "result": {}})
    );
    // Both were forwarded (the fake answers 202), the batch and the bad line were not.
    rig.session.shared().first_flight().unwrap().wait_idle();
    let methods: Vec<Value> = rig
        .mcp_requests()
        .iter()
        .map(|request| request.json().unwrap()["method"].clone())
        .collect();
    assert_eq!(methods.len(), 3);
    assert!(methods.contains(&json!("notifications/initialized")));
}

#[test]
fn each_event_of_a_stream_comes_back_as_one_line() {
    let rig = rig();
    rig.fake.respond_as_sse(true);
    let mut agent = rig.agent();
    agent.send_value(&request(1, "tools/list"));
    let line = agent.recv_line().unwrap();
    let reply: Value = serde_json::from_slice(&line).unwrap();
    assert_eq!(reply["id"], 1);
    assert!(reply["result"]["tools"].is_array());
    assert_eq!(
        line,
        serde_json::to_vec(&reply).unwrap(),
        "compact, one line"
    );
}

#[test]
fn slow_calls_come_back_in_completion_order_each_line_whole() {
    let gated = Gated::start();
    let rig = rig_with(&RigOptions {
        upstream: Some(gated.origin.clone()),
        ..RigOptions::default()
    });
    let mut agent = rig.agent();
    for id in 1..=3 {
        agent.send_value(&request(id, "tools/list"));
    }
    gated.wait_arrived(3);
    for id in [3, 1, 2] {
        gated.release(id);
        assert_eq!(
            agent.recv(),
            json!({"jsonrpc": "2.0", "id": id, "result": {"n": id}})
        );
    }
}

#[test]
fn the_agent_closing_its_side_still_gets_the_answers_in_flight() {
    let gated = Gated::start();
    let rig = rig_with(&RigOptions {
        upstream: Some(gated.origin.clone()),
        ..RigOptions::default()
    });
    let mut agent = rig.agent();
    agent.send_value(&request(1, "tools/list"));
    gated.wait_arrived(1);
    agent.shut_write();
    gated.release(1);
    assert_eq!(agent.recv()["id"], 1);
    assert!(agent.closed());
}

#[test]
fn only_the_answer_to_the_request_is_passed_on() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    let json_type = [("Content-Type", "application/json")];
    let foreign = json!({"jsonrpc": "2.0", "id": 99, "result": {"isError": false, "content": []}});
    rig.fake.answer_next(
        "/mcp/case-law",
        200,
        &json_type,
        foreign.to_string().as_bytes(),
    );
    agent.send_value(&call(1, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::NO_ANSWER);
    // The foreign answer was not read as this call's result.
    assert_eq!(rig.events.take(), Vec::new());
    // In a stream, only the matching answer passes; the rest is dropped.
    let note = json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {}});
    let answer = json!({"jsonrpc": "2.0", "id": 2, "result": {"tools": []}});
    let stream = format!("data: {foreign}\n\ndata: {note}\n\ndata: {answer}\n\ndata: {answer}\n\n");
    let sse_type = [("Content-Type", "text/event-stream")];
    rig.fake
        .answer_next("/mcp/case-law", 200, &sse_type, stream.as_bytes());
    agent.send_value(&request(2, "tools/list"));
    assert_eq!(agent.recv(), answer);
    agent.send_value(&request(3, "ping"));
    assert_eq!(agent.recv()["id"], 3, "one line for request 2, no more");
}
