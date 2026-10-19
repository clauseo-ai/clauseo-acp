//! `initialize-caps` and a plain mismatch.

use serde_json::{Value, json};

use crate::harness::{Agent, Scratch, request, result};

/// The `initialize` params Clauseo's client sends.
fn clauseo_initialize() -> Value {
    json!({"protocolVersion": 1, "clientInfo": {"name": "clauseo", "version": "0.0.0"},
        "clientCapabilities": {
            "fs": {"readTextFile": false, "writeTextFile": false},
            "terminal": false,
            "auth": {"terminal": true},
            "elicitation": {"form": {}, "url": {}},
            "session": {"notices": {}, "compaction": {}, "configOptions": {"boolean": {}}},
            "plan": {},
            "_meta": {"jetbrains": {"air": {"version": 1, "capabilities": ["sessionFailure"]}}}}})
}

#[test]
fn initialize_caps_over_stdio() {
    let dir = Scratch::new("initialize_caps_over_stdio");
    let mut agent = Agent::start("initialize-caps", dir.path());
    agent.send(&request(0, "initialize", clauseo_initialize()));
    agent.expect_line(&result(0, json!({"protocolVersion": 1,
        "agentCapabilities": {"loadSession": true,
            "promptCapabilities": {"image": false, "audio": false, "embeddedContext": true},
            "sessionCapabilities": {"list": {}, "resume": {}, "close": {}}},
        "authMethods": [
            {"id": "fake-login", "name": "Log in to fake_agent", "description": "The agent signs in by itself"},
            {"type": "terminal", "id": "fake-terminal-login", "name": "Log in in a terminal", "args": ["--login"]}],
        "agentInfo": {"name": "fake_agent", "version": env!("CARGO_PKG_VERSION")}})));
    agent.send(&request(
        1,
        "session/new",
        json!({"cwd": dir.cwd(), "mcpServers": []}),
    ));
    let response = agent.read_line();
    assert_eq!(response["result"]["sessionId"], "sess-1");
    assert_eq!(response["result"]["modes"]["currentModeId"], "default");
    let options = response["result"]["configOptions"].as_array().unwrap();
    assert_eq!(options.len(), 1);
    assert_eq!(options[0]["type"], "select");
    agent.expect_exit(0);
}

#[test]
fn initialize_caps_refuses_other_capabilities() {
    let cases = [
        (
            "/clientCapabilities/fs/readTextFile",
            json!(true),
            "fake_agent: initialize-caps.jsonl:2: initialize params: /clientCapabilities/fs: expected \
             off (missing, null, false or all off), got {\"readTextFile\":true,\"writeTextFile\":false}",
        ),
        (
            "/clientCapabilities/auth/terminal",
            json!(false),
            "fake_agent: initialize-caps.jsonl:2: initialize params: /clientCapabilities/auth/terminal: \
             expected true, got false",
        ),
        (
            "/clientCapabilities/_meta/jetbrains/air/capabilities",
            json!([]),
            "fake_agent: initialize-caps.jsonl:2: initialize params: \
             /clientCapabilities/_meta/jetbrains/air/capabilities: expected 1 items, got 0",
        ),
    ];
    for (index, (pointer, value, message)) in cases.into_iter().enumerate() {
        let dir = Scratch::new(&format!("initialize_caps_refuses_{index}"));
        let mut params = clauseo_initialize();
        *params.pointer_mut(pointer).unwrap() = value;
        let mut agent = Agent::start("initialize-caps", dir.path());
        agent.send(&request(0, "initialize", params));
        let finished = agent.expect_exit(3);
        assert_eq!(finished.stderr, format!("{message}\n"));
    }
    let dir = Scratch::new("initialize_caps_refuses_plan");
    let mut params = clauseo_initialize();
    params["clientCapabilities"]
        .as_object_mut()
        .unwrap()
        .remove("plan");
    let mut agent = Agent::start("initialize-caps", dir.path());
    agent.send(&request(0, "initialize", params));
    assert_eq!(
        agent.expect_exit(3).stderr,
        "fake_agent: initialize-caps.jsonl:2: initialize params: /clientCapabilities/plan: missing, \
         expected \"{{any}}\"\n"
    );
}

#[test]
fn mismatch_exits_3() {
    let dir = Scratch::new("mismatch_exits_3");
    let mut agent = Agent::start("initialize-caps", dir.path());
    agent.send(&request(
        0,
        "session/new",
        json!({"cwd": dir.cwd(), "mcpServers": []}),
    ));
    let finished = agent.expect_exit(3);
    assert_eq!(
        finished.stderr,
        "fake_agent: initialize-caps.jsonl:2: expected request initialize, got session/new\n"
    );
}

#[test]
fn message_after_the_last_step_is_a_mismatch() {
    let dir = Scratch::new("message_after_the_last_step");
    let mut agent = Agent::start("idle", dir.path());
    agent.send(&json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {}}));
    assert_eq!(
        agent.expect_exit(3).stderr,
        "fake_agent: idle.jsonl:1: unexpected notification session/cancel after the last step\n"
    );
    let dir = Scratch::new("idle_exits_0_on_eof");
    Agent::start("idle", dir.path()).expect_exit(0);
}
