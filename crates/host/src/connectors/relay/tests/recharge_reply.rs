//! A tool result reaches the agent whole: every `content` block beside a `structuredContent`,
//! for `recharge` (a resumed call's blocks after the status) and for any tool, in JSON and in an
//! event stream. A `recharge` result that says paid reports its balance and drops the connectors
//! crate's account copy.

use serde_json::{Value, json};

use super::super::super::RelayEvent;
use super::support::*;

/// A tool's answer for a request id.
type Reply = fn(u64) -> Value;

/// The connector's path.
const MCP: &str = "/mcp/case-law";

/// A `tools/call` result with `structuredContent` and these `content` blocks.
fn result(id: u64, structured: &Value, blocks: &[Value]) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": {
        "content": blocks, "structuredContent": structured, "isError": false}})
}

/// Today's paid `recharge` with resume and wait: the status as JSON text, then the resumed
/// `search_law`'s own blocks (`in_content_blocks`).
fn recharge_today(id: u64) -> Value {
    let status = json!({"status": "paid", "balance_credits": 2000, "balance_inr": "200.00",
        "message_for_human": "Payment of ₹236 received.\nCarrying on with your request.",
        "repeat_call": false, "resumed": {"tool": "search_law", "in_content_blocks": true}});
    result(
        id,
        &status,
        &[
            json!({"type": "text", "text": status.to_string()}),
            json!({"type": "text", "text": "2 queries, 18 unique hits\n1. Daryao v. State of U.P."}),
            json!({"type": "text", "text": "2. State of U.P. v. Nawab Hussain"}),
            json!({"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"}),
        ],
    )
}

/// The connector's live shape since 2026-11-06: the resumed result in
/// `structuredContent.resumed` too (`result` is null for `search_law`; `text` carries it).
fn recharge_coming(id: u64) -> Value {
    let status = json!({"recharge_id": "rch_1", "status": "paid", "balance_credits": 1940,
        "resumed": {"tool": "search_law", "in_content_blocks": true, "result": null,
            "text": "1. Daryao v. State of U.P.", "charge_lines": ["search_law: 60 credits"]}});
    result(
        id,
        &status,
        &[
            json!({"type": "text", "text": status.to_string()}),
            json!({"type": "text", "text": "1. Daryao v. State of U.P."}),
        ],
    )
}

/// Any tool with an `outputSchema`: its structured result, and text blocks beside it.
fn search_with_schema(id: u64) -> Value {
    result(
        id,
        &json!({"results": [{"title": "Daryao", "url": "https://indiankanoon.org/doc/1/"}]}),
        &[
            json!({"type": "text", "text": "{\"results\":[{\"title\":\"Daryao\"}]}"}),
            json!({"type": "text", "text": "Daryao v. State of U.P., SC 1961"}),
        ],
    )
}

#[test]
fn every_content_block_beside_structured_content_reaches_the_agent_unchanged() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    let replies: [(&str, Reply); 3] = [
        ("recharge", recharge_today),
        ("recharge", recharge_coming),
        ("search_law", search_with_schema),
    ];
    let mut id = 0;
    for (tool, reply) in replies {
        for sse in [false, true] {
            id += 1;
            let reply = reply(id);
            let text = reply.to_string();
            let (kind, body) = if sse {
                (
                    "text/event-stream",
                    format!("event: message\ndata: {text}\n\n"),
                )
            } else {
                ("application/json", text.clone())
            };
            rig.fake
                .answer_next(MCP, 200, &[("Content-Type", kind)], body.as_bytes());
            agent.send_value(&call(id, tool));
            let line = agent.recv_line().expect("an answer");
            assert_eq!(String::from_utf8(line).unwrap(), text, "{tool}, sse {sse}");
        }
    }
}

#[test]
fn a_paid_recharge_result_reports_its_balance_and_drops_the_account_copy() {
    let rig = rig();
    rig.add_api_key();
    let reads = || rig.fake.requests_to("/v1/case-law/account").len();
    rig.handle.account().unwrap();
    let before = reads();
    // From memory: the copy is younger than a minute.
    rig.handle.account().unwrap();
    assert_eq!(reads(), before);
    rig.events.take();
    let mut agent = rig.agent();
    let json_type = [("Content-Type", "application/json")];
    // Not paid, or an error: nothing.
    let waiting = result(1, &json!({"status": "awaiting_payment"}), &[]);
    let failed = json!({"jsonrpc": "2.0", "id": 2, "result": {"content": [], "isError": true,
        "structuredContent": {"status": "paid", "balance_credits": 5}}});
    for (id, reply) in [(1, waiting), (2, failed)] {
        rig.fake
            .answer_next(MCP, 200, &json_type, reply.to_string().as_bytes());
        agent.send_value(&call(id, "recharge"));
        assert_eq!(agent.recv()["id"], id);
    }
    rig.handle.account().unwrap();
    assert_eq!(reads(), before);
    assert_eq!(rig.events.take(), Vec::new());
    // Paid: the balance it carried, and the next account read asks the connector.
    let paid = recharge_today(3);
    rig.fake
        .answer_next(MCP, 200, &json_type, paid.to_string().as_bytes());
    agent.send_value(&call(3, "recharge"));
    assert_eq!(agent.recv(), paid);
    assert_eq!(
        rig.events.take(),
        vec![RelayEvent::RechargePaid {
            session: rig.key,
            balance_credits: Some(2000),
        }]
    );
    rig.handle.account().unwrap();
    assert_eq!(reads(), before + 1);
    // A balance that is not a whole number is left out; paid still counts.
    let odd = result(
        4,
        &json!({"status": "paid", "balance_credits": "lots"}),
        &[],
    );
    rig.fake
        .answer_next(MCP, 200, &json_type, odd.to_string().as_bytes());
    agent.send_value(&call(4, "recharge"));
    assert_eq!(agent.recv()["id"], 4);
    assert_eq!(
        rig.events.take(),
        vec![RelayEvent::RechargePaid {
            session: rig.key,
            balance_credits: None,
        }]
    );
}
