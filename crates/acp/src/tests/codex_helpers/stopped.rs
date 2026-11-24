//! A helper after its end (`cancelled`, `failed`, `completed`): what it sends afterwards, in
//! the turn it ended in and in the root's next turn, and a running helper's elicitation.
//! Recorded codex-acp sends nothing on a helper's session after its end, so these frames are
//! made up, in the shapes of x8's permission request and of codex-acp's elicitation
//! (`dist/index.js:42102-42115`: `sessionId` is the asking thread's id, a helper's for a
//! helper).

use super::sessions::{ended, spawned, usage_on, warned};
use super::*;

const ROOT: &str = "root-a";
const HELPER: &str = "helper-a";

/// A permission request in x8's shape, with JSON-RPC id `id`, sent on `session`.
fn permission(session: &str, id: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "session/request_permission", "params": {
    "sessionId": session,
    "toolCall": {"toolCallId": "exec-1", "title": "curl -sI https://example.com",
        "rawInput": {"command": "curl -sI https://example.com"}},
    "options": [
        {"optionId": "allow_once", "name": "Yes, proceed", "kind": "allow_once"},
        {"optionId": "cancel", "name": "No", "kind": "reject_once"},
    ]}})
}

/// A form elicitation as codex-acp builds one, with JSON-RPC id `id`, sent on `session`.
fn elicitation(session: &str, id: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "elicitation/create", "params": {
        "mode": "form", "sessionId": session, "message": "Codex needs your input to continue.",
        "requestedSchema": {"type": "object", "properties": {"answer": {"type": "string"}}}}})
}

fn is_request(frame: &Value) -> bool {
    frame.get("id").is_some() && frame.get("method").is_some()
}

/// Reads the client's `session/prompt` and its barrier request, which it sends at the same
/// time, in either order.
async fn prompt_and_barrier(agent: &mut Agent) -> (Value, Value) {
    let (first, second) = (agent.read().await, agent.read().await);
    if first["method"] == "session/prompt" {
        (first, second)
    } else {
        (second, first)
    }
}

/// Sends `frames`, then reads until it holds the client's barrier request (unless `barrier`
/// has it already) and the client's answer to each request among `frames`; gives the barrier
/// and the answers, in order.
async fn send_and_read(
    agent: &mut Agent,
    frames: &[Value],
    mut barrier: Option<Value>,
) -> (Value, Vec<Value>) {
    for frame in frames {
        agent.send(frame);
    }
    let asked = frames.iter().filter(|frame| is_request(frame)).count();
    let mut answers = Vec::new();
    while barrier.is_none() || answers.len() < asked {
        let line = agent.read().await;
        if line.get("method").is_some() {
            assert!(barrier.is_none(), "{line}");
            barrier = Some(line);
        } else {
            answers.push(line);
        }
    }
    (barrier.unwrap(), answers)
}

/// What [`stop_then`] saw.
struct Played {
    /// The events of turn 1 up to the helper's end, from its end, and of turn 2.
    steps: [Vec<AcpEvent>; 3],
    /// The client's answers to the agent's requests, in order.
    answers: Vec<Value>,
}

