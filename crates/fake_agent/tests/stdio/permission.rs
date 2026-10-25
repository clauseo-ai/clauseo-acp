//! `permission-cancelled`: the cancel and the cancelled answer to `perm-1` in either order.

use clauseo_fake_agent::{EXIT_MISMATCH, EXIT_OK};
use serde_json::{Value, json};

use crate::harness::{Agent, Scratch, notification, prompt, result, update};

/// Starts the scenario and reads up to the `perm-1` request.
fn up_to_permission(test: &str) -> (Scratch, Agent) {
    let dir = Scratch::new(test);
    let mut agent = Agent::start("permission-cancelled", dir.path());
    agent.handshake(&dir.cwd());
    agent.send(&prompt(2, "Tighten clause 4"));
    let location = format!("{}/brief.txt", dir.cwd());
    agent.expect_line(&update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t-edit-1",
        "title": "Edit brief.txt", "name": "Edit", "kind": "edit", "status": "pending",
        "locations": [{"path": location}]}),
    ));
    let permission = agent.read_line();
    assert_eq!(permission["id"], "perm-1");
    assert_eq!(permission["method"], "session/request_permission");
    let kinds: Vec<&Value> = permission["params"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| &o["kind"])
        .collect();
    assert_eq!(
        kinds,
        ["allow_once", "allow_always", "reject_once", "reject_always"]
    );
    (dir, agent)
}

fn cancel() -> Value {
    notification("session/cancel", json!({"sessionId": "sess-1"}))
}

fn cancelled_answer() -> Value {
    json!({"jsonrpc": "2.0", "id": "perm-1", "result": {"outcome": {"outcome": "cancelled"}}})
}

fn finish_turn(mut agent: Agent) {
    agent.expect_line(&update(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "t-edit-1",
        "status": "failed"}),
    ));
    agent.expect_line(&result(2, json!({"stopReason": "cancelled"})));
    agent.expect_exit(EXIT_OK);
}

#[test]
fn permission_cancelled_cancel_first() {
    let (_dir, mut agent) = up_to_permission("permission_cancelled_cancel_first");
    agent.send(&cancel());
    agent.send(&cancelled_answer());
    finish_turn(agent);
}

#[test]
fn permission_cancelled_response_first() {
    let (_dir, mut agent) = up_to_permission("permission_cancelled_response_first");
    agent.send(&cancelled_answer());
    agent.send(&cancel());
    finish_turn(agent);
}

#[test]
fn permission_selected_instead_of_cancelled_is_a_mismatch() {
    let (_dir, mut agent) = up_to_permission("permission_selected_instead_of_cancelled");
    agent.send(&cancel());
    agent.send(&json!({"jsonrpc": "2.0", "id": "perm-1",
        "result": {"outcome": {"outcome": "selected", "optionId": "allow-once"}}}));
    let stderr = agent.expect_exit(EXIT_MISMATCH).stderr;
    assert_eq!(
        stderr,
        "fake_agent: permission-cancelled.jsonl:6: response to perm-1 result: /outcome/outcome: \
         expected \"cancelled\", got \"selected\"\n"
    );
}

#[test]
fn permission_other_message_in_all_is_a_mismatch() {
    let (_dir, mut agent) = up_to_permission("permission_other_message_in_all");
    agent.send(&prompt(3, "Another question"));
    let stderr = agent.expect_exit(EXIT_MISMATCH).stderr;
    assert_eq!(
        stderr,
        "fake_agent: permission-cancelled.jsonl:6: expected one of: notification session/cancel, \
         response to perm-1; got session/prompt\n"
    );
}
