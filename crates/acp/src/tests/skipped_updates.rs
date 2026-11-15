//! A `session/update` over the line cap, through the driver: the reader drops it, and what its
//! start said reaches the host in its place (`line/oversize.rs`, `inbound.rs`). A
//! `tool_call_update` whose status comes before its content becomes that status alone; one
//! whose status it cannot read becomes `AcpEvent::UpdateSkipped`; another kind gives nothing.

use clauseo_protocol::{CompactString, ToolCallPatch, ToolCallStatus};
use futures::executor::LocalPool;
use futures::task::LocalSpawnExt;

use super::*;
use crate::line::MAX_LINE_BYTES;

/// A line that opens with `start`, pads a string with `A` to just over the cap, and closes
/// with `end`.
fn oversize(start: &str, end: &str) -> Vec<u8> {
    let mut line = start.as_bytes().to_vec();
    line.resize(MAX_LINE_BYTES + 1 - end.len(), b'A');
    line.extend_from_slice(end.as_bytes());
    line.push(b'\n');
    line
}

/// Starts session `s-1`, sends `line` and then one good update, and runs the driver until
/// nothing more can happen. Gives the events, the good update's last.
fn send_after_start(line: Vec<u8>) -> Vec<AcpEvent> {
    let (client, events, driver, mut agent) = pipes();
    let mut pool = LocalPool::new();
    pool.spawner()
        .spawn_local(async move { drop(driver.await) })
        .unwrap();
    pool.run_until(async {
        futures::join!(handshake(&client), agent.start());
    });
    agent.stdout.unbounded_send(Ok(line)).unwrap();
    agent.update(&json!({"sessionId": "s-1", "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "after"},
    }}));
    pool.run_until_stalled();
    let mut received = drain(&events);
    assert!(
        matches!(
            received.pop(),
            Some(AcpEvent::Update {
                update: AgentUpdate::MessageChunk { .. },
                ..
            })
        ),
        "the update after the skipped line"
    );
    drop(client);
    received
}

#[test]
fn a_failed_status_before_the_content_passes_on_alone() {
    // claude-agent-acp's order for a tool result.
    let line = oversize(
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"_meta":{"claudeCode":{"toolName":"Read"}},"toolCallId":"toolu_1","sessionUpdate":"tool_call_update","status":"failed","content":[{"type":"content","content":{"type":"text","text":""#,
        r#""}}]}}}"#,
    );
    let received = send_after_start(line);
    let patch = ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        ..ToolCallPatch::default()
    };
    assert_eq!(
        received,
        [AcpEvent::Update {
            session: session(),
            origin: Origin::Idle,
            update: AgentUpdate::ToolCallUpdate {
                acp_id: CompactString::from("toolu_1"),
                patch,
            },
        }]
    );
}

#[test]
fn a_status_after_the_content_is_a_skip_of_that_tool_call() {
    let line = oversize(
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"t-1","content":[{"type":"content","content":{"type":"text","text":""#,
        r#""}}],"status":"failed"}}}"#,
    );
    assert_eq!(
        send_after_start(line),
        [AcpEvent::UpdateSkipped {
            session: Some(session()),
            origin: Origin::Idle,
            acp_tool_call_id: Some(CompactString::from("t-1")),
        }]
    );
}

#[test]
fn a_start_with_no_tool_call_id_is_a_skip_of_unknown_tool_calls() {
    // The id after a `_meta` longer than the start, or no session.
    for start in [
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"_meta":{"claudeCode":{"toolResponse":""#,
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"t-1","rawOutput":""#,
    ] {
        let line = oversize(start, r#""}}}}}"#);
        let received = send_after_start(line);
        assert!(
            matches!(
                received.as_slice(),
                [AcpEvent::UpdateSkipped {
                    origin: Origin::Idle,
                    acp_tool_call_id: None,
                    ..
                }]
            ),
            "{start}: {received:?}"
        );
    }
}

#[test]
fn another_kind_gives_nothing() {
    let line = oversize(
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":""#,
        r#""}}}}"#,
    );
    assert_eq!(send_after_start(line), []);
}
