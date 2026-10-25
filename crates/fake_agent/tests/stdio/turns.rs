//! `cancel-late-updates`, `queue-two-prompts` and `steer-fallback`: the order of a cancelled turn,
//! and the two mismatches a client can make by sending a message too early.

use clauseo_fake_agent::{EXIT_MISMATCH, EXIT_OK};
use serde_json::{Value, json};

use crate::harness::{Agent, Scratch, notification, prompt, request, result, update};

fn chunk(id: &str, text: &str) -> Value {
    update(
        json!({"sessionUpdate": "agent_message_chunk", "messageId": id,
        "content": {"type": "text", "text": text}}),
    )
}

#[test]
fn cancel_late_updates_order() {
    let dir = Scratch::new("cancel_late_updates_order");
    let mut agent = Agent::start("cancel-late-updates", dir.path());
    agent.handshake(&dir.cwd());
    agent.send(&prompt(2, "Tighten clause 4"));
    agent.expect_line(&update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "tc-late-1",
        "title": "Search for \"cap\"", "name": "Grep", "kind": "search", "status": "in_progress",
        "locations": [{"path": format!("{}/brief.txt", dir.cwd())}]}),
    ));
    agent.expect_line(&chunk("m-answer-1", "The clause "));
    agent.send(&notification(
        "session/cancel",
        json!({"sessionId": "sess-1"}),
    ));
    agent.expect_line(&chunk("m-answer-1", "caps liability "));
    agent.expect_line(&chunk("m-answer-1", "at the fees "));
    agent.expect_line(&update(json!({"sessionUpdate": "tool_call_update",
        "toolCallId": "tc-late-1", "status": "completed"})));
    agent.expect_line(&result(2, json!({"stopReason": "cancelled"})));
    agent.expect_line(&chunk("m-answer-1", "Still writing."));
    agent.send(&prompt(3, "Still there?"));
    agent.expect_line(&chunk("m-answer-2", "Second answer."));
    agent.expect_line(&result(3, json!({"stopReason": "end_turn"})));
    agent.expect_exit(EXIT_OK);
}

#[test]
fn queue_two_prompts_early_prompt_is_mismatch() {
    let dir = Scratch::new("queue_two_prompts_early_prompt_is_mismatch");
    let mut agent = Agent::start("queue-two-prompts", dir.path());
    agent.handshake(&dir.cwd());
    agent.send(&prompt(2, "First question"));
    let permission = agent.read_line();
    assert_eq!(permission["id"], "perm-1");
    assert_eq!(permission["method"], "session/request_permission");
    agent.send(&prompt(3, "Second question"));
    assert_eq!(
        agent.expect_exit(EXIT_MISMATCH).stderr,
        "fake_agent: queue-two-prompts.jsonl:5: expected response to perm-1, got session/prompt\n"
    );
}

#[test]
fn steer_fallback_steering_is_mismatch() {
    let dir = Scratch::new("steer_fallback_steering_is_mismatch");
    let mut agent = Agent::start("steer-fallback", dir.path());
    agent.send(&request(
        0,
        "initialize",
        json!({"protocolVersion": 1, "clientCapabilities": {}}),
    ));
    let init = agent.read_line();
    assert_eq!(init["result"]["_meta"]["steering"]["supported"], true);
    agent.send(&request(
        1,
        "session/new",
        json!({"cwd": dir.cwd(), "mcpServers": []}),
    ));
    agent.expect_line(&result(1, json!({"sessionId": "sess-1"})));
    agent.send(&prompt(2, "Tighten clause 4"));
    agent.expect_line(&chunk("m-answer-1", "The clause "));
    agent.send(&request(
        3,
        "_session/steering",
        json!({"sessionId": "sess-1", "prompt": [{"type": "text", "text": "Focus on clause 4"}]}),
    ));
    assert_eq!(
        agent.expect_exit(EXIT_MISMATCH).stderr,
        "fake_agent: steer-fallback.jsonl:8: expected notification session/cancel, got \
         _session/steering\n"
    );
}
