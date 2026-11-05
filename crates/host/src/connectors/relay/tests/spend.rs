//! The spend path on the parsed value: the method exactly `tools/call`, unknown tools paid, a
//! `tools/call` with no usable id refused unforwarded, a non-string `params.name` refused, and
//! the connector's `protocolVersion` and `Mcp-Session-Id` checked before they become headers.

use serde_json::{Value, json};

use super::super::super::texts;
use super::support::*;

fn paid_today(rig: &Rig) -> Option<String> {
    std::fs::read_to_string(rig.day_file()).ok()
}

#[test]
fn a_variant_of_tools_call_is_refused_unforwarded_and_only_the_exact_method_counts() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    let variants = [
        "Tools/call",
        "TOOLS/CALL",
        "tools/call ",
        " tools/call",
        r"tools/call\t",
        r"tools/call\n",
        r"tools/call\u0000",
        r"\u00a0tools/call",
        "ｔｏｏｌｓ/call",
    ];
    for (id, method) in variants.iter().enumerate() {
        let line = format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"name":"search_law"}}}}"#
        );
        agent.send(&line);
        assert_eq!(
            agent.recv(),
            json!({"jsonrpc": "2.0", "id": null,
                "error": {"code": -32600, "message": texts::INVALID_REQUEST}}),
            "{method:?}"
        );
    }
    assert_eq!(rig.mcp_requests().len(), 0);
    assert_eq!(paid_today(&rig), None);
    // Names that only contain it are other methods, forwarded as such.
    agent.send_value(&request(50, "tools/callx"));
    assert_eq!(agent.recv()["error"]["code"], -32601);
    assert_eq!(paid_today(&rig), None);
    // The exact method still goes through and counts.
    agent.send_value(&call(51, "search_law"));
    assert_eq!(agent.recv()["result"]["isError"], false);
    assert_eq!(
        paid_today(&rig).as_deref(),
        Some(r#"{"day":"2026-11-02","paid":1}"#)
    );
}

#[test]
fn unknown_tools_and_other_spellings_count_as_paid() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    agent.send_value(&call(1, "made_up_tool"));
    agent.recv();
    agent.send_value(&call(2, "Account"));
    agent.recv();
    agent.send_value(&call(3, "account"));
    agent.recv();
    assert_eq!(
        paid_today(&rig).as_deref(),
        Some(r#"{"day":"2026-11-02","paid":2}"#)
    );
}

#[test]
fn a_tools_call_with_no_usable_id_is_refused_unforwarded() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    // A notification: nothing can answer it, and nothing is sent.
    agent.send_value(&json!({"jsonrpc": "2.0", "method": "tools/call",
        "params": {"name": "search_law", "arguments": {}}}));
    for (n, id) in [json!(null), json!({"a": 1}), json!([1]), json!(true)]
        .iter()
        .enumerate()
    {
        agent.send_value(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "search_law", "arguments": {}}}));
        assert_eq!(
            agent.recv(),
            json!({"jsonrpc": "2.0", "id": null,
                "error": {"code": -32600, "message": texts::INVALID_REQUEST}}),
            "case {n}"
        );
    }
    agent.send_value(&request(9, "ping"));
    assert_eq!(agent.recv()["id"], 9);
    let sent: Vec<Value> = rig
        .mcp_requests()
        .iter()
        .map(|request| request.json().unwrap()["method"].clone())
        .collect();
    assert_eq!(sent, [json!("ping")]);
    assert_eq!(paid_today(&rig), None);
}

#[test]
fn a_tool_name_that_is_not_a_string_is_refused() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    let bad_params = [
        json!({"name": 5}),
        json!({"name": ["search_law"]}),
        json!({"name": null}),
        json!({}),
        json!("search_law"),
    ];
    for (id, params) in bad_params.iter().enumerate() {
        agent.send_value(
            &json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params}),
        );
        assert_eq!(
            agent.recv(),
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": texts::BAD_TOOL_NAME}})
        );
    }
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call"}));
    assert_eq!(agent.recv()["error"]["code"], -32602);
    assert_eq!(rig.mcp_requests().len(), 0);
    assert_eq!(paid_today(&rig), None);
}

/// Answers the next MCP request with an `initialize` result naming `version`, and
/// `Mcp-Session-Id: session`.
fn initialize_answer(rig: &Rig, version: &str, session: &str) {
    let body = json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": version,
        "capabilities": {}, "serverInfo": {"name": "x", "version": "1"}}});
    rig.fake.answer_next(
        "/mcp/case-law",
        200,
        &[
            ("Content-Type", "application/json"),
            ("Mcp-Session-Id", session),
        ],
        body.to_string().as_bytes(),
    );
}

#[test]
fn a_protocol_version_or_session_id_unfit_for_a_header_is_not_sent_back() {
    let rig = rig();
    let mut agent = rig.agent();
    let long = "s".repeat(257);
    for (version, session) in [("2025-06-18 x", "a b"), ("2025/06/18", long.as_str())] {
        initialize_answer(&rig, version, session);
        agent.send_value(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
        agent.recv();
        agent.send_value(&request(2, "tools/list"));
        agent.recv();
        let list = rig.mcp_requests().pop().unwrap();
        assert_eq!(list.header("mcp-protocol-version"), None, "{version}");
        assert_eq!(list.header("mcp-session-id"), None, "{session}");
    }
    // Fit ones are sent back as they came.
    let fit = "s".repeat(256);
    initialize_answer(&rig, "2026-07-28", &fit);
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
    agent.recv();
    agent.send_value(&request(2, "tools/list"));
    agent.recv();
    let list = rig.mcp_requests().pop().unwrap();
    assert_eq!(list.header("mcp-protocol-version"), Some("2026-07-28"));
    assert_eq!(list.header("mcp-session-id"), Some(fit.as_str()));
}

#[test]
fn a_call_that_cannot_go_out_counts_nothing() {
    // Not added: no credential at all.
    let rig = rig();
    let mut agent = rig.agent();
    for id in 1..=3 {
        agent.send_value(&call(id, "search_law"));
        assert_eq!(error_text(&agent.recv()), texts::SIGNED_OUT);
    }
    assert_eq!(rig.paid_today(), 0);
    // Signed out: the first call meets the revoke at the connector, the rest go nowhere.
    let rig = rig_with(&RigOptions::default());
    rig.sign_in();
    rig.fake.revoke_all();
    let mut agent = rig.agent();
    for id in 1..=3 {
        agent.send_value(&call(id, "search_law"));
        assert_eq!(error_text(&agent.recv()), texts::SIGNED_OUT);
    }
    assert_eq!(rig.paid_today(), 0);
}

#[test]
fn a_call_read_while_the_session_ends_counts_nothing() {
    let rig = rig();
    rig.add_api_key();
    rig.session.end();
    let (ours, theirs) = socket_pair();
    let conn =
        super::super::conn::Conn::for_test(std::sync::Arc::clone(rig.session.shared()), ours);
    super::super::forward::message(&conn, call(1, "search_law").to_string().as_bytes());
    drop(theirs);
    assert_eq!(rig.paid_today(), 0);
    assert_eq!(rig.mcp_requests().len(), 0);
}
