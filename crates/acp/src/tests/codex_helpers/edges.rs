//! Helper sessions at their edges: the switch off, a helper never announced, the root's own
//! state, a helper's failure, malformed `subagent_*` updates, and which chunks are a report.

use super::*;
use crate::event::ProtocolWarning;

/// x1's frames without those `skip` picks out.
fn x1_without(skip: impl Fn(&Value) -> bool) -> Vec<Value> {
    frames(X1)
        .into_iter()
        .filter(|frame| !skip(frame))
        .collect()
}

fn update_name(frame: &Value) -> &str {
    frame["params"]["update"]["sessionUpdate"]
        .as_str()
        .unwrap_or_default()
}

/// `update` sent on `session`.
fn on(session: &str, update: &Value) -> Value {
    json!({"jsonrpc": "2.0", "method": "session/update",
        "params": {"sessionId": session, "update": update}})
}

const HELPER: &str = "01a20c29-d483-7382-9b0e-1629bbf4cee4";

/// A second helper of x1's root, made up.
const OTHER: &str = "01a20c29-e000-7382-9b0e-1629bbf4cee4";

/// `initialize` asks for helper sessions only with the switch on: `nativeSubagentSessions`
/// joins the AIR list; off, the list is as it was.
#[test]
fn initialize_asks_for_helper_sessions_only_when_on() {
    for (on, list) in [
        (false, json!(["sessionFailure", "recommendedValue"])),
        (
            true,
            json!([
                "sessionFailure",
                "recommendedValue",
                "nativeSubagentSessions"
            ]),
        ),
    ] {
        let (client, _events, driver, mut agent) = pipes_with(options(on));
        let calls = async move {
            client.initialize().await.unwrap();
        };
        let script = async {
            let init = agent.read().await;
            let air = &init["params"]["clientCapabilities"]["_meta"]["jetbrains"]["air"];
            assert_eq!(air["capabilities"], list, "on: {on}");
            agent.respond(&init, &json!({"protocolVersion": 1}));
            agent.read_end().await;
        };
        let (driver, (), ()) =
            futures::executor::block_on(async { futures::join!(driver, calls, script) });
        assert_eq!(driver, Ok(()));
    }
}

/// The switch off, the very same frames (x1) map as before helper sessions existed: each
/// `subagent_*` update is an unknown update, and the helper's frames are `Update`s of a session
/// the connection does not know, so `Idle`.
#[test]
fn with_the_switch_off_the_same_frames_map_as_before() {
    let events = play(false, X1_ROOT, &frames(X1));
    let unknown = |name: &str| {
        let warning = ProtocolWarning::UnknownUpdate {
            name: name.to_owned(),
        };
        format!("Some(\"bdfa\") warning {warning:?}")
    };
    assert_eq!(
        shapes(&events),
        [
            "bdfa Answer \".\"".to_owned(),
            unknown("subagent_spawned"),
            "d483 options of 1".to_owned(),
            "bdfa usage 12071".to_owned(),
            "bdfa tool call call_3a599374996943828a280bc01e0e8cb4".to_owned(),
            "d483 Answer \"I\"".to_owned(),
            "d483 Answer \"’ll\"".to_owned(),
            "d483 Answer \".\"".to_owned(),
            "d483 tool call exec-7fd37d45-aacb-4501-8bc2-96ebd303db13".to_owned(),
            "d483 tool update exec-7fd37d45-aacb-4501-8bc2-96ebd303db13".to_owned(),
            "d483 tool call exec-ceaf303f-ba35-46ac-9089-b0127f63760b".to_owned(),
            "d483 tool update exec-ceaf303f-ba35-46ac-9089-b0127f63760b".to_owned(),
            "d483 usage 12869".to_owned(),
            "d483 Answer \"Lease\"".to_owned(),
            "d483 Answer \" start\"".to_owned(),
            "d483 Answer \"5\"".to_owned(),
            "d483 usage 14406".to_owned(),
            unknown("subagent_state_update"),
            "bdfa tool update call_3a599374996943828a280bc01e0e8cb4".to_owned(),
            "bdfa Answer \"Lease\"".to_owned(),
        ]
    );
    for event in &events {
        if let AcpEvent::Update {
            session, origin, ..
        } = event
        {
            let idle = session.as_str() == HELPER;
            let expected = if idle {
                Origin::Idle
            } else {
                Origin::Turn(turn(1))
            };
            assert_eq!(*origin, expected, "{event:?}");
        }
    }
}

