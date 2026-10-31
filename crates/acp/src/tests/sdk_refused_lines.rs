//! Lines the line filter passes on as JSON-RPC but the SDK's own decode refuses: a number out
//! of `f64`'s range (`1e400`, which `serde_json` skips when it ignores a value but refuses when it
//! builds one) and `serde_json`'s private `RawValue` token used as a map key, which the SDK refuses
//! or reads as another value. Such a line is a malformed line like any other: it becomes a
//! `ProtocolWarning::MalformedLine`, nothing is written back to the agent, and it never holds
//! back the lines after it.

use futures::executor::LocalPool;
use futures::task::LocalSpawnExt;

use super::*;
use crate::line::MAX_LINES_IN_FLIGHT;

/// A `session/update` whose `used` is out of range for every number type.
const HUGE_NUMBER: &str = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"usage_update","used":1e400,"size":1}}}"#;
/// A `session/update` with `serde_json`'s private `RawValue` token as a key.
const RAW_VALUE_KEY: &str = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"$serde_json::private::RawValue":"agent_message_chunk"}}}"#;
/// A `session/update` whose `params` the SDK's decode reads as the array `[1]`: the `RawValue`
/// token's string holds JSON, so the line is not refused but read differently.
const RAW_VALUE_PARAMS: &str = r#"{"jsonrpc":"2.0","method":"session/update","params":{"$serde_json::private::RawValue":"[1]"}}"#;

/// Starts a session, sends `line` `count` times and then one good update, and runs the driver
/// until nothing more can happen. Returns the events and every line the client wrote after the
/// handshake.
fn send_after_start(line: &str, count: usize) -> (Vec<AcpEvent>, Vec<String>) {
    let (client, events, driver, mut agent) = pipes();
    let mut pool = LocalPool::new();
    pool.spawner()
        .spawn_local(async move { drop(driver.await) })
        .unwrap();
    pool.run_until(async {
        futures::join!(handshake(&client), agent.start());
    });
    for _ in 0..count {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        agent.stdout.unbounded_send(Ok(bytes)).unwrap();
    }
    agent.update(&json!({"sessionId": "s-1", "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "after"},
    }}));
    pool.run_until_stalled();
    let received = drain(&events);
    let mut written = Vec::new();
    while let Ok(bytes) = agent.written.try_recv() {
        written.push(String::from_utf8_lossy(&bytes).into_owned());
    }
    drop(client);
    (received, written)
}

fn after() -> AcpEvent {
    AcpEvent::Update {
        session: session(),
        origin: Origin::Idle,
        update: AgentUpdate::MessageChunk {
            role: MessageRole::Answer,
            acp_message_id: None,
            content: ContentBlock::Text {
                text: "after".to_owned(),
            },
        },
    }
}

#[test]
fn a_line_the_sdk_refuses_is_a_malformed_line_and_nothing_is_written_back() {
    for line in [HUGE_NUMBER, RAW_VALUE_KEY, RAW_VALUE_PARAMS] {
        let (received, written) = send_after_start(line, 1);
        assert_eq!(written, Vec::<String>::new(), "written back for {line}");
        assert_eq!(
            received,
            [
                AcpEvent::Warning {
                    session: None,
                    warning: ProtocolWarning::MalformedLine { bytes: line.len() },
                },
                after(),
            ],
            "events for {line}"
        );
    }
}

#[test]
fn lines_the_sdk_refuses_never_stall_the_reader() {
    for line in [HUGE_NUMBER, RAW_VALUE_KEY, RAW_VALUE_PARAMS] {
        let (received, _) = send_after_start(line, MAX_LINES_IN_FLIGHT + 1);
        assert!(
            received.contains(&after()),
            "the update after {} such lines never arrived ({} events): {line}",
            MAX_LINES_IN_FLIGHT + 1,
            received.len()
        );
    }
}
