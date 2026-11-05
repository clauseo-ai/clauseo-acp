//! The input limits: 4 messages in flight per connection, 4 MB per line from the agent, and the
//! relay's own 4 MB check on a connector reply (S28 reads up to 8 MB).

use clauseo_connectors::fake::Fixture;
use serde_json::{Value, json};

use super::super::super::texts;
use super::super::super::{
    MAX_IN_FLIGHT, MAX_IN_FLIGHT_BYTES_PER_SESSION, MAX_LINE_BYTES, MAX_REPLY_BYTES,
};
use super::support::*;

#[test]
fn with_4_in_flight_no_further_line_is_read_until_one_finishes() {
    let gated = Gated::start();
    let rig = rig_with(&RigOptions {
        upstream: Some(gated.origin.clone()),
        ..RigOptions::default()
    });
    let mut agent = rig.agent();
    let flight = rig.session.shared().first_flight().unwrap();
    for id in 1..=5 {
        agent.send_value(&request(id, "tools/list"));
    }
    // The pump stalls with 4 lines read; the fifth stays in the socket.
    assert_eq!(flight.wait_stalls(1), 4);
    let arrived = gated.wait_arrived(MAX_IN_FLIGHT);
    assert_eq!(flight.lines(), 4);
    assert_eq!(gated.arrived(), 4);
    // One answer frees a slot: the fifth line is read and forwarded.
    gated.release(arrived[0]);
    assert_eq!(agent.recv()["id"], arrived[0]);
    let arrived = gated.wait_arrived(5);
    assert_eq!(arrived[4], 5);
    for id in &arrived[1..] {
        gated.release(*id);
    }
    let mut ids: Vec<i64> = (0..4)
        .map(|_| agent.recv()["id"].as_i64().unwrap())
        .collect();
    ids.sort_unstable();
    let mut expected: Vec<i64> = arrived[1..].to_vec();
    expected.sort_unstable();
    assert_eq!(ids, expected);
}

#[test]
fn a_line_over_4_mb_gets_the_error_and_the_connection_closes() {
    let rig = rig();
    let mut agent = rig.agent();
    // Exactly the cap is a line (here not JSON, so the parse error answers it).
    agent.send_bytes(&vec![b'x'; MAX_LINE_BYTES]);
    assert_eq!(agent.recv()["error"]["code"], -32700);
    agent.send_bytes(&vec![b'x'; MAX_LINE_BYTES + 1]);
    assert_eq!(
        agent.recv(),
        json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": texts::LINE_TOO_LONG}})
    );
    assert!(agent.closed());
    assert_eq!(rig.mcp_requests().len(), 0);
}

/// A JSON-RPC answer to `id` whose serialised line is just over the relay's cap.
fn big_answer(id: u64) -> Vec<u8> {
    let pad = "p".repeat(MAX_REPLY_BYTES);
    json!({"jsonrpc": "2.0", "id": id, "result": {"tools": [], "pad": pad}})
        .to_string()
        .into_bytes()
}

#[test]
fn a_reply_over_4_mb_gets_the_reply_too_large_answer() {
    let rig = rig();
    rig.add_api_key();
    let pad = "p".repeat(MAX_REPLY_BYTES);
    rig.fake.push_tool_result(
        "search_law",
        Fixture::Json(json!({"content": [{"type": "text", "text": pad}], "isError": false})),
    );
    let mut agent = rig.agent();
    agent.send_value(&call(1, "search_law"));
    let reply = agent.recv();
    assert_eq!(reply["id"], 1);
    assert_eq!(error_text(&reply), texts::REPLY_TOO_LARGE);
    // Another method gets JSON-RPC error -32603 with the same text.
    let json = [("Content-Type", "application/json")];
    rig.fake
        .answer_next("/mcp/case-law", 200, &json, &big_answer(2));
    agent.send_value(&request(2, "tools/list"));
    assert_eq!(
        agent.recv(),
        json!({"jsonrpc": "2.0", "id": 2, "error": {"code": -32603, "message": texts::REPLY_TOO_LARGE}})
    );
    // Over S28's own 8 MB limit: the same answer.
    let mut huge = big_answer(3);
    huge.splice(1..1, vec![b' '; 2 * MAX_REPLY_BYTES]);
    rig.fake.answer_next("/mcp/case-law", 200, &json, &huge);
    agent.send_value(&request(3, "tools/list"));
    let reply: Value = agent.recv();
    assert_eq!(reply["error"]["message"], texts::REPLY_TOO_LARGE);
}