/// Switch on, a helper whose `subagent_spawned` never came: its frames map as any unknown
/// session's do, as before; its end only warns, so no card could end that was never shown.
#[test]
fn a_helper_never_announced_maps_as_an_unknown_session_and_its_end_warns() {
    let events = play(
        true,
        X1_ROOT,
        &x1_without(|f| update_name(f) == "subagent_spawned"),
    );
    let shapes = shapes(&events);
    assert_eq!(shapes.get(1).map(String::as_str), Some("d483 options of 1"));
    let warning = ProtocolWarning::InvalidUpdate {
        name: Some("subagent_state_update".to_owned()),
        error: "a subagentSessionId not announced under this session".to_owned(),
    };
    assert_eq!(
        shapes.get(16),
        Some(&format!("Some(\"bdfa\") warning {warning:?}"))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AcpEvent::HelperEnded { .. }))
    );
    let helper = events.iter().filter(
        |event| matches!(event, AcpEvent::Update { session, .. } if session.as_str() == HELPER),
    );
    assert_eq!(helper.clone().count(), 13);
    assert!(
        helper
            .map(origin)
            .all(|origin| origin == Some(Origin::Idle))
    );
}

/// The root's own events (x1) are the same with the helper's frames in between (its options,
/// usage, tool calls, text, status) as without them: the helper's frames change nothing the
/// driver keeps for the root.
#[test]
fn the_roots_events_are_the_same_without_its_helpers_frames() {
    let with = play(true, X1_ROOT, &frames(X1));
    let roots: Vec<AcpEvent> = with
        .into_iter()
        .filter(|event| matches!(event, AcpEvent::Update { .. }))
        .collect();
    let alone = x1_without(|frame| {
        frame["params"]["sessionId"] != X1_ROOT || update_name(frame).starts_with("subagent_")
    });
    assert_eq!(alone.len(), 5);
    assert_eq!(roots, play(true, X1_ROOT, &alone));
}

fn items(plan: &str, count: usize) -> Value {
    let entries = vec![
        json!({"content": "Read the lease", "priority": "medium", "status": "pending"});
        count
    ];
    json!({"sessionUpdate": "plan_update",
        "plan": {"type": "items", "planId": plan, "entries": entries}})
}

fn removed(plan: &str) -> Value {
    json!({"sessionUpdate": "plan_removed", "planId": plan})
}

/// A helper's plans leave the root's shown plan alone: the helper's plan comes as its own, its
/// removal of the root's plan gives nothing, and the root's removal of the helper's plan gives
/// nothing either; only the root removing its own shown plan empties it.
#[test]
fn a_helpers_plan_leaves_the_roots_shown_plan() {
    let spawned = frames(X1).swap_remove(1);
    let events = play(
        true,
        X1_ROOT,
        &[
            spawned,
            on(X1_ROOT, &items("p-root", 1)),
            on(HELPER, &items("p-helper", 2)),
            on(HELPER, &removed("p-root")),
            on(X1_ROOT, &removed("p-helper")),
            on(X1_ROOT, &removed("p-root")),
        ],
    );
    assert_eq!(
        shapes(&events),
        [
            "bdfa spawned d483 by None: \"Read documents\" None",
            "bdfa plan of 1",
            "bdfa helper d483: plan of 2",
            "bdfa plan of 0",
        ]
    );
}

