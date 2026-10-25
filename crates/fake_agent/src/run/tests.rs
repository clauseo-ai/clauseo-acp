//! Plays scenarios through [`Runner`] over in-memory pipes: the client's lines are read from a
//! `Cursor`, the agent's lines are written to a `Vec`. No process and no clock.

use std::io::Cursor;

use super::*;

/// Plays `scenario` against the client lines `input`. Returns why the run stopped and the
/// agent's stdout lines, each parsed as JSON.
fn play(scenario: &Scenario, input: &str) -> (Stop, Vec<Value>) {
    let mut runner = Runner {
        scenario,
        input: Cursor::new(input.as_bytes()),
        out: Vec::new(),
        cwd: None,
        session: None,
        labels: HashMap::new(),
    };
    let stop = runner.play();
    let text = String::from_utf8(runner.out).unwrap();
    let lines = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (stop, lines)
}

/// The `file:line: message` of a run that failed, or `None` when it reached EOF.
fn outcome(stop: Stop) -> Option<String> {
    match stop {
        Stop::StdinClosed => None,
        Stop::Exit(code) => Some(format!("exit {code}")),
        Stop::Fail(at, message) => Some(format!("{}:{}: {message}", at.file, at.line)),
    }
}

/// A scenario named `t` with `steps`, one per line after its header.
fn scenario_of(steps: &[&str]) -> Scenario {
    let mut text = String::from(r#"{"scenario":"t","description":"test"}"#);
    for step in steps {
        text.push('\n');
        text.push_str(step);
    }
    Scenario::parse("t", &text).unwrap()
}

const INITIALIZE: &str =
    r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":1}}"#;

#[test]
fn load_replay_takes_the_cwd_and_session_from_session_load() {
    let scenario = Scenario::load(&crate::scenario_path("load-replay")).unwrap();
    let load = r#"{"jsonrpc":"2.0","id":1,"method":"session/load","params":{"sessionId":"sess-1","cwd":"/work/matter","mcpServers":[]}}"#;
    let input = format!("{INITIALIZE}\n{load}\n");
    let (stop, lines) = play(&scenario, &input);
    assert_eq!(outcome(stop), None);
    // The init response, four updates that name the session and the cwd, the load response.
    assert_eq!(lines.len(), 6, "{lines:?}");
    assert_eq!(lines[0]["id"], 0);
    for update in &lines[1..5] {
        assert_eq!(update["params"]["sessionId"], "sess-1");
    }
    let diff = &lines[3]["params"]["update"];
    assert_eq!(diff["content"][0]["path"], "/work/matter/brief.txt");
    assert_eq!(diff["locations"][0]["path"], "/work/matter/brief.txt");
    assert_eq!(lines[5], json!({"jsonrpc": "2.0", "id": 1, "result": {}}));
}

#[test]
fn expect_response_matches_an_error_reply() {
    let scenario = scenario_of(&[
        r#"{"request":{"id":"perm-1","method":"fake/ask","params":{},"raw":true}}"#,
        r#"{"expect_response":{"id":"perm-1","error":{"code":-32000}}}"#,
    ]);
    let error = r#"{"jsonrpc":"2.0","id":"perm-1","error":{"code":-32000,"message":"denied"}}"#;
    let (stop, _) = play(&scenario, &format!("{error}\n"));
    assert_eq!(outcome(stop), None);

    let result = r#"{"jsonrpc":"2.0","id":"perm-1","result":{}}"#;
    let (stop, _) = play(&scenario, &format!("{result}\n"));
    assert_eq!(
        outcome(stop).as_deref(),
        Some("t.jsonl:3: expected an error in the response to perm-1, got result {}")
    );

    let other = r#"{"jsonrpc":"2.0","id":"perm-1","error":{"code":1}}"#;
    let (stop, _) = play(&scenario, &format!("{other}\n"));
    assert_eq!(
        outcome(stop).as_deref(),
        Some("t.jsonl:3: response to perm-1 error: /code: expected -32000, got 1")
    );
}

#[test]
fn a_numeric_response_id_is_named_by_its_number() {
    let scenario = scenario_of(&[
        r#"{"request":{"id":"7","method":"fake/ask","params":{},"raw":true}}"#,
        r#"{"expect_response":{"id":"7","result":{}}}"#,
    ]);
    let (stop, _) = play(&scenario, "{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n");
    assert_eq!(
        outcome(stop).as_deref(),
        Some("t.jsonl:3: expected response to 7, got response to 7")
    );
}

#[test]
fn cwd_before_any_session_fails_the_step() {
    let scenario = scenario_of(&[
        r#"{"notify":{"method":"fake/note","params":{"path":"{{cwd}}/a.txt"},"raw":true}}"#,
    ]);
    let (stop, lines) = play(&scenario, "");
    assert_eq!(outcome(stop), Some(format!("t.jsonl:2: {NO_CWD}")));
    assert!(lines.is_empty(), "{lines:?}");
}
