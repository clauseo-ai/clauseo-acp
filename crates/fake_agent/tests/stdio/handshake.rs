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

/// Sets the value at `pointer`, creating the last key if it is missing.
fn set(params: &mut Value, pointer: &str, value: Value) {
    let (parent, key) = pointer.rsplit_once('/').unwrap();
    params
        .pointer_mut(parent)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert(key.to_owned(), value);
}

/// Sends `params` as `initialize` and returns the agent's stderr after it exits 3.
fn refused(test: &str, params: Value) -> String {
    let dir = Scratch::new(test);
    let mut agent = Agent::start("initialize-caps", dir.path());
    agent.send(&request(0, "initialize", params));
    agent.expect_exit(3).stderr
}

const AT: &str = "fake_agent: initialize-caps.jsonl:2: initialize params: ";
const OFF: &str = "expected off (missing, null, false, or a non-empty object of off values)";

#[test]
fn initialize_caps_refuses_other_capabilities() {
    let cases = [
        (
            "/clientCapabilities/fs/readTextFile",
            json!(true),
            format!(
                "/clientCapabilities/fs: {OFF}, got {{\"readTextFile\":true,\"writeTextFile\":false}}"
            ),
        ),
        (
            "/clientCapabilities/subagents",
            json!({}),
            format!("/clientCapabilities/subagents: {OFF}, got {{}}"),
        ),
        (
            "/clientCapabilities/terminal",
            json!(true),
            format!("/clientCapabilities/terminal: {OFF}, got true"),
        ),
        (
            "/clientCapabilities/auth/terminal",
            json!(false),
            "/clientCapabilities/auth/terminal: expected true, got false".to_owned(),
        ),
        (
            "/clientCapabilities/_meta/jetbrains/air/capabilities",
            json!([]),
            "/clientCapabilities/_meta/jetbrains/air/capabilities: expected 1 items, got 0"
                .to_owned(),
        ),
    ];
    for (index, (pointer, value, message)) in cases.into_iter().enumerate() {
        let mut params = clauseo_initialize();
        set(&mut params, pointer, value);
        let stderr = refused(&format!("initialize_caps_refuses_{index}"), params);
        assert_eq!(stderr, format!("{AT}{message}\n"));
    }
}

/// Each presence-only capability must be an object: `null`, `false` or a missing key is no
/// advertisement in schema 1.11.0.
#[test]
fn initialize_caps_refuses_null_or_false_capabilities() {
    let pointers = [
        "/clientCapabilities/elicitation/form",
        "/clientCapabilities/elicitation/url",
        "/clientCapabilities/session/notices",
        "/clientCapabilities/session/compaction",
        "/clientCapabilities/session/configOptions/boolean",
        "/clientCapabilities/plan",
    ];
    for (index, pointer) in pointers.into_iter().enumerate() {
        for value in [json!(null), json!(false)] {
            let mut params = clauseo_initialize();
            set(&mut params, pointer, value.clone());
            let stderr = refused(
                &format!("initialize_caps_refuses_absent_{index}_{value}"),
                params,
            );
            assert_eq!(
                stderr,
                format!("{AT}{pointer}: expected an object, got {value}\n")
            );
        }
    }
    let mut params = clauseo_initialize();
    params["clientCapabilities"]
        .as_object_mut()
        .unwrap()
        .remove("plan");
    let stderr = refused("initialize_caps_refuses_missing_plan", params);
    assert_eq!(
        stderr,
        format!("{AT}/clientCapabilities/plan: missing, expected {{}}\n")
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

#[test]
fn last_line_without_newline_is_a_mismatch() {
    let line = request(0, "initialize", json!({"protocolVersion": 1})).to_string();
    for scenario in ["idle", "prompt-stream"] {
        let dir = Scratch::new(&format!("last_line_without_newline_{scenario}"));
        let mut agent = Agent::start(scenario, dir.path());
        agent.send_unterminated(&line);
        let at = if scenario == "idle" {
            "idle.jsonl:1"
        } else {
            "handshake.jsonl:1"
        };
        assert_eq!(
            agent.expect_exit(3).stderr,
            format!("fake_agent: {at}: client line has no newline before stdin closed\n")
        );
    }
}
