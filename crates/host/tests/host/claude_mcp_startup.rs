//! A Claude MCP server that did not start (claude-agent-acp 0.112.0 reports it as a warning
//! `notice` titled `MCP server "<name>" failed to start`) gets the same notice as Codex's report:
//! the host's words, of kind `tool_not_started`, naming the tool as the lawyer added it, once per
//! server per session start, though Claude reports it twice.

use clauseo_protocol::{DomainEvent, NoticeKind, NoticeLevel, StopReason, StreamId};
use serde_json::json;

use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

#[test]
fn a_claude_mcp_server_that_did_not_start_gets_one_tool_not_started_notice() {
    let dir = Dir::new("claude_mcp_server_not_started", &[]);
    let bin = dir.folder("bin", &[]);
    let command = bin.join("firm-precedents").display().to_string();
    let mcp = json!({"mcp": {"servers": {
        "firm-precedents": {"command": command, "enabled": true},
    }}});
    let scenario = Scenario::Local("claude-mcp-startup-failed");
    dir.set_settings(&[(FAKE, scenario)], mcp);
    let host: Running = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the papers.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    host.shutdown();
    events.until_end();

    let notices: Vec<_> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(notices.len(), 1, "{}", events.dump());
    assert_eq!(notices[0].level, NoticeLevel::Warning);
    assert_eq!(notices[0].kind, Some(NoticeKind::ToolNotStarted));
    assert_eq!(
        notices[0].text,
        "\"firm-precedents\" didn't start, so this answer runs without it. Ask again to try \
         once more."
    );
    assert_eq!(events.turn_text(turn), "Reading the papers. Done.");
}
