//! Plays scenarios through [`Runner`] over in-memory pipes: the client's lines are read from a
//! `Cursor`, the agent's lines are written to a `Vec`. No process and no clock.

use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;
use std::time::Duration;

use super::*;

/// Plays `scenario` against the client lines `input`. Returns why the run stopped and the
/// agent's stdout lines, each parsed as JSON.
fn play(scenario: &Scenario, input: &str) -> (Stop, Vec<Value>) {
    let (stop, lines, _) = play_paced(scenario, input, Duration::ZERO);
    (stop, lines)
}

/// Plays `scenario` with `pace` between updates. Also returns every pause the runner asked
/// for, so no test sleeps.
fn play_paced(
    scenario: &Scenario,
    input: &str,
    pace: Duration,
) -> (Stop, Vec<Value>, Vec<Duration>) {
    let pauses = Rc::new(RefCell::new(Vec::new()));
    let recorder = Rc::clone(&pauses);
    let mut runner = Runner {
        scenario,
        input: Cursor::new(input.as_bytes()),
        out: Vec::new(),
        cwd: None,
        session: None,
        labels: HashMap::new(),
        pace,
        pause: Box::new(move |duration| recorder.borrow_mut().push(duration)),
    };
    let stop = runner.play();
    let text = String::from_utf8(runner.out).unwrap();
    let lines = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let recorded = pauses.borrow().clone();
    (stop, lines, recorded)
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

const PROMPT: &str =
    r#"{"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{"sessionId":"sess-1"}}"#;

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

#[test]
fn pace_pauses_before_each_update_that_follows_an_update() {
    let update = r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-answer-1","content":{"type":"text","text":"Clause."}},"session":"sess-1"}}"#;
    let scenario = scenario_of(&[
        update,
        update,
        r#"{"expect":{"method":"session/prompt","as":"p1"}}"#,
        update,
        update,
    ]);
    let input = format!("{PROMPT}\n");
    let pace = Duration::from_millis(7);
    let (stop, lines, pauses) = play_paced(&scenario, &input, pace);
    assert_eq!(outcome(stop), None);
    assert_eq!(lines.len(), 4);
    // Before the second update and before the fourth. The third follows the expect, so no pause
    // comes before it.
    assert_eq!(pauses, [pace, pace]);
}

#[test]
fn pace_zero_never_pauses() {
    let update = r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-answer-1","content":{"type":"text","text":"Clause."}},"session":"sess-1"}}"#;
    let scenario = scenario_of(&[update, update, update]);
    let (stop, lines, pauses) = play_paced(&scenario, "", Duration::ZERO);
    assert_eq!(outcome(stop), None);
    assert_eq!(lines.len(), 3);
    assert!(pauses.is_empty(), "{pauses:?}");
}

/// A `one_of` on `session/new` or `session/load`, then one prompt both branches answer.
fn new_or_load() -> Scenario {
    scenario_of(&[
        r#"{"one_of":[
            [{"expect":{"method":"session/new","as":"new","params":{"cwd":"{{any}}"}}},
             {"respond":{"to":"new","result":{"sessionId":"sess-1"}}}],
            [{"expect":{"method":"session/load","as":"load","params":{"sessionId":"sess-1"}}},
             {"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-0","content":{"type":"text","text":"Replayed."}}}},
             {"respond":{"to":"load","result":{}}}]
        ]}"#
        .replace('\n', "")
        .as_str(),
        r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#,
        r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    ])
}

const NEW: &str =
    r#"{"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":"/w","mcpServers":[]}}"#;
const LOAD: &str = r#"{"jsonrpc":"2.0","id":1,"method":"session/load","params":{"sessionId":"sess-1","cwd":"/w","mcpServers":[]}}"#;

