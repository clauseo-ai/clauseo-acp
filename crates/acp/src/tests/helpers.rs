//! A helper's updates: claude-agent-acp stamps each with `_meta.claudeCode.parentToolUseId`
//! (`dist/sdk-to-acp.js:9-17`), and the host gets them as `AcpEvent::HelperUpdate`.

use clauseo_protocol::CompactString;

use super::*;
use crate::event::ProtocolWarning;

/// The `toolCallId` of the lead's `Agent` call in these tests.
const LAUNCH: &str = "toolu_01Launch";

/// Plays the agent sending `updates` after the handshake; returns every event the host got.
fn events_for(updates: &[Value]) -> Vec<AcpEvent> {
    let (client, events, driver, mut agent) = pipes();
    let calls = async move { handshake(&client).await };
    let script = async {
        agent.start().await;
        for update in updates {
            agent.update(&json!({"sessionId": "s-1", "update": update}));
        }
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    drain(&events)
}

/// `update` with `meta` as its `_meta`.
fn with_meta(update: &Value, meta: Value) -> Value {
    let mut update = update.clone();
    update["_meta"] = meta;
    update
}

/// The meta claude-agent-acp writes on a helper's tool call: its tool name and the launch.
fn stamp(parent: &Value) -> Value {
    json!({"claudeCode": {"toolName": "Read", "parentToolUseId": parent}})
}

/// One of each kind the adapter stamps, without a stamp.
fn kinds() -> [Value; 4] {
    [
        json!({"sessionUpdate": "tool_call", "toolCallId": "toolu_02Read", "title": "Read lease.txt",
            "kind": "read", "status": "pending"}),
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "toolu_02Read",
            "status": "completed"}),
        json!({"sessionUpdate": "agent_message_chunk", "messageId": "m-1",
            "content": {"type": "text", "text": "Clause 4 caps the fees."}}),
        json!({"sessionUpdate": "agent_thought_chunk", "messageId": "m-1",
            "content": {"type": "text", "text": "Check clause 4."}}),
    ]
}

/// The update each event carries, and the helper it names (`None` for `AcpEvent::Update`).
fn helpers_and_updates(events: Vec<AcpEvent>) -> Vec<(Option<CompactString>, AgentUpdate)> {
    events
        .into_iter()
        .map(|event| match event {
            AcpEvent::Update { update, .. } => (None, update),
            AcpEvent::HelperUpdate { helper, update, .. } => (Some(helper), update),
            other @ (AcpEvent::HelperInBackground { .. }
            | AcpEvent::PermissionRequested { .. }
            | AcpEvent::ElicitationRequested { .. }
            | AcpEvent::ElicitationCompleted { .. }
            | AcpEvent::Limit { .. }
            | AcpEvent::TooLong { .. }
            | AcpEvent::ConnectionLost { .. }
            | AcpEvent::Warning { .. }
            | AcpEvent::UpdateSkipped { .. }) => panic!("expected an update, got {other:?}"),
        })
        .collect()
}

/// A stamped tool call, tool call update, answer chunk and thought chunk each reach the host as
/// a `HelperUpdate` naming the launch, carrying what the same update without the stamp carries.
#[test]
fn stamped_updates_of_each_kind_name_their_helper() {
    let plain = helpers_and_updates(events_for(&kinds()));
    let stamped: Vec<Value> = kinds()
        .iter()
        .map(|update| with_meta(update, stamp(&json!(LAUNCH))))
        .collect();
    let stamped = helpers_and_updates(events_for(&stamped));
    assert_eq!(plain.len(), 4);
    let launch = Some(CompactString::from(LAUNCH));
    let expected: Vec<_> = plain
        .into_iter()
        .map(|(helper, update)| {
            assert_eq!(helper, None, "{update:?}");
            (launch.clone(), update)
        })
        .collect();
    assert_eq!(stamped, expected);
}

