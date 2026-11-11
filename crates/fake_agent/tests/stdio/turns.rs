//! `cancel-late-updates`, `queue-two-prompts` and `steer-fallback`: the order of a cancelled turn,
//! and the two mismatches a client can make by sending a message too early.
//! `claude-steer-injected`: the transcript line Claude Code writes for a steer it takes in, which
//! the host confirms it by.

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
    // codex-acp's handshake (common/codex-handshake.jsonl): a session, then the ask mode.
    agent.send(&request(
        1,
        "session/new",
        json!({"cwd": dir.cwd(), "mcpServers": []}),
    ));
    assert_eq!(agent.read_line()["result"]["sessionId"], "sess-1");
    agent.send(&request(
        4,
        "session/set_mode",
        json!({"sessionId": "sess-1", "modeId": "workspace-write"}),
    ));
    agent.expect_line(&result(4, json!({})));
    agent.send(&prompt(2, "Tighten clause 4"));
    agent.expect_line(&chunk("m-answer-1", "The clause "));
    agent.send(&request(
        3,
        "_session/steering",
        json!({"sessionId": "sess-1", "prompt": [{"type": "text", "text": "Focus on clause 4"}]}),
    ));
    assert_eq!(
        agent.expect_exit(EXIT_MISMATCH).stderr,
        "fake_agent: steer-fallback.jsonl:3: expected notification session/cancel, got \
         _session/steering\n"
    );
}

/// Plays `claude-steer-injected` up to its steer, with `CLAUDE_CONFIG_DIR` set to `home`.
fn steer_injected(dir: &Scratch, home: &str) -> Agent {
    let mut agent = Agent::start_args(
        &["--scenario", "claude-steer-injected"],
        dir.path(),
        &[("CLAUDE_CONFIG_DIR", home)],
    );
    agent.claude_handshake(&dir.cwd());
    agent.send(&prompt(2, "Summarise the indemnity clause"));
    agent.expect_line(&chunk("m-answer-1", "The clause "));
    agent.send(&request(
        3,
        "_session/steering",
        json!({"sessionId": "sess-1", "prompt": [{"type": "text", "text": "Focus on clause 4"}],
            "_meta": {"steering": {"idleBehavior": "promptRequired"}}}),
    ));
    agent.expect_line(&result(3, json!({"outcome": "injected"})));
    agent
}

#[test]
fn claude_steer_injected_writes_the_steer_to_the_transcript() {
    let dir = Scratch::new("claude_steer_injected_writes_the_steer_to_the_transcript");
    let home = dir.path().join("claude home");
    let mut agent = steer_injected(&dir, home.to_str().unwrap());
    agent.expect_line(&chunk("m-answer-1", "caps liability "));
    agent.expect_line(&chunk("m-answer-1", "at the fees paid."));
    agent.expect_line(&result(2, json!({"stopReason": "end_turn"})));
    agent.close_stdin();
    agent.expect_exit(EXIT_OK);

    // Claude Code's project key: every character but an ASCII letter or digit made `-`.
    let key: String = dir
        .cwd()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let path = home.join("projects").join(key).join("sess-1.jsonl");
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            json!({"type": "user", "timestamp": "2999-01-01T00:00:00.000Z",
            "sessionId": "sess-1", "cwd": dir.cwd(),
            "message": {"role": "user",
                "content": [{"type": "text", "text": "Focus on clause 4"}]}})
        ]
    );
}

#[test]
fn claude_steer_injected_without_a_claude_home_fails() {
    let dir = Scratch::new("claude_steer_injected_without_a_claude_home_fails");
    let agent = steer_injected(&dir, "");
    assert_eq!(
        agent.expect_exit(EXIT_MISMATCH).stderr,
        "fake_agent: claude-steer-injected.jsonl:10: transcript_user: CLAUDE_CONFIG_DIR is not \
         set\n"
    );
}
