//! codex-acp's report of an MCP server that did not start (`map/mcp_startup.rs`): the failed
//! `tool_call` it makes up reaches the host as a warning naming the server and why, never as a
//! step. The reports are the ones recorded from codex-acp 2.10.0 and Codex 0.171.0 (a server
//! whose program exits at once, one that never answers), with the session id made neutral; the
//! cancelled and skipped ones are the adapter's own words (`dist/index.js:42387-42411`).

use super::*;
use crate::event::{McpStartupReason, ProtocolWarning};

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

/// A start-up report as codex-acp sends it, for `server` with `id`'s uuid and `text`.
fn report(server: &str, uuid: &str, text: &str) -> Value {
    json!({"sessionUpdate": "tool_call", "toolCallId": format!("mcp_startup.{server}.{uuid}"),
        "kind": "other", "title": format!("mcp__{server}__startup"), "status": "failed",
        "content": [{"type": "content", "content": {"type": "text", "text": text}}]})
}

/// The recorded report for a server whose program exits at once.
fn exited() -> Value {
    report(
        "fails",
        "acee92bd-26cd-4814-9fae-a6deff3f0a10",
        "[codex-acp forwarded startup error] MCP server `fails` failed to start: MCP client for \
         `fails` failed to start: MCP startup failed: handshaking with MCP server failed: \
         connection closed: initialize response: connection closed: initialize response",
    )
}

/// The recorded report for a server that never answers, 30 s after the session started.
fn silent() -> Value {
    report(
        "silent",
        "f40c8e05-ae4e-4bc5-9bec-ba83dbe21fbb",
        "[codex-acp forwarded startup error] MCP server `silent` failed to start: MCP client for \
         `silent` timed out after 30 seconds. Add or adjust `startup_timeout_sec` in your \
         config.toml:\n[mcp_servers.silent]\nstartup_timeout_sec = XX",
    )
}

fn not_started(server: &str, reason: McpStartupReason) -> AcpEvent {
    AcpEvent::Warning {
        session: Some(session()),
        warning: ProtocolWarning::McpServerNotStarted {
            server: server.to_owned(),
            reason,
        },
    }
}

#[test]
fn a_failed_start_up_report_is_a_warning_and_no_step() {
    let cancelled = report(
        "browser",
        "5d37b1d1-814c-49dc-ae1a-cc17ee26f9b4",
        "[codex-acp forwarded startup error] MCP server `browser` startup was cancelled.",
    );
    let skipped = report(
        "case_law",
        "ac32c4f6-faa6-4787-bc14-e540a9545046",
        "[codex-acp] MCP server `case_law` was not started, because the Codex config already \
         defines an MCP server with this name. Codex uses its own entry, or no server if that \
         entry is disabled.",
    );
    let events = events_for(&[exited(), silent(), cancelled, skipped]);
    assert_eq!(
        events,
        [
            not_started("fails", McpStartupReason::Failed),
            not_started("silent", McpStartupReason::TimedOut),
            not_started("browser", McpStartupReason::Cancelled),
            not_started("case_law", McpStartupReason::NameTaken),
        ]
    );
}

/// An update failing a report's id goes the same way; with no title, the name comes from the
/// id as codex-acp encoded it.
#[test]
fn a_failed_update_of_a_start_up_report_is_a_warning_too() {
    let events = events_for(&[
        json!({"sessionUpdate": "tool_call_update",
            "toolCallId": "mcp_startup.silent.f40c8e05-ae4e-4bc5-9bec-ba83dbe21fbb",
            "status": "failed"}),
        json!({"sessionUpdate": "tool_call_update",
            "toolCallId": "mcp_startup.case%20law.0f3c", "status": "failed"}),
        json!({"sessionUpdate": "tool_call_update",
            "toolCallId": "mcp_startup.x.0f3c", "title": "mcp__fails__startup",
            "status": "failed"}),
    ]);
    assert_eq!(
        events,
        [
            not_started("silent", McpStartupReason::Failed),
            not_started("case%20law", McpStartupReason::Failed),
            not_started("fails", McpStartupReason::Failed),
        ]
    );
}

/// A call or an update that differs from the report's shape in any part stays a step.
#[test]
fn a_call_of_another_shape_stays_a_step() {
    let mut running = exited();
    running["status"] = json!("in_progress");
    let mut other_title = exited();
    other_title["title"] = json!("mcp__fails__search");
    let mut empty_name = exited();
    empty_name["title"] = json!("mcp____startup");
    let mut other_id = exited();
    other_id["toolCallId"] = json!("call_mcp_startup.fails.1");
    let calls = [running, other_title, empty_name, other_id];
    let updates = [
        json!({"sessionUpdate": "tool_call_update",
            "toolCallId": "mcp_startup.fails.1", "status": "completed"}),
        json!({"sessionUpdate": "tool_call_update",
            "toolCallId": "mcp_startup.fails.1", "title": "Search", "status": "failed"}),
    ];
    let events = events_for(&[&calls[..], &updates[..]].concat());
    let steps: Vec<_> = events
        .iter()
        .map(|event| {
            if let AcpEvent::Update {
                update: AgentUpdate::ToolCall(call),
                ..
            } = event
            {
                return call.acp_id.to_string();
            }
            let AcpEvent::Update {
                update: AgentUpdate::ToolCallUpdate { acp_id, .. },
                ..
            } = event
            else {
                panic!("expected a step, got {event:?}");
            };
            acp_id.to_string()
        })
        .collect();
    assert_eq!(
        steps,
        [
            "mcp_startup.fails.acee92bd-26cd-4814-9fae-a6deff3f0a10",
            "mcp_startup.fails.acee92bd-26cd-4814-9fae-a6deff3f0a10",
            "mcp_startup.fails.acee92bd-26cd-4814-9fae-a6deff3f0a10",
            "call_mcp_startup.fails.1",
            "mcp_startup.fails.1",
            "mcp_startup.fails.1",
        ]
    );
}
