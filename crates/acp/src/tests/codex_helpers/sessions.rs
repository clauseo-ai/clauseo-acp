//! Helper sessions across the connection's sessions: two roots, a helper announced again under
//! the same root or another, the cap on helpers per root, a spawn on a session this connection
//! never started, ends that are refused, a session started under a helper's id, frames after a
//! helper's end, a failed load, and a skipped update. Each step's events are taken once the step
//! is over: a request's answer comes after every frame the agent sent before it.

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
pub(super) fn on(session: &str, update: &Value) -> Value {
    json!({"jsonrpc": "2.0", "method": "session/update",
        "params": {"sessionId": session, "update": update}})
}

pub(super) fn spawned(session: &str, helper: &str, name: &str) -> Value {
    on(
        session,
        &json!({"sessionUpdate": "subagent_spawned", "subagentSessionId": helper, "name": name,
            "task": format!("Delegated task for {name}"), "capabilities": {}}),
    )
}

pub(super) fn ended(session: &str, helper: &str, state: &str) -> Value {
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

pub(super) fn usage_on(session: &str) -> Value {
    on(
        session,
        &json!({"sessionUpdate": "usage_update", "used": 7, "size": 100}),
    )
}

/// The shape of an `InvalidUpdate` warning about update `name` on `session`.
pub(super) fn warned(session: &str, name: &str, error: &str) -> String {
    let warning = crate::event::ProtocolWarning::InvalidUpdate {
        name: Some(name.to_owned()),
        error: error.to_owned(),
    };
    format!("Some({session:?}) warning {warning:?}")
}

fn invalid(session: &str, error: &str) -> String {
    warned(session, "subagent_spawned", error)
}

fn invalid_end(session: &str, error: &str) -> String {
    warned(session, "subagent_state_update", error)
}

/// What a helper's update `name` on `session` warns after the helper ended outside a turn.
fn ended_update(session: &str, name: &str) -> String {
    warned(
        session,
        name,
        "an update of a helper that ended outside a turn",
    )
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
/// still find the root. Once the replay ends it `disconnected`, outside any turn, a frame of it
/// is dropped with a warning.
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
                vec![ended_update("helper-a", "usage_update")],
            ][..]
        )
    );
    let replay = &got[2];
    assert!(
        replay
            .iter()
            .all(|event| origin(event) == Some(Origin::Replay))
    );
}

/// A helper's frame after it completed outside a turn is dropped with a warning: it belongs to
/// no turn of the root's.
#[test]
fn a_frame_after_a_helpers_end_outside_a_turn_is_dropped() {
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
            "root-a spawned helper-a by None: \"Lease\" None".to_owned(),
            "root-a ended helper-a: Completed".to_owned(),
            ended_update("helper-a", "usage_update"),
        ]
    );
}

/// An end for a helper never announced only warns: no `HelperEnded`, and the helper's frames
/// stay an unknown session's.
#[test]
fn an_end_of_a_helper_never_announced_warns() {
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![ended("root-a", "helper-a", "completed")]),
        Step::Send(vec![options_on("helper-a")]),
    ]);
    let error = "a subagentSessionId not announced under this session";
    assert_eq!(
        shapes_of(&got).get(1..),
        Some(
            &[
                vec![invalid_end("root-a", error)],
                vec!["helper-a options of 1".to_owned()],
            ][..]
        )
    );
}

/// An end of another root's helper, sent on the other root or on a session this connection
/// never started, only warns; the helper still runs under its own root, which can end it.
#[test]
fn an_end_from_another_root_warns() {
    let got = run(&[
        Step::New("root-a"),
        Step::New("root-b"),
        Step::Send(vec![spawned("root-a", "helper-a", "Lease")]),
        Step::Send(vec![
            ended("root-b", "helper-a", "completed"),
            ended("nobody", "helper-a", "cancelled"),
        ]),
        Step::Send(vec![
            usage_on("helper-a"),
            ended("root-a", "helper-a", "completed"),
        ]),
    ]);
    let error = "a subagentSessionId not announced under this session";
    assert_eq!(
        shapes_of(&got).get(3..),
        Some(
            &[
                vec![invalid_end("root-b", error), invalid_end("nobody", error)],
                vec![
                    "root-a helper helper-a: usage 7".to_owned(),
                    "root-a ended helper-a: Completed".to_owned(),
                ],
            ][..]
        )
    );
}

