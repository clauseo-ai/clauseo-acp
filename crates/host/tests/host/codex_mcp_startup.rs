//! A Codex MCP server that did not start (codex-acp 2.10.0 reports it as a failed `tool_call`
//! titled `mcp__<name>__startup`) is no step of the lawyer's: not when it comes right after the
//! `session/new` answer, before any prompt, nor in the middle of a turn, nor its update. The
//! host only logs it. The scenario also checks that `session/new` carries the start-up wait.

use clauseo_protocol::{DomainEvent, StopReason, StreamId};

use crate::support::{Dir, Scenario, matter_with_thread};

const CODEX: &str = "codex-acp";

#[test]
fn a_codex_mcp_server_that_did_not_start_is_no_step() {
    let test = "a_codex_mcp_server_that_did_not_start_is_no_step";
    let dir = Dir::new(
        test,
        &[(CODEX, Scenario::Local("codex-mcp-startup-failed"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the papers.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // An agent-started turn would open for held output once the lawyer's turn has closed.
    host.shutdown();
    events.until_end();
    // The scenario has no other tool call, so no step and no step's update at all.
    let steps = events.count(|event| {
        matches!(
            event,
            DomainEvent::ToolCallStarted(_) | DomainEvent::ToolCallUpdated(_)
        )
    });
    assert_eq!(steps, 0, "{}", events.dump());
    let turns = events.count(|event| matches!(event, DomainEvent::TurnStarted(_)));
    assert_eq!(turns, 1, "{}", events.dump());
    assert_eq!(events.turn_text(turn), "Reading the papers. Done.");
}