/// The session and origin of a helper's update are the lead's, as for any update.
#[test]
fn a_helper_update_keeps_the_lead_session_and_origin() {
    let [call, ..] = kinds();
    let events = events_for(&[with_meta(&call, stamp(&json!(LAUNCH)))]);
    let [
        AcpEvent::HelperUpdate {
            session, origin, ..
        },
    ] = <[AcpEvent; 1]>::try_from(events).unwrap()
    else {
        panic!("expected one helper update");
    };
    assert_eq!(
        (session, origin),
        (AgentSessionId::new("s-1").unwrap(), Origin::Idle)
    );
}

/// A stamp that is not a non-empty string, a `claudeCode` without one, or a stamp outside
/// `claudeCode` names no helper: the update reaches the host as an `Update`.
#[test]
fn a_stamp_that_is_not_a_non_empty_string_names_no_helper() {
    let [call, ..] = kinds();
    let metas = [
        stamp(&json!("")),
        stamp(&json!(7)),
        stamp(&Value::Null),
        stamp(&json!({"id": LAUNCH})),
        json!({"claudeCode": {"toolName": "Read"}}),
        json!({"parentToolUseId": LAUNCH}),
        json!(LAUNCH),
    ];
    let updates: Vec<Value> = metas
        .into_iter()
        .map(|meta| with_meta(&call, meta))
        .collect();
    let seen = helpers_and_updates(events_for(&updates));
    assert_eq!(seen.len(), updates.len());
    for (helper, update) in seen {
        assert_eq!(helper, None, "{update:?}");
    }
}

/// A stamped update whose body does not decode still gives the warning it gives unstamped.
#[test]
fn a_stamped_update_that_does_not_decode_stays_a_warning() {
    let broken = json!({"sessionUpdate": "tool_call", "toolCallId": "toolu_02Read", "title": 5});
    let plain = events_for(std::slice::from_ref(&broken));
    let stamped = events_for(&[with_meta(&broken, stamp(&json!(LAUNCH)))]);
    assert!(
        matches!(
            plain.as_slice(),
            [AcpEvent::Warning {
                warning: ProtocolWarning::InvalidUpdate { name: Some(name), .. },
                ..
            }] if name == "tool_call"
        ),
        "{plain:?}"
    );
    assert_eq!(stamped, plain);
}

/// The launch's update that says the helper runs in the background, as 0.110.1 sent it.
fn launched_async(is_async: &Value) -> Value {
    json!({"_meta": {"claudeCode": {"toolResponse": {"status": "async_launched", "isAsync": is_async}}},
        "toolCallId": LAUNCH, "sessionUpdate": "tool_call_update"})
}

/// `isAsync: true` on the launch's update gives `HelperInBackground` naming the launch, then
/// the update's own event, unchanged.
#[test]
fn an_async_launch_says_the_helper_runs_in_the_background() {
    let plain = events_for(&[launched_async(&json!(false))]);
    let events = events_for(&[launched_async(&json!(true))]);
    let [first, rest @ ..] = events.as_slice() else {
        panic!("expected events, got {events:?}");
    };
    assert_eq!(
        first,
        &AcpEvent::HelperInBackground {
            session: AgentSessionId::new("s-1").unwrap(),
            origin: Origin::Idle,
            helper: CompactString::from(LAUNCH),
        }
    );
    assert_eq!(rest, plain.as_slice());
}

/// `isAsync` false, missing or not a boolean, or `true` outside a `tool_call_update`, adds
/// nothing.
#[test]
fn no_async_flag_adds_nothing() {
    let mut missing = launched_async(&json!(true));
    missing["_meta"]["claudeCode"]["toolResponse"] = json!({"status": "completed"});
    let mut on_a_call = with_meta(&kinds()[0], json!({}));
    on_a_call["_meta"] = launched_async(&json!(true))["_meta"].clone();
    let updates = [
        launched_async(&json!(false)),
        launched_async(&json!("true")),
        missing,
        on_a_call,
    ];
    let events = events_for(&updates);
    assert_eq!(events.len(), updates.len(), "{events:?}");
    assert!(
        events
            .iter()
            .all(|event| matches!(event, AcpEvent::Update { .. })),
        "{events:?}"
    );
}

