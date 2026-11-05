//! The caps through the relay: 60 paid calls per turn (`account` free), the between-turns
//! bucket, 5 `recharge` calls per turn, and 600 paid calls per local day kept in `day.json`.

use std::time::Duration;

use serde_json::json;

use super::super::super::{MAX_PAID_CALLS_PER_TURN, MAX_RECHARGE_CALLS_PER_TURN, texts};
use super::support::*;

/// Sends `count` calls of `tool` from `first`; each answer must not be a cap's.
fn calls_pass(agent: &mut Agent, first: u64, count: u32, tool: &str) {
    for n in 0..u64::from(count) {
        agent.send_value(&call(first + n, tool));
        let reply = agent.recv();
        assert_eq!(reply["result"]["isError"], false, "call {n}: {reply}");
    }
}

#[test]
fn the_61st_paid_call_of_a_turn_gets_the_cap_and_the_next_turn_starts_at_0() {
    let rig = rig();
    rig.add_api_key();
    rig.fake.set_balance(1_000_000);
    let mut agent = rig.agent();
    let turns = rig.session.turns();
    turns.start();
    calls_pass(&mut agent, 1, MAX_PAID_CALLS_PER_TURN, "search_law");
    assert_eq!(rig.mcp_requests().len(), 60);
    agent.send_value(&call(100, "citator"));
    let reply = agent.recv();
    assert_eq!(reply["id"], 100);
    assert_eq!(error_text(&reply), texts::TURN_CAP);
    assert_eq!(
        rig.mcp_requests().len(),
        60,
        "the 61st did not reach the fake"
    );
    // `account` is not counted.
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 101, "method": "tools/call",
        "params": {"name": "account", "arguments": {}}}));
    assert_eq!(agent.recv()["result"]["isError"], false);
    turns.end();
    turns.start();
    calls_pass(&mut agent, 200, 1, "search_law");
}

#[test]
fn the_between_turns_bucket_keeps_its_count_until_the_next_turn_starts() {
    let rig = rig();
    rig.add_api_key();
    rig.fake.set_balance(1_000_000);
    let mut agent = rig.agent();
    // No turn yet: the between-turns bucket.
    calls_pass(&mut agent, 1, MAX_PAID_CALLS_PER_TURN, "search_law");
    agent.send_value(&call(100, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::TURN_CAP);
    agent.send_value(&call(101, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::TURN_CAP);
    rig.session.turns().start();
    calls_pass(&mut agent, 200, 1, "search_law");
}

#[test]
fn a_sixth_recharge_call_in_a_turn_gets_the_recharge_cap() {
    let rig = rig();
    rig.add_api_key();
    let mut agent = rig.agent();
    rig.session.turns().start();
    for n in 0..u64::from(MAX_RECHARGE_CALLS_PER_TURN) {
        agent.send_value(&json!({"jsonrpc": "2.0", "id": n, "method": "tools/call",
            "params": {"name": "recharge", "arguments": {"pack": "200"}}}));
        agent.recv();
    }
    assert_eq!(rig.mcp_requests().len(), 5);
    agent.send_value(&json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": {"name": "recharge", "arguments": {"pack": "200"}}}));
    assert_eq!(error_text(&agent.recv()), texts::RECHARGE_CAP);
    assert_eq!(rig.mcp_requests().len(), 5);
    // Recharge calls are not paid calls: none reached day.json.
    assert!(!rig.day_file().exists());
    calls_pass(&mut agent, 20, 1, "search_law");
}

#[test]
fn the_601st_paid_call_of_a_local_day_gets_the_day_cap_until_midnight() {
    let rig = rig_with(&RigOptions {
        day_json: Some(r#"{"day":"2026-11-02","paid":599}"#.to_owned()),
        ..RigOptions::default()
    });
    rig.add_api_key();
    let mut agent = rig.agent();
    let turns = rig.session.turns();
    turns.start();
    calls_pass(&mut agent, 1, 1, "search_law");
    assert_eq!(
        std::fs::read_to_string(rig.day_file()).unwrap(),
        r#"{"day":"2026-11-02","paid":600}"#
    );
    agent.send_value(&call(2, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::DAY_CAP);
    // In any turn, and between turns.
    turns.end();
    agent.send_value(&call(3, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::DAY_CAP);
    turns.start();
    agent.send_value(&call(4, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::DAY_CAP);
    let reached = rig.mcp_requests().len();
    // 18:29:59 UTC is 23:59:59 in India: still the same day.
    rig.clock
        .advance(Duration::from_secs(18 * 3600 + 29 * 60 + 59));
    agent.send_value(&call(5, "search_law"));
    assert_eq!(error_text(&agent.recv()), texts::DAY_CAP);
    assert_eq!(rig.mcp_requests().len(), reached);
    rig.clock.advance(Duration::from_secs(1));
    calls_pass(&mut agent, 6, 1, "search_law");
    assert_eq!(
        std::fs::read_to_string(rig.day_file()).unwrap(),
        r#"{"day":"2026-11-03","paid":1}"#
    );
}