#[test]
fn one_of_takes_the_session_new_branch_and_goes_on_after_it() {
    let (stop, lines) = play(&new_or_load(), &format!("{NEW}\n{PROMPT}\n"));
    assert_eq!(outcome(stop), None);
    assert_eq!(
        lines,
        [
            json!({"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "sess-1"}}),
            json!({"jsonrpc": "2.0", "id": 2, "result": {"stopReason": "end_turn"}}),
        ]
    );
}

#[test]
fn one_of_takes_the_session_load_branch_and_goes_on_after_it() {
    let (stop, lines) = play(&new_or_load(), &format!("{LOAD}\n{PROMPT}\n"));
    assert_eq!(outcome(stop), None);
    assert_eq!(lines.len(), 3, "{lines:?}");
    // The branch's update goes to the session the load named.
    assert_eq!(lines[0]["params"]["sessionId"], "sess-1");
    assert_eq!(lines[0]["params"]["update"]["content"]["text"], "Replayed.");
    assert_eq!(lines[1], json!({"jsonrpc": "2.0", "id": 1, "result": {}}));
    assert_eq!(lines[2]["result"]["stopReason"], "end_turn");
}

#[test]
fn one_of_with_no_matching_branch_is_a_mismatch() {
    let (stop, lines) = play(&new_or_load(), &format!("{PROMPT}\n"));
    assert_eq!(
        outcome(stop).as_deref(),
        Some(
            "t.jsonl:2: expected one of: request session/new, request session/load; got \
             session/prompt"
        )
    );
    assert!(lines.is_empty(), "{lines:?}");
    // The right method with the wrong params names the params, as `expect` does.
    let other = LOAD.replace("sess-1", "sess-9");
    let (stop, _) = play(&new_or_load(), &format!("{other}\n"));
    assert_eq!(
        outcome(stop).as_deref(),
        Some("t.jsonl:2: session/load params: /sessionId: expected \"sess-1\", got \"sess-9\"")
    );
}

/// With one branch, a mismatch reads exactly as the same `expect` step's does.
#[test]
fn one_of_with_one_branch_fails_as_expect_does() {
    let expect = r#"{"expect":{"method":"session/new","as":"new"}}"#;
    let alone = scenario_of(&[expect]);
    let wrapped = scenario_of(&[&format!(r#"{{"one_of":[[{expect}]]}}"#)]);
    let input = format!("{PROMPT}\n");
    let (alone, _) = play(&alone, &input);
    let (wrapped, _) = play(&wrapped, &input);
    assert_eq!(
        outcome(wrapped).as_deref(),
        Some("t.jsonl:2: expected request session/new, got session/prompt")
    );
    assert_eq!(
        outcome(alone).as_deref(),
        Some("t.jsonl:2: expected request session/new, got session/prompt")
    );
}

#[test]
fn a_nested_one_of_plays_inside_its_branch() {
    let scenario = scenario_of(&[&r#"{"one_of":[[
        {"expect":{"method":"session/new","as":"new"}},
        {"one_of":[
            [{"expect":{"method":"session/prompt","as":"p1","params":{"prompt":[{"text":"a"}]}}},
             {"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}],
            [{"expect":{"method":"session/prompt","as":"p2"}},
             {"respond":{"to":"p2","result":{"stopReason":"cancelled"}}}]
        ]}
    ]]}"#
        .replace('\n', "")]);
    let prompt = |text: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{{"sessionId":"sess-1","prompt":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    };
    for (text, stop_reason) in [("a", "end_turn"), ("b", "cancelled")] {
        let (stop, lines) = play(&scenario, &format!("{NEW}\n{}\n", prompt(text)));
        assert_eq!(outcome(stop), None);
        assert_eq!(lines[0]["result"]["stopReason"], stop_reason, "{text}");
    }
}

#[test]
fn pace_counts_updates_across_a_branch() {
    let update = r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-answer-1","content":{"type":"text","text":"Clause."}},"session":"sess-1"}}"#;
    let one_of = format!(
        r#"{{"one_of":[[{{"expect":{{"method":"session/prompt","as":"p1"}}}},{update},{update}]]}}"#
    );
    let scenario = scenario_of(&[update, &one_of, update]);
    let pace = Duration::from_millis(7);
    let (stop, lines, pauses) = play_paced(&scenario, &format!("{PROMPT}\n"), pace);
    assert_eq!(outcome(stop), None);
    assert_eq!(lines.len(), 4);
    // Before the branch's second update and before the update after the one_of; the branch's
    // first update follows its expect.
    assert_eq!(pauses, [pace, pace]);
}