/// claude-agent-acp 0.110.1's frames of one helper, recorded on macOS with Clauseo's helper
/// definition (`fixtures/claude-agent-acp-0.110.1-helper.jsonl`, the home folder renamed
/// `/Users/lawyer`): the lead's `Agent` call (its start, title, input, the update with
/// `toolResponse.isAsync`, its completion), the helper's two `Read` steps and their updates, the
/// helper's `SubagentHandback` call and its update, then a chunk of the lead's answer.
const RECORDED: &str = include_str!("fixtures/claude-agent-acp-0.110.1-helper.jsonl");

/// The tool call `update` is about, if any.
fn tool_id(update: &AgentUpdate) -> Option<&str> {
    if let AgentUpdate::ToolCall(call) = update {
        return Some(call.acp_id.as_str());
    }
    if let AgentUpdate::ToolCallUpdate { acp_id, .. } = update {
        return Some(acp_id.as_str());
    }
    None
}

/// What an event is, the helper it names and the tool call it is about, for comparing.
fn shape(event: &AcpEvent) -> (&'static str, Option<&str>, Option<&str>) {
    match event {
        AcpEvent::Update { update, .. } => ("update", None, tool_id(update)),
        AcpEvent::HelperUpdate { helper, update, .. } => {
            ("helper update", Some(helper.as_str()), tool_id(update))
        }
        AcpEvent::HelperInBackground { helper, .. } => {
            ("in background", Some(helper.as_str()), None)
        }
        other @ (AcpEvent::PermissionRequested { .. }
        | AcpEvent::ElicitationRequested { .. }
        | AcpEvent::ElicitationCompleted { .. }
        | AcpEvent::Limit { .. }
        | AcpEvent::TooLong { .. }
        | AcpEvent::ConnectionLost { .. }
        | AcpEvent::Warning { .. }
        | AcpEvent::UpdateSkipped { .. }) => panic!("unexpected {other:?}"),
    }
}

/// The recorded run: only the helper's `tool_call`s carry the stamp, so only they become
/// `HelperUpdate`s; the updates of its steps come unstamped, as `Update`s; the launch's
/// `isAsync` update gives `HelperInBackground` before its own `Update`.
#[test]
fn a_recorded_0_110_1_helper_names_its_steps_and_runs_in_the_background() {
    let (agent_call, read_1, read_2, handback) = (
        "toolu_01Gr3CDaJEng8pyzihUmv3Cq",
        "toolu_01WesqZa8jMTM4WMn2ELQNTp",
        "toolu_01VFDnvL4EbFFj1FnZthBh4g",
        "toolu_014ydnQPzgARsFJViooHGimr",
    );
    let (client, events, driver, mut agent) = pipes();
    let calls = async move { handshake(&client).await };
    let script = async {
        agent.start().await;
        for line in RECORDED.lines() {
            agent.send(&serde_json::from_str::<Value>(line).unwrap());
        }
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
    let seen = drain(&events);
    let shapes: Vec<_> = seen.iter().map(shape).collect();
    let helper = Some(agent_call);
    assert_eq!(
        shapes,
        [
            ("update", None, Some(agent_call)),
            ("update", None, Some(agent_call)),
            ("update", None, Some(agent_call)),
            ("in background", helper, None),
            ("update", None, Some(agent_call)),
            ("update", None, Some(agent_call)),
            ("helper update", helper, Some(read_1)),
            ("helper update", helper, Some(read_2)),
            ("update", None, Some(read_2)),
            ("update", None, Some(read_1)),
            ("helper update", helper, Some(handback)),
            ("update", None, Some(handback)),
            ("update", None, None),
        ]
    );
}