/// Turn 1 on [`ROOT`]: the agent announces [`HELPER`]; with `cancel` the client cancels; the
/// agent ends the helper `state`, sends `late`, and answers the prompt. Turn 2: the agent sends
/// `next`. A barrier request (`codex_hooks_list`) closes each step, since its answer comes
/// after every frame the agent sent before it.
fn stop_then(cancel: bool, state: &str, late: &[Value], next: &[Value]) -> Played {
    let (client, events, driver, mut agent) = pipes_with(options(true));
    let seen = &events;
    let root = id(ROOT);
    let calls = async move {
        start(&client).await;
        let cwd = folder();
        let barrier = || client.codex_hooks_list(&cwd);
        let first = async {
            barrier().await.unwrap();
            let before = drain(seen);
            if cancel {
                client.cancel(&root).await.unwrap();
            }
            barrier().await.unwrap();
            (before, drain(seen))
        };
        let (end, (before, after)) =
            futures::join!(client.prompt(&root, turn(1), Vec::new()), first);
        end.unwrap();
        let second = async {
            barrier().await.unwrap();
            drain(seen)
        };
        let (end, mut next) = futures::join!(client.prompt(&root, turn(2), Vec::new()), second);
        end.unwrap();
        next.extend(drain(seen));
        [before, after, next]
    };
    let script = async {
        answer_start(&mut agent, ROOT).await;
        let mut answers = Vec::new();
        let (prompt, barrier) = prompt_and_barrier(&mut agent).await;
        agent.send(&spawned(ROOT, HELPER, "Lease"));
        agent.respond(&barrier, &json!({}));
        if cancel {
            assert_eq!(agent.read().await["method"], "session/cancel");
        }
        agent.send(&ended(ROOT, HELPER, state));
        let (barrier, got) = send_and_read(&mut agent, late, None).await;
        answers.extend(got);
        agent.respond(&barrier, &json!({}));
        let stop = if cancel { "cancelled" } else { "end_turn" };
        agent.respond(&prompt, &json!({"stopReason": stop}));
        let (prompt, barrier) = prompt_and_barrier(&mut agent).await;
        let (barrier, got) = send_and_read(&mut agent, next, Some(barrier)).await;
        answers.extend(got);
        agent.respond(&barrier, &json!({}));
        agent.respond(&prompt, &json!({"stopReason": "end_turn"}));
        agent.read_end().await;
        answers
    };
    let (driver, steps, answers) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    Played { steps, answers }
}

/// Each event's shape with its origin.
fn with_origins(events: &[AcpEvent]) -> Vec<(String, Option<Origin>)> {
    events
        .iter()
        .map(|event| (shape(event), origin(event)))
        .collect()
}

/// What an ended helper's refused request warns.
fn refused(method: &str) -> (String, Option<Origin>) {
    let error = "a request of a helper that has ended, answered cancelled";
    (warned(HELPER, method, error), None)
}

/// No request reached the host.
fn asked_nothing(played: &Played) {
    let requests = played.steps.iter().flatten().filter(|event| {
        matches!(
            event,
            AcpEvent::PermissionRequested { .. } | AcpEvent::ElicitationRequested { .. }
        )
    });
    assert_eq!(requests.count(), 0);
}

/// After the root's cancel ends a helper `cancelled`, its permission request in the same turn
/// is answered `cancelled` by the driver and reaches the host only as a warning; its update is
/// late, as the root's are.
#[test]
fn a_cancelled_helpers_permission_request_is_answered_cancelled() {
    let late = [usage_on(HELPER), permission(HELPER, "ask-1")];
    let played = stop_then(true, "cancelled", &late, &[]);
    let late_1 = Some(Origin::Late(turn(1)));
    assert_eq!(
        with_origins(&played.steps[1]),
        [
            ("root-a ended helper-a: Cancelled".to_owned(), late_1),
            ("root-a helper helper-a: usage 7".to_owned(), late_1),
            refused("session/request_permission"),
        ]
    );
    let [answer] = played.answers.as_slice() else {
        panic!("expected one answer, got {:?}", played.answers);
    };
    assert_eq!(
        answer,
        &json!({"jsonrpc": "2.0", "id": "ask-1",
            "result": {"outcome": {"outcome": "cancelled"}}})
    );
    asked_nothing(&played);
}

/// A cancelled helper that talks into the root's next turn: its update stays late with the
/// cancelled turn (the host drops it) while the root's own update is the new turn's; its
/// permission request is answered `cancelled` and its elicitation `cancel`, each a warning.
#[test]
fn a_cancelled_helper_in_the_roots_next_turn_is_late_and_asks_nothing() {
    let next = [
        usage_on(HELPER),
        usage_on(ROOT),
        permission(HELPER, "ask-1"),
        elicitation(HELPER, "ask-2"),
    ];
    let played = stop_then(true, "cancelled", &[], &next);
    assert_eq!(
        with_origins(&played.steps[2]),
        [
            (
                "root-a helper helper-a: usage 7".to_owned(),
                Some(Origin::Late(turn(1)))
            ),
            ("root-a usage 7".to_owned(), Some(Origin::Turn(turn(2)))),
            refused("session/request_permission"),
            refused("elicitation/create"),
        ]
    );
    let answers: Vec<_> = played
        .answers
        .iter()
        .map(|answer| &answer["result"])
        .collect();
    assert_eq!(
        answers,
        [
            &json!({"outcome": {"outcome": "cancelled"}}),
            &json!({"action": "cancel"}),
        ]
    );
    asked_nothing(&played);
}

