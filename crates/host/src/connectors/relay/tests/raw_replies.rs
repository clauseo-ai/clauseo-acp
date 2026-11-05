//! Replies read raw: the answer's own bytes as the line (a raw newline made a space), a member
//! present as `null` still counting, a duplicate top-level key or an oversized id answering
//! nothing, and an error result's oversized `structuredContent` read as another error.

use serde_json::{Value, json};

use super::super::super::{RelayEvent, texts};
use super::support::*;

const JSON_TYPE: [(&str, &str); 1] = [("Content-Type", "application/json")];

fn answer_next(rig: &Rig, body: &str) {
    rig.fake
        .answer_next("/mcp/case-law", 200, &JSON_TYPE, body.as_bytes());
}

#[test]
fn the_answer_passes_as_sent_on_one_line() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    let sent = "{ \"jsonrpc\" : \"2.0\",\r\n  \"result\" : {\"isError\":false, \"content\":\
                [{\"type\":\"text\",\"text\":\"a\\nb\"}]},\n  \"id\" : 1 }";
    answer_next(&rig, sent);
    agent.send_value(&call(1, "search_law"));
    let line = agent.recv_line().unwrap();
    // Not re-serialised: the spacing and the key order are the connector's; the raw CR and LF
    // between tokens are spaces, the escaped one in the string is kept.
    assert_eq!(line, sent.replace(['\r', '\n'], " ").into_bytes());
    let reply: Value = serde_json::from_slice(&line).unwrap();
    assert_eq!(reply["result"]["content"][0]["text"], "a\nb");
    assert_eq!(
        rig.events.take(),
        vec![RelayEvent::PaidResult { session: rig.key }]
    );
    // An event stream's value of several `data:` lines comes back as one line too.
    let stream = "data: {\"jsonrpc\":\"2.0\",\ndata: \"id\":2,\"result\":{}}\n\n";
    rig.fake.answer_next(
        "/mcp/case-law",
        200,
        &[("Content-Type", "text/event-stream")],
        stream.as_bytes(),
    );
    agent.send_value(&request(2, "tools/list"));
    assert_eq!(
        agent.recv_line().unwrap(),
        br#"{"jsonrpc":"2.0", "id":2,"result":{}}"#.to_vec()
    );
}

#[test]
fn a_member_present_as_null_counts() {
    let rig = rig();
    let mut agent = rig.agent();
    answer_next(&rig, r#"{"jsonrpc":"2.0","id":1,"result":null}"#);
    agent.send_value(&request(1, "tools/list"));
    assert_eq!(
        agent.recv(),
        json!({"jsonrpc": "2.0", "id": 1, "result": null})
    );
    // A `method` present as `null` makes it no answer.
    answer_next(
        &rig,
        r#"{"jsonrpc":"2.0","id":2,"method":null,"result":{}}"#,
    );
    agent.send_value(&request(2, "tools/list"));
    assert_eq!(agent.recv()["error"]["message"], texts::NO_ANSWER);
}

#[test]
fn a_duplicate_top_level_key_or_an_oversized_id_answers_nothing() {
    let rig = rig();
    let mut agent = rig.agent();
    answer_next(&rig, r#"{"jsonrpc":"2.0","id":1,"id":1,"result":{}}"#);
    agent.send_value(&request(1, "tools/list"));
    assert_eq!(agent.recv()["error"]["message"], texts::NO_ANSWER);
    // A duplicate key deeper down is the connector's to send.
    answer_next(&rig, r#"{"jsonrpc":"2.0","id":2,"result":{"a":1,"a":2}}"#);
    agent.send_value(&request(2, "tools/list"));
    assert_eq!(
        agent.recv_line().unwrap(),
        br#"{"jsonrpc":"2.0","id":2,"result":{"a":1,"a":2}}"#.to_vec()
    );
    // An id over 1 KB is never compared, so it answers nothing even when it matches.
    let long = "i".repeat(1100);
    answer_next(
        &rig,
        &json!({"jsonrpc": "2.0", "id": long, "result": {}}).to_string(),
    );
    agent.send_value(&json!({"jsonrpc": "2.0", "id": long, "method": "tools/list"}));
    assert_eq!(agent.recv()["error"]["message"], texts::NO_ANSWER);
}

#[test]
fn an_oversized_structured_content_is_another_error() {
    let captured = Captured::default();
    let rig = rig_with(&RigOptions {
        log: Some(captured.dispatch()),
        ..RigOptions::default()
    });
    rig.add_api_key();
    let mut agent = rig.agent();
    // Out of credit, but with 70 KB of padding beside the code: too large to read.
    let padding = "p".repeat(70 * 1024);
    let result = json!({"isError": true, "content": [],
        "structuredContent": {"error": {"code": "insufficient_credits"}, "padding": padding}});
    answer_next(
        &rig,
        &json!({"jsonrpc": "2.0", "id": 1, "result": result}).to_string(),
    );
    agent.send_value(&call(1, "search_law"));
    assert_eq!(agent.recv()["result"]["isError"], true);
    assert_eq!(rig.events.take(), Vec::new());
    let log = captured.text();
    assert!(log.contains("too large to read"), "{log}");
}
