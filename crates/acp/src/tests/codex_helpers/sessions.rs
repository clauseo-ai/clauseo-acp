//! Helper sessions across the connection's sessions: two roots, a helper announced again or
//! under another root, a spawn on a session this connection never started, a session started
//! under a helper's id, frames after a helper's end, a failed load, and a skipped update. Each
//! step's events are taken once the step is over: a request's answer comes after every frame the
//! agent sent before it.

use super::*;
use crate::line::MAX_LINE_BYTES;

/// One step, as the client and the agent both see it.
enum Step {
    /// `session/new`, answered with this id.
    New(&'static str),
    /// `session/load` of `id`: the agent replays `replay`, then answers, or refuses when `fails`.
    Load {
        id: &'static str,
        replay: Vec<Value>,
        fails: bool,
    },
    /// The agent sends these frames.
    Send(Vec<Value>),
    /// The agent writes this line as it is.
    Raw(Vec<u8>),
}

/// Plays `steps` with helper sessions on; gives each step's events.
fn run(steps: &[Step]) -> Vec<Vec<AcpEvent>> {
    let (client, events, driver, mut agent) = pipes_with(options(true));
    let seen = &events;
    let calls = async move {
        client.initialize().await.unwrap();
        let mut got = Vec::new();
        for step in steps {
            match step {
                Step::New(_) => {
                    client.new_session(new_session()).await.unwrap();
                }
                Step::Load { id, fails, .. } => {
                    let load = client.load_session(LoadSession {
                        session: super::id(id),
                        cwd: folder(),
                        additional_directories: Vec::new(),
                        mcp_servers: Vec::new(),
                        meta: None,
                    });
                    assert_eq!(load.await.is_err(), *fails, "{id}");
                }
                // A request whose answer comes after the frames, as a barrier.
                Step::Send(_) | Step::Raw(_) => {
                    client.codex_hooks_list(&folder()).await.unwrap();
                }
            }
            got.push(drain(seen));
        }
        got
    };
    let script = async {
        agent.initialize().await;
        for step in steps {
            let request = agent.read().await;
            match step {
                Step::New(id) => agent.respond(&request, &json!({"sessionId": id})),
                Step::Load { replay, fails, .. } => {
                    for frame in replay {
                        agent.send(frame);
                    }
                    if *fails {
                        agent.send(&json!({"jsonrpc": "2.0", "id": request["id"],
                            "error": {"code": -32603, "message": "no such session"}}));
                    } else {
                        agent.respond(&request, &json!({}));
                    }
                }
                Step::Send(frames) => {
                    for frame in frames {
                        agent.send(frame);
                    }
                    agent.respond(&request, &json!({}));
                }
                Step::Raw(line) => {
                    agent.stdout.unbounded_send(Ok(line.clone())).unwrap();
                    agent.respond(&request, &json!({}));
                }
            }
        }
        agent.read_end().await;
    };
    let (driver, got, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    got
}

fn new_session() -> NewSession {
    NewSession {
        cwd: folder(),
        additional_directories: Vec::new(),
        mcp_servers: Vec::new(),
        meta: None,
    }
}

/// `update` sent on `session`.
fn on(session: &str, update: &Value) -> Value {
    json!({"jsonrpc": "2.0", "method": "session/update",
        "params": {"sessionId": session, "update": update}})
}

fn spawned(session: &str, helper: &str, name: &str) -> Value {
    on(
        session,
        &json!({"sessionUpdate": "subagent_spawned", "subagentSessionId": helper, "name": name,
            "task": format!("Delegated task for {name}"), "capabilities": {}}),
    )
}

fn ended(session: &str, helper: &str, state: &str) -> Value {
    on(
        session,
        &json!({"sessionUpdate": "subagent_state_update", "subagentSessionId": helper,
            "state": state}),
    )
}

/// A helper's first update in every recording, its model option, sent on `session`.
fn options_on(session: &str) -> Value {
    let mut frame = frames(X1).swap_remove(2);
    frame["params"]["sessionId"] = json!(session);
    frame
}

fn usage_on(session: &str) -> Value {
    on(
        session,
        &json!({"sessionUpdate": "usage_update", "used": 7, "size": 100}),
    )
}

fn invalid(session: &str, error: &str) -> String {
    let warning = crate::event::ProtocolWarning::InvalidUpdate {
        name: Some("subagent_spawned".to_owned()),
        error: error.to_owned(),
    };
    format!("Some({session:?}) warning {warning:?}")
}

fn shapes_of(steps: &[Vec<AcpEvent>]) -> Vec<Vec<String>> {
    steps.iter().map(|events| shapes(events)).collect()
}

/// Two roots on one connection each keep their own helpers; a helper announced again under the
/// other root warns and stays where it was first announced, with its frames.
#[test]
fn each_root_keeps_its_own_helpers() {
    let got = run(&[
        Step::New("root-a"),
        Step::New("root-b"),
        Step::Send(vec![
            spawned("root-a", "helper-a", "Lease"),
            spawned("root-b", "helper-b", "Order"),
        ]),
        Step::Send(vec![spawned("root-b", "helper-a", "Lease")]),
        Step::Send(vec![options_on("helper-a"), options_on("helper-b")]),
    ]);
    let none: Vec<String> = Vec::new();
    assert_eq!(
        shapes_of(&got),
        [
            none.clone(),
            none,
            vec![
                "root-a spawned helper-a by None: \"Lease\" None".to_owned(),
                "root-b spawned helper-b by None: \"Order\" None".to_owned(),
            ],
            vec![invalid(
                "root-b",
                "a subagentSessionId already announced under another session"
            )],
            vec![
                "root-a helper helper-a: options of 1".to_owned(),
                "root-b helper helper-b: options of 1".to_owned(),
            ],
        ]
    );
}

/// A spawn on a session this connection never started is refused, so nothing is filed for it:
/// the helper's frames stay an unknown session's.
#[test]
fn a_spawn_on_a_session_not_started_here_is_refused() {
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![spawned("nobody", "helper-a", "Lease")]),
        Step::Send(vec![options_on("helper-a")]),
    ]);
    assert_eq!(
        shapes_of(&got).get(1..),
        Some(
            &[
                vec![invalid(
                    "nobody",
                    "a subagent_spawned on a session this connection did not start"
                )],
                vec!["helper-a options of 1".to_owned()],
            ][..]
        )
    );
}

