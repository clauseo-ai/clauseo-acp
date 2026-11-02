//! `prompt-stream`, `malformed-line`, `exit-mid-turn` and `exit-mid-turn-then-load`.

use clauseo_fake_agent::{EXIT_MISMATCH, EXIT_OK};
use serde_json::json;

use crate::harness::{Agent, Scratch, prompt, request, result, update};

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
    agent.expect_exit(EXIT_OK);
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
        agent.expect_exit(EXIT_MISMATCH).stderr,
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
    agent.expect_exit(EXIT_OK);
}

#[test]
fn malformed_client_line_is_a_mismatch() {
    let dir = Scratch::new("malformed_client_line");
    let mut agent = Agent::start("prompt-stream", dir.path());
    agent.send_raw(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","par"#);
    assert_eq!(
        agent.expect_exit(EXIT_MISMATCH).stderr,
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

/// Two launches of `exit-mid-turn-then-load`, as a host relaunching a stopped agent makes: the
/// first, after `session/new`, exits mid-turn; the second, after `session/load`, answers.
#[test]
fn exit_mid_turn_then_load_answers_after_the_relaunch() {
    let dir = Scratch::new("exit_mid_turn_then_load");
    let mut first = Agent::start("exit-mid-turn-then-load", dir.path());
    first.handshake(&dir.cwd());
    first.send(&prompt(2, "Go"));
    first.expect_line(&chunk(
        "agent_message_chunk",
        "m-answer-1",
        "Starting on the answer.",
    ));
    assert_eq!(first.expect_exit(1).stderr.lines().count(), 200);

    let mut second = Agent::start("exit-mid-turn-then-load", dir.path());
    second.send(&request(
        0,
        "initialize",
        json!({"protocolVersion": 1, "clientCapabilities": {}}),
    ));
    assert_eq!(
        second.read_line()["result"]["agentCapabilities"]["loadSession"],
        true
    );
    second.send(&request(
        1,
        "session/load",
        json!({"sessionId": "sess-1", "cwd": dir.cwd(), "mcpServers": []}),
    ));
    second.expect_line(&chunk(
        "agent_message_chunk",
        "m-answer-1",
        "Starting on the answer.",
    ));
    second.expect_line(&result(1, json!({})));
    second.send(&prompt(2, "Go on"));
    second.expect_line(&chunk(
        "agent_message_chunk",
        "m-answer-2",
        "Second answer.",
    ));
    second.expect_line(&result(2, json!({"stopReason": "end_turn"})));
    assert_eq!(second.expect_exit(EXIT_OK).stderr, "");
}

#[test]
fn exit_mid_turn_then_load_refuses_a_load_of_another_session() {
    let dir = Scratch::new("exit_mid_turn_then_load_other");
    let mut agent = Agent::start("exit-mid-turn-then-load", dir.path());
    agent.send(&request(
        0,
        "initialize",
        json!({"protocolVersion": 1, "clientCapabilities": {}}),
    ));
    agent.read_line();
    agent.send(&request(
        1,
        "session/load",
        json!({"sessionId": "sess-2", "cwd": dir.cwd(), "mcpServers": []}),
    ));
    assert_eq!(
        agent.expect_exit(EXIT_MISMATCH).stderr,
        "fake_agent: exit-mid-turn-then-load.jsonl:4: session/load params: /sessionId: expected \
         \"sess-1\", got \"sess-2\"\n"
    );
}
