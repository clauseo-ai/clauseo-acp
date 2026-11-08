//! A tool result reaches the agent whole: every `content` block beside a `structuredContent`,
//! for `recharge` (a resumed call's blocks after the status) and for any tool, in JSON and in an
//! event stream. A `recharge` result that says paid reports its balance and drops the connectors
//! crate's account copy.

use serde_json::{Value, json};

use super::super::super::{RelayEvent, ResumedBalance};
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
            recharge_id: None,
            balance_credits: Some(2000),
            resumed_balance: ResumedBalance::Unknown,
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
            recharge_id: None,
            balance_credits: None,
            resumed_balance: ResumedBalance::NoCall,
        }]
    );
}

/// The connector's resumed shape: `recharge_id` at the top, and the balance after the resumed
/// call: `resumed.balance_credits` first, else the one the last charge line names, else
/// `Unknown` (a format not read is never a number); a `null` `resumed` resumed nothing. Five
/// cases per relay: a turn takes at most five `recharge` calls.
#[test]
fn a_resumed_paid_result_reports_the_balance_after_the_resumed_call() {
    use ResumedBalance::{Known, NoCall, Unknown};
    let line = "search_law: Charged 40 credits · balance 1,960";
    let resumed = |fields: Value| {
        let mut resumed = json!({"tool": "search_law", "in_content_blocks": true,
            "result": null, "text": "1. Daryao v. State of U.P."});
        for (key, value) in fields.as_object().unwrap() {
            resumed[key] = value.clone();
        }
        resumed
    };
    let cases = [
        (
            resumed(
                json!({"balance_credits": 1955, "charged_credits": 45, "charge_lines": [line]}),
            ),
            Known(1955),
        ),
        (resumed(json!({"charge_lines": [line]})), Known(1960)),
        (
            resumed(json!({"charge_lines": ["Charged 40 credits", "Balance: 12,345,678 credits"]})),
            Known(12_345_678),
        ),
        (
            resumed(json!({"charge_lines": ["balance 5", "Charged 40 credits"]})),
            Unknown,
        ),
        (
            resumed(json!({"charge_lines": ["search_law: 60 credits"]})),
            Unknown,
        ),
        (
            resumed(json!({"charge_lines": ["balance 1,96,0"]})),
            Unknown,
        ),
        (
            resumed(json!({"charge_lines": ["balance 1960.5 credits"]})),
            Unknown,
        ),
        (
            resumed(json!({"charge_lines": ["balance ₹196.00"]})),
            Unknown,
        ),
        // A number cut short is not read: a space, an apostrophe, a thin or no-break space, or a
        // dot before more digits.
        (
            resumed(json!({"charge_lines": ["balance 12 345"]})),
            Unknown,
        ),
        (resumed(json!({"charge_lines": ["balance 1'960"]})), Unknown),
        (resumed(json!({"charge_lines": ["balance 1’960"]})), Unknown),
        (
            resumed(json!({"charge_lines": ["balance 1\u{2009}960"]})),
            Unknown,
        ),
        (
            resumed(json!({"charge_lines": ["balance 1\u{a0}960 credits"]})),
            Unknown,
        ),
        (
            resumed(json!({"charge_lines": ["balance 1,960 5"]})),
            Unknown,
        ),
        (
            resumed(json!({"charge_lines": ["balance 1960_000"]})),
            Unknown,
        ),
        (
            resumed(json!({"charge_lines": ["balance 1,960'"]})),
            Unknown,
        ),
        // A word, punctuation or the end after it reads.
        (
            resumed(json!({"charge_lines": ["balance 1,960."]})),
            Known(1960),
        ),
        (
            resumed(json!({"charge_lines": ["Balance 1960 credits"]})),
            Known(1960),
        ),
        (
            resumed(json!({"charge_lines": ["balance 1,960 (₹196)"]})),
            Known(1960),
        ),
        (resumed(json!({"charge_lines": "balance 5"})), Unknown),
        (resumed(json!({"balance_credits": "lots"})), Unknown),
        (resumed(json!({})), Unknown),
        (json!(null), NoCall),
    ];
    for chunk in cases.chunks(5) {
        resumed_cases(chunk);
    }
}

/// Each `resumed` in a paid answer through a fresh relay, and the balance it reports.
fn resumed_cases(cases: &[(Value, ResumedBalance)]) {
    let rig = rig();
    rig.add_api_key();
    rig.events.take();
    let mut agent = rig.agent();
    let json_type = [("Content-Type", "application/json")];
    for (n, (resumed, resumed_balance)) in (1_u64..).zip(cases.iter().cloned()) {
        let status = json!({"recharge_id": "rch_1", "status": "paid", "balance_credits": 2000,
            "then": "Carry on.", "resumed": resumed});
        let reply = result(
            n,
            &status,
            &[json!({"type": "text", "text": status.to_string()})],
        );
        rig.fake
            .answer_next(MCP, 200, &json_type, reply.to_string().as_bytes());
        agent.send_value(&call(n, "recharge"));
        assert_eq!(agent.recv(), reply);
        assert_eq!(
            rig.events.take(),
            vec![RelayEvent::RechargePaid {
                session: rig.key,
                recharge_id: Some("rch_1".to_owned()),
                balance_credits: Some(2000),
                resumed_balance,
            }],
            "{resumed}"
        );
    }
}