/// A session loaded under an id once announced as a helper is its own: its replay comes as its
/// updates, in its replay, not as the old root's helper.
#[test]
fn a_session_started_under_a_helpers_id_is_its_own() {
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![spawned("root-a", "helper-a", "Lease")]),
        Step::Load {
            id: "helper-a",
            replay: vec![options_on("helper-a")],
            fails: false,
        },
    ]);
    let replay = got.last().unwrap();
    assert_eq!(shapes(replay), ["helper-a options of 1"]);
    assert_eq!(origin(&replay[0]), Some(Origin::Replay));
}

/// A reload announces the root's helpers again, as `Replay`; each is filed again and its frames
/// still find the root.
#[test]
fn a_reload_announces_its_helpers_again() {
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![spawned("root-a", "helper-a", "Lease")]),
        Step::Load {
            id: "root-a",
            replay: vec![
                spawned("root-a", "helper-a", "Lease"),
                options_on("helper-a"),
                ended("root-a", "helper-a", "disconnected"),
            ],
            fails: false,
        },
        Step::Send(vec![usage_on("helper-a")]),
    ]);
    let shapes = shapes_of(&got);
    assert_eq!(
        shapes.get(2..),
        Some(
            &[
                vec![
                    "root-a spawned helper-a by None: \"Lease\" None".to_owned(),
                    "root-a helper helper-a: options of 1".to_owned(),
                    "root-a ended helper-a: Disconnected".to_owned(),
                ],
                vec!["root-a helper helper-a: usage 7".to_owned()],
            ][..]
        )
    );
    let replay = got.get(2).unwrap();
    assert!(
        replay
            .iter()
            .all(|event| origin(event) == Some(Origin::Replay))
    );
}

/// A helper's frames after its end still come as its own, on the root.
#[test]
fn frames_after_a_helpers_end_still_find_the_root() {
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![
            spawned("root-a", "helper-a", "Lease"),
            ended("root-a", "helper-a", "completed"),
            usage_on("helper-a"),
        ]),
    ]);
    assert_eq!(
        shapes_of(&got).last().unwrap(),
        &[
            "root-a spawned helper-a by None: \"Lease\" None",
            "root-a ended helper-a: Completed",
            "root-a helper helper-a: usage 7",
        ]
    );
}

/// A failed load drops the session and the helpers filed under it: a helper's frames are then
/// an unknown session's.
#[test]
fn a_failed_load_drops_its_helpers() {
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![spawned("root-a", "helper-a", "Lease")]),
        Step::Load {
            id: "root-a",
            replay: Vec::new(),
            fails: true,
        },
        Step::Send(vec![options_on("helper-a")]),
    ]);
    let last = got.last().unwrap();
    assert_eq!(shapes(last), ["helper-a options of 1"]);
    assert_eq!(origin(&last[0]), Some(Origin::Idle));
}

/// A line that opens with `start`, pads a string with `A` to just over the line cap, and closes
/// with `end`.
fn oversize(start: &str, end: &str) -> Vec<u8> {
    let mut line = start.as_bytes().to_vec();
    line.resize(MAX_LINE_BYTES + 1 - end.len(), b'A');
    line.extend_from_slice(end.as_bytes());
    line.push(b'\n');
    line
}

/// A helper's `tool_call_update` over the line cap: with its status shown first it comes as that
/// status, the helper's, on the root; without, as `UpdateSkipped` of the root.
#[test]
fn a_helpers_skipped_update_is_the_roots() {
    let start = |status: &str| {
        let head =
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"helper-a","#;
        let update = r#""update":{"sessionUpdate":"tool_call_update","toolCallId":"exec-1","#;
        let content = r#""content":[{"type":"content","content":{"type":"text","text":""#;
        format!("{head}{update}{status}{content}")
    };
    let end = r#""}}]}}}"#;
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![spawned("root-a", "helper-a", "Lease")]),
        Step::Raw(oversize(&start(r#""status":"failed","#), end)),
        Step::Raw(oversize(&start(""), end)),
    ]);
    assert_eq!(
        shapes(got.get(2).unwrap()),
        ["root-a helper helper-a: tool update exec-1"]
    );
    assert_eq!(
        got.get(3).unwrap().as_slice(),
        [AcpEvent::UpdateSkipped {
            session: Some(id("root-a")),
            origin: Origin::Idle,
            acp_tool_call_id: Some(clauseo_protocol::CompactString::from("exec-1")),
        }]
    );
}
