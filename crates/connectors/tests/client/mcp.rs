//! `mcp_post`: its headers, its three body kinds, and no `Authorization` without a credential.

use clauseo_connectors::{
    CASE_LAW, McpBody, McpRequestHeaders, ToolOutcome, classify_tool_result_for,
};
use serde_json::value::RawValue;
use serde_json::{Value, json};

use crate::support::*;

/// A raw answer, parsed.
fn value(raw: &RawValue) -> Value {
    serde_json::from_str(raw.get()).unwrap()
}

fn message(method: &str, id: Option<u64>) -> Vec<u8> {
    let mut value = json!({"jsonrpc": "2.0", "method": method, "params": {}});
    if let Some(id) = id {
        value["id"] = json!(id);
    }
    value.to_string().into_bytes()
}

fn call(name: &str) -> Vec<u8> {
    json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": {"name": name, "arguments": {"query": "Kesavananda"}}})
    .to_string()
    .into_bytes()
}

#[test]
fn mcp_post_sends_the_headers_and_returns_json() {
    let rig = rig();
    sign_in(&rig);
    let token = rig.handle.bearer().unwrap();
    let headers = McpRequestHeaders {
        protocol_version: Some("2025-06-18".to_owned()),
        session_id: Some("fake-session-0001".to_owned()),
    };
    let reply = rig.handle.mcp_post(&call("search_law"), &headers).unwrap();
    assert_eq!(reply.status, 200);
    let McpBody::Json(body) = &reply.body else {
        panic!("expected JSON, got {:?}", reply.body);
    };
    assert_eq!(value(body)["id"], 9);
    let request = &rig.fake.requests_to("/mcp/case-law")[0];
    let auth = format!("Bearer {}", token.0.expose());
    assert_eq!(request.header("authorization"), Some(auth.as_str()));
    assert_eq!(request.header("user-agent"), Some("Clauseo/0.0.0-test"));
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(
        request.header("accept"),
        Some("application/json, text/event-stream")
    );
    assert_eq!(request.header("mcp-protocol-version"), Some("2025-06-18"));
    assert_eq!(request.header("mcp-session-id"), Some("fake-session-0001"));
    assert_eq!(request.body, call("search_law"));
}

#[test]
fn optional_headers_are_sent_only_when_given() {
    let rig = rig();
    sign_in(&rig);
    rig.handle
        .mcp_post(
            &message("tools/list", Some(1)),
            &McpRequestHeaders::default(),
        )
        .unwrap();
    let request = &rig.fake.requests_to("/mcp/case-law")[0];
    assert!(request.header("mcp-protocol-version").is_none());
    assert!(request.header("mcp-session-id").is_none());
}

#[test]
fn without_a_credential_no_authorization_is_sent() {
    let rig = rig();
    let reply = rig
        .handle
        .mcp_post(
            &message("initialize", Some(1)),
            &McpRequestHeaders::default(),
        )
        .unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(reply.session_id.as_deref(), Some("fake-session-0001"));
    let request = &rig.fake.requests_to("/mcp/case-law")[0];
    assert!(request.header("authorization").is_none());
    assert_eq!(rig.store.reads(), 0);
}

#[test]
fn an_event_stream_is_one_value_per_data_event() {
    let rig = rig();
    sign_in(&rig);
    rig.fake.respond_as_sse(true);
    let reply = rig
        .handle
        .mcp_post(
            &message("tools/list", Some(4)),
            &McpRequestHeaders::default(),
        )
        .unwrap();
    let McpBody::Events(events) = reply.body else {
        panic!("expected events, got {:?}", reply.body);
    };
    assert_eq!(events.len(), 1);
    assert_eq!(value(&events[0])["id"], 4);
}

#[test]
fn a_json_answer_is_kept_raw_as_sent_and_bad_json_is_refused() {
    let rig = rig();
    let text =
        "  {\"jsonrpc\" : \"2.0\", \"id\":1,\n \"result\":{\"a\":[0,0]}, \"x\":1, \"x\":2}\n";
    let json_type = [("Content-Type", "application/json")];
    rig.fake
        .answer_next("/mcp/case-law", 200, &json_type, text.as_bytes());
    let reply = rig
        .handle
        .mcp_post(
            &message("tools/list", Some(1)),
            &McpRequestHeaders::default(),
        )
        .unwrap();
    let McpBody::Json(body) = reply.body else {
        panic!("expected JSON, got {:?}", reply.body);
    };
    // Not re-serialised: the spacing, the newline and the duplicate key are as sent.
    assert_eq!(body.get(), text.trim());
    for (status, body, err) in [
        (200, &b"{not json"[..], "parse"),
        (200, &b"{\"a\":\"\xff\"}"[..], "parse"),
        (500, &b"{not json"[..], "http"),
        (500, &b"\xff"[..], "http"),
    ] {
        rig.fake
            .answer_next("/mcp/case-law", status, &json_type, body);
        let got = rig.handle.mcp_post(
            &message("tools/list", Some(1)),
            &McpRequestHeaders::default(),
        );
        match (err, got) {
            ("parse", Err(clauseo_connectors::Error::Parse(_)))
            | ("http", Err(clauseo_connectors::Error::Http { status: 500 })) => {}
            (_, other) => panic!("{status} {body:?}: {other:?}"),
        }
    }
}

#[test]
fn a_notification_is_202_empty() {
    let rig = rig();
    let reply = rig
        .handle
        .mcp_post(
            &message("notifications/initialized", None),
            &McpRequestHeaders::default(),
        )
        .unwrap();
    assert_eq!(reply.status, 202);
    assert_eq!(reply.body, McpBody::Empty);
}

#[test]
fn an_out_of_credit_tool_result_classifies_with_our_pay_link() {
    let rig = rig();
    sign_in(&rig);
    rig.fake.set_balance(0);
    let reply = rig
        .handle
        .mcp_post(&call("search_law"), &McpRequestHeaders::default())
        .unwrap();
    let McpBody::Json(body) = reply.body else {
        panic!("expected JSON");
    };
    let body = value(&body);
    let origin = clauseo_connectors::Origin::Fake(rig.fake.base_url());
    let ToolOutcome::OutOfCredit(payment) = classify_tool_result_for(&CASE_LAW, &origin, &body)
    else {
        panic!("expected OutOfCredit: {body}");
    };
    let id = payment.recharge_id.unwrap();
    assert_eq!(
        payment.url.unwrap().as_str(),
        format!("https://data.clauseo.chat/pay/{id}")
    );
    rig.fake.set_balance(100);
    let reply = rig
        .handle
        .mcp_post(&call("search_law"), &McpRequestHeaders::default())
        .unwrap();
    let McpBody::Json(body) = reply.body else {
        panic!("expected JSON");
    };
    let result: &Value = &value(&body);
    assert_eq!(
        classify_tool_result_for(&CASE_LAW, &origin, result),
        ToolOutcome::Ok
    );
}
