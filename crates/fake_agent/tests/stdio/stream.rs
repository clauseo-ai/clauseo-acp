//! `prompt-stream`, `malformed-line` and `exit-mid-turn`.

use serde_json::json;

use crate::harness::{Agent, Scratch, prompt, result, update};

fn chunk(kind: &str, id: &str, text: &str) -> serde_json::Value {
    update(
        json!({"sessionUpdate": kind, "messageId": id, "content": {"type": "text", "text": text}}),
    )
}

#[test]
fn prompt_stream_over_stdio() {
    let dir = Scratch::new("prompt_stream_over_stdio");
    let mut agent = Agent::start("prompt-stream", dir.path());
    agent.handshake(&dir.cwd());
    agent.send(&prompt(2, "Summarise the indemnity clause"));
    for text in ["Reading the ", "indemnity clause ", "first."] {
        agent.expect_line(&chunk("agent_thought_chunk", "m-thought-1", text));
    }
    let answer = [
        "The clause ",
        "caps liability ",
        "at the fees ",
        "paid in the ",
        "last twelve months.",
    ];
    for text in answer {
        agent.expect_line(&chunk("agent_message_chunk", "m-answer-1", text));
    }
    agent.expect_line(&result(2, json!({"stopReason": "end_turn"})));
    agent.expect_exit(0);
}

#[test]
fn prompt_for_another_session_is_a_mismatch() {
    let dir = Scratch::new("prompt_for_another_session");
    let mut agent = Agent::start("prompt-stream", dir.path());
    agent.handshake(&dir.cwd());
    let mut wrong = prompt(2, "Hello");
    wrong["params"]["sessionId"] = json!("sess-2");
    agent.send(&wrong);
    assert_eq!(
        agent.expect_exit(3).stderr,
        "fake_agent: prompt-stream.jsonl:3: session/prompt params: /sessionId: expected \"sess-1\", \
         got \"sess-2\"\n"
    );
}

#[test]
fn malformed_line_written_verbatim() {
    let dir = Scratch::new("malformed_line_written_verbatim");
    let mut agent = Agent::start("malformed-line", dir.path());
    agent.handshake(&dir.cwd());
    agent.send(&prompt(2, "Go"));
    agent.expect_line(&chunk(
        "agent_message_chunk",
        "m-answer-1",
        "Before the broken line. ",
    ));
    assert_eq!(
        agent.read_raw(),
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_mess"#
    );
    agent.expect_line(&chunk(
        "agent_message_chunk",
        "m-answer-1",
        "After the broken line.",
    ));
    agent.expect_line(&result(2, json!({"stopReason": "end_turn"})));
    agent.expect_exit(0);
}

#[test]
fn malformed_client_line_is_a_mismatch() {
    let dir = Scratch::new("malformed_client_line");
    let mut agent = Agent::start("prompt-stream", dir.path());
    agent.send_raw(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","par"#);
    assert_eq!(
        agent.expect_exit(3).stderr,
        "fake_agent: handshake.jsonl:1: malformed line from the client: \
         {\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\",\"par\n"
    );
}

#[test]
fn exit_mid_turn_code_and_stderr() {
    let dir = Scratch::new("exit_mid_turn_code_and_stderr");
    let mut agent = Agent::start("exit-mid-turn", dir.path());
    agent.handshake(&dir.cwd());
    agent.send(&prompt(2, "Go"));
    agent.expect_line(&chunk(
        "agent_message_chunk",
        "m-answer-1",
        "Starting on the answer.",
    ));
    let finished = agent.expect_exit(1);
    let lines: Vec<&str> = finished.stderr.lines().collect();
    assert_eq!(lines.len(), 200);
    assert!(lines.iter().all(|line| line.len() + 1 == 40), "{lines:?}");
    assert_eq!(finished.stderr.len(), 8000);
    assert!(finished.stderr.len() > 4096);
}