/// A second end of one helper only warns, whatever state it names: the first end stands.
#[test]
fn a_second_end_warns() {
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![spawned("root-a", "helper-a", "Lease")]),
        Step::Send(vec![ended("root-a", "helper-a", "completed")]),
        Step::Send(vec![
            ended("root-a", "helper-a", "completed"),
            ended("root-a", "helper-a", "cancelled"),
        ]),
        Step::Send(vec![usage_on("helper-a")]),
    ]);
    let error = "a subagentSessionId that already ended";
    assert_eq!(
        shapes_of(&got).get(2..),
        Some(
            &[
                vec!["root-a ended helper-a: Completed".to_owned()],
                vec![invalid_end("root-a", error), invalid_end("root-a", error)],
                // The first end stands: the helper's frame is an ended helper's.
                vec![ended_update("helper-a", "usage_update")],
            ][..]
        )
    );
}

/// A helper announced again under the same root, on the root or on another of its helpers'
/// sessions (which would make that helper its parent), only warns: one helper, one
/// `HelperSpawned`, its first parent kept.
#[test]
fn a_helper_announced_again_under_the_same_root_warns() {
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![
            spawned("root-a", "helper-a", "Lease"),
            spawned("helper-a", "helper-b", "Order"),
        ]),
        Step::Send(vec![
            spawned("root-a", "helper-a", "Lease"),
            spawned("helper-b", "helper-a", "Lease"),
            spawned("root-a", "helper-b", "Order"),
        ]),
        Step::Send(vec![options_on("helper-a")]),
    ]);
    let error = "a subagentSessionId already announced";
    assert_eq!(
        shapes_of(&got).get(1..),
        Some(
            &[
                vec![
                    "root-a spawned helper-a by None: \"Lease\" None".to_owned(),
                    "root-a spawned helper-b by Some(\"helper-a\"): \"Order\" None".to_owned(),
                ],
                vec![
                    invalid("root-a", error),
                    invalid("helper-b", error),
                    invalid("root-a", error),
                ],
                vec!["root-a helper helper-a: options of 1".to_owned()],
            ][..]
        )
    );
}

/// A helper that has ended announces no helper of its own: the announcement only warns, and
/// the would-be helper's frames stay an unknown session's.
#[test]
fn a_helper_that_has_ended_announces_no_helper() {
    let got = run(&[
        Step::New("root-a"),
        Step::Send(vec![
            spawned("root-a", "helper-a", "Relay"),
            ended("root-a", "helper-a", "cancelled"),
        ]),
        Step::Send(vec![spawned("helper-a", "helper-b", "Reader")]),
        Step::Send(vec![options_on("helper-b")]),
    ]);
    assert_eq!(
        shapes_of(&got).get(2..),
        Some(
            &[
                vec![invalid(
                    "helper-a",
                    "a subagent_spawned on a helper that has ended"
                )],
                vec!["helper-b options of 1".to_owned()],
            ][..]
        )
    );
}

/// A root takes at most 1024 helpers: the 1025th is refused and its frames stay an unknown
/// session's, while another root still takes its own.
#[test]
fn a_root_takes_at_most_1024_helpers() {
    let many = |range: std::ops::Range<usize>| {
        let spawn = |n| spawned("root-a", &format!("helper-{n}"), "Lease");
        Step::Send(range.map(spawn).collect())
    };
    let got = run(&[
        Step::New("root-a"),
        Step::New("root-b"),
        many(0..512),
        many(512..1024),
        Step::Send(vec![
            spawned("root-a", "helper-1024", "Lease"),
            spawned("root-b", "helper-b", "Order"),
        ]),
        Step::Send(vec![options_on("helper-1024"), options_on("helper-b")]),
    ]);
    let spawned_events = got[2..4]
        .iter()
        .flatten()
        .filter(|event| matches!(event, AcpEvent::HelperSpawned { .. }))
        .count();
    assert_eq!(spawned_events, 1024);
    let error = "this session has used its 1024 helpers since it was last started";
    assert_eq!(
        shapes_of(&got).get(4..),
        Some(
            &[
                vec![
                    invalid("root-a", error),
                    "root-b spawned helper-b by None: \"Order\" None".to_owned(),
                ],
                vec![
                    "helper-1024 options of 1".to_owned(),
                    "root-b helper helper-b: options of 1".to_owned(),
                ],
            ][..]
        )
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
        shapes(&got[2]),
        ["root-a helper helper-a: tool update exec-1"]
    );
    assert_eq!(
        got[3].as_slice(),
        [AcpEvent::UpdateSkipped {
            session: Some(id("root-a")),
            origin: Origin::Idle,
            acp_tool_call_id: Some(clauseo_protocol::CompactString::from("exec-1")),
        }]
    );
}