#[test]
fn a_connection_closed_for_a_long_line_keeps_its_slot_until_its_calls_finish() {
    let gated = Gated::start();
    let rig = rig_with(&RigOptions {
        upstream: Some(gated.origin.clone()),
        ..RigOptions::default()
    });
    let mut first = rig.agent();
    for id in 1..=3 {
        first.send_value(&request(id, "tools/list"));
    }
    gated.wait_arrived(3);
    first.send_bytes(&vec![b'x'; MAX_LINE_BYTES + 1]);
    assert_eq!(first.recv()["error"]["message"], texts::LINE_TOO_LONG);
    assert!(first.closed());
    // Its 3 calls are still at the upstream, so its slot is still taken: 3 more, not 4.
    let others = [rig.agent(), rig.agent(), rig.agent()];
    let mut fourth = Agent::connect(&rig.session);
    assert_eq!(
        fourth.hello(rig.token.expose(), "case-law"),
        Some(json!({"clauseo_relay": 1, "ok": false, "error": "busy"}))
    );
    drop(others);
    for id in 1..=3 {
        gated.release(id);
    }
}

/// A `tools/list` request padded to about `bytes` long.
fn padded(id: u64, bytes: usize) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {"pad": "p".repeat(bytes)}})
}

#[test]
fn the_session_holds_at_most_4_mb_of_lines_in_flight() {
    let gated = Gated::start();
    let rig = rig_with(&RigOptions {
        upstream: Some(gated.origin.clone()),
        ..RigOptions::default()
    });
    let session = rig.session.shared();
    let mut first = rig.agent();
    let mut second = rig.agent();
    // Small lines share the budget freely.
    first.send_value(&padded(1, 1000));
    second.send_value(&padded(2, 1000));
    gated.wait_arrived(2);
    // A line just under 4 MB waits for them, then takes the budget nearly whole.
    let size = MAX_IN_FLIGHT_BYTES_PER_SESSION - 200;
    first.send_value(&padded(3, size));
    session.wait_byte_waits(1);
    assert_eq!(gated.arrived(), 2);
    gated.release(1);
    gated.release(2);
    assert_eq!(first.recv()["id"], 1);
    assert_eq!(second.recv()["id"], 2);
    assert_eq!(gated.wait_arrived(3)[2], 3);
    // While it is out, a small line on another connection waits too.
    second.send_value(&padded(4, 1000));
    session.wait_byte_waits(2);
    assert_eq!(gated.arrived(), 3);
    gated.release(3);
    assert_eq!(first.recv()["id"], 3);
    assert_eq!(gated.wait_arrived(4)[3], 4);
    gated.release(4);
    assert_eq!(second.recv()["id"], 4);
}

#[test]
fn a_large_line_waiting_for_room_is_not_passed_by_later_small_ones() {
    let gated = Gated::start();
    let rig = rig_with(&RigOptions {
        upstream: Some(gated.origin.clone()),
        ..RigOptions::default()
    });
    let session = rig.session.shared();
    let mut first = rig.agent();
    let mut second = rig.agent();
    first.send_value(&padded(1, 1000));
    gated.wait_arrived(1);
    // The large line waits for the small call held upstream.
    first.send_value(&padded(2, MAX_IN_FLIGHT_BYTES_PER_SESSION - 200));
    session.wait_byte_waits(1);
    // A small line that would fit queues behind it all the same.
    second.send_value(&padded(3, 1000));
    session.wait_byte_waits(2);
    assert_eq!(gated.arrived(), 1);
    gated.release(1);
    assert_eq!(first.recv()["id"], 1);
    assert_eq!(gated.wait_arrived(2)[1], 2, "the large line goes first");
    assert_eq!(gated.arrived(), 2);
    gated.release(2);
    assert_eq!(gated.wait_arrived(3)[2], 3);
    gated.release(3);
    assert_eq!(first.recv()["id"], 2);
    assert_eq!(second.recv()["id"], 3);
}