/// An AIR `sessionFailure` that stops a turn for a plan limit: on the root it is a `Limit`; on
/// a helper's session a warning naming the helper's session, so it never stops the root's turn.
#[test]
fn a_helpers_failure_is_a_warning_not_the_roots_limit() {
    let failure = json!({"sessionUpdate": "session_info_update",
    "_meta": {"jetbrains": {"air": {"version": 1, "sessionFailure": {
        "id": "f", "revision": 1, "category": "limit", "severity": "error",
        "title": "Usage limit reached.", "actions": [],
    }}}}});
    let spawned = frames(X1).swap_remove(1);
    let events = play(
        true,
        X1_ROOT,
        &[spawned, on(HELPER, &failure), on(X1_ROOT, &failure)],
    );
    let [_, helper, root] = events.as_slice() else {
        panic!("expected three events, got {events:?}");
    };
    assert_eq!(
        helper,
        &AcpEvent::Warning {
            session: Some(id(HELPER)),
            warning: ProtocolWarning::SessionFailure {
                category: Some("limit".to_owned()),
                severity: Some("error".to_owned()),
                title: Some("Usage limit reached.".to_owned()),
            },
        }
    );
    assert!(
        matches!(root, AcpEvent::Limit { session, .. } if session.as_str() == X1_ROOT),
        "{root:?}"
    );
}

/// `subagent_*` updates that cannot be read warn and announce nothing; a real task is kept; an
/// unknown or missing state only warns and leaves the helper running, so a known state still
/// ends it.
#[test]
fn malformed_subagent_updates_warn() {
    let spawned = |fields: Value| {
        let mut update = json!({"sessionUpdate": "subagent_spawned", "capabilities": {}});
        update
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        on(X1_ROOT, &update)
    };
    let ended = |state: Value| {
        on(
            X1_ROOT,
            &json!({"sessionUpdate": "subagent_state_update", "subagentSessionId": HELPER,
                "state": state}),
        )
    };
    let events = play(
        true,
        X1_ROOT,
        &[
            spawned(json!({"name": "Lease", "task": "Delegated task for Lease"})),
            spawned(json!({"subagentSessionId": X1_ROOT, "name": "Lease"})),
            spawned(json!({"subagentSessionId": HELPER})),
            spawned(json!({"subagentSessionId": HELPER, "name": "Lease", "task": ""})),
            spawned(json!({"subagentSessionId": OTHER, "name": "Lease",
                "task": "Delegated task for Order"})),
            ended(json!("paused")),
            ended(Value::Null),
            ended(json!("failed")),
        ],
    );
    let invalid = |name: &str, error: &str| {
        let warning = ProtocolWarning::InvalidUpdate {
            name: Some(name.to_owned()),
            error: error.to_owned(),
        };
        format!("Some(\"bdfa\") warning {warning:?}")
    };
    let spawn = "subagent_spawned";
    let state = "subagent_state_update";
    assert_eq!(
        shapes(&events),
        [
            invalid(spawn, "no subagentSessionId string"),
            invalid(
                spawn,
                "a subagentSessionId that names a session of this connection"
            ),
            invalid(spawn, "no name string"),
            "bdfa spawned d483 by None: \"Lease\" None".to_owned(),
            "bdfa spawned e000 by None: \"Lease\" Some(\"Delegated task for Order\")".to_owned(),
            invalid(state, "unknown state \"paused\""),
            invalid(state, "no state string"),
            "bdfa ended d483: Failed".to_owned(),
        ]
    );
}

/// Only a helper's `agent_message_chunk` marked `final_answer` is its report: a `commentary`
/// chunk, an unmarked one, and a thought chunk marked `final_answer` stay `HelperUpdate`s.
#[test]
fn only_a_final_answer_chunk_is_the_report() {
    let x1 = frames(X1);
    let (spawned, commentary, answer) = (x1[1].clone(), x1[7].clone(), x1[15].clone());
    let mut unmarked = answer.clone();
    unmarked["params"]["update"]
        .as_object_mut()
        .unwrap()
        .remove("_meta");
    let mut thought = answer.clone();
    thought["params"]["update"]["sessionUpdate"] = json!("agent_thought_chunk");
    let events = play(
        true,
        X1_ROOT,
        &[spawned, commentary, answer, unmarked, thought],
    );
    assert_eq!(
        shapes(&events).get(1..),
        Some(
            &[
                "bdfa helper d483: Answer \"I\"".to_owned(),
                "bdfa answer d483: \"Lease\"".to_owned(),
                "bdfa helper d483: Answer \"Lease\"".to_owned(),
                "bdfa helper d483: Thought \"Lease\"".to_owned(),
            ][..]
        )
    );
}