/// A helper ended `failed` while the root's turn still runs (codex-acp's wait for its helpers
/// timed out) is stopped as a cancelled one: in the next turn its update is late with turn 1
/// and its request is answered `cancelled`.
#[test]
fn a_failed_helper_is_stopped_as_a_cancelled_one() {
    let next = [usage_on(HELPER), permission(HELPER, "ask-1")];
    let played = stop_then(false, "failed", &[], &next);
    assert_eq!(
        with_origins(&played.steps[1]),
        [(
            "root-a ended helper-a: Failed".to_owned(),
            Some(Origin::Turn(turn(1)))
        )]
    );
    assert_eq!(
        with_origins(&played.steps[2]),
        [
            (
                "root-a helper helper-a: usage 7".to_owned(),
                Some(Origin::Late(turn(1)))
            ),
            refused("session/request_permission"),
        ]
    );
    assert_eq!(
        played.answers[0]["result"],
        json!({"outcome": {"outcome": "cancelled"}})
    );
    asked_nothing(&played);
}

/// A completed helper is ended as a cancelled one: in the next turn its update is late with
/// the turn it completed in, and its permission request and elicitation are answered here.
#[test]
fn a_completed_helper_is_ended_as_a_cancelled_one() {
    let next = [
        usage_on(HELPER),
        permission(HELPER, "ask-1"),
        elicitation(HELPER, "ask-2"),
    ];
    let played = stop_then(false, "completed", &[], &next);
    assert_eq!(
        with_origins(&played.steps[2]),
        [
            (
                "root-a helper helper-a: usage 7".to_owned(),
                Some(Origin::Late(turn(1)))
            ),
            refused("session/request_permission"),
            refused("elicitation/create"),
        ]
    );
    let answers: Vec<_> = played
        .answers
        .iter()
        .map(|answer| &answer["result"])
        .collect();
    assert_eq!(
        answers,
        [
            &json!({"outcome": {"outcome": "cancelled"}}),
            &json!({"action": "cancel"}),
        ]
    );
    asked_nothing(&played);
}

/// A running helper's elicitation (codex-acp sends it under the helper's session id) comes as
/// the root's, in the root's turn, and its answer goes back to the agent by the request's id.
#[test]
fn a_helpers_elicitation_is_the_roots() {
    let (client, events, driver, mut agent) = pipes_with(options(true));
    let seen = &events;
    let root = id(ROOT);
    let calls = async move {
        start(&client).await;
        let answer = async {
            loop {
                let event = seen.recv().await.unwrap();
                if let AcpEvent::ElicitationRequested { key, .. } = &event {
                    let decline = clauseo_protocol::ElicitationResponse::Decline;
                    client.answer_elicitation(*key, decline).await.unwrap();
                    return event;
                }
            }
        };
        let (end, request) = futures::join!(client.prompt(&root, turn(1), Vec::new()), answer);
        end.unwrap();
        request
    };
    let script = async {
        answer_start(&mut agent, ROOT).await;
        let prompt = agent.read().await;
        agent.send(&spawned(ROOT, HELPER, "Lease"));
        agent.send(&elicitation(HELPER, "ask-1"));
        assert_eq!(
            agent.read().await,
            json!({"jsonrpc": "2.0", "id": "ask-1", "result": {"action": "decline"}})
        );
        agent.respond(&prompt, &json!({"stopReason": "end_turn"}));
        agent.read_end().await;
    };
    let (driver, request, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let AcpEvent::ElicitationRequested {
        session, origin, ..
    } = request
    else {
        panic!("expected an elicitation, got {request:?}");
    };
    assert_eq!(session, id(ROOT));
    assert_eq!(origin, Origin::Turn(turn(1)));
}
