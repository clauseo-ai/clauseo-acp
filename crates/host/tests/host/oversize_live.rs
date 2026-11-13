//! A `session/update` over the ACP reader's 8 MiB cap in a live turn: Claude Code's Read of a
//! scanned PDF with `pages` can send one tool result that long. The reader drops the line and
//! keeps the connection (`crates/acp/src/line.rs`), so the turn goes on and ends `end_turn`. The
//! dropped update was the tool call's last, so the host completes the tool calls the turn leaves
//! unfinished when the agent ends it well (`actor/turn/close.rs`, `complete_tools`): the room
//! shows the tool done with no output, never running. A normal stop and a failed turn are as
//! they were: the first gains no tool update, the second leaves its tool calls as they stood.

use clauseo_protocol::{
    DomainEvent, ErrorCode, StopReason, StreamId, ThreadId, ToolCallEntry, ToolCallStatus,
    ViewEntry, ViewPatchBody,
};
use serde_json::{Value, json};

use crate::load_line_cap::{
    MAX_LINE_BYTES, Warns, answered, attached, failures, skips, write_scenario,
};
use crate::observe::Events;
use crate::support::{Dir, FAKE, Running};

/// The tool call's id, as the agent names it.
const TOOL: &str = "tc-read";

/// `tool_call` for [`TOOL`], running.
fn tool_started() -> Value {
    json!({"update": {"update": {"sessionUpdate": "tool_call", "toolCallId": TOOL,
        "title": "Read scan.pdf", "kind": "read", "status": "in_progress",
        "rawInput": {"file_path": "scan.pdf", "pages": "1-20"}}}})
}

/// The `tool_call_update` that finishes [`TOOL`] with `text` as its content, as one line.
fn tool_finished_line(text: &str) -> String {
    json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "sess-1",
        "update": {"sessionUpdate": "tool_call_update", "toolCallId": TOOL,
        "status": "completed", "content": [{"type": "content",
        "content": {"type": "text", "text": text}}]}}})
    .to_string()
}

/// A scenario whose first prompt runs [`TOOL`] with `middle` between its start and the answer,
/// and ends with `end`; then, unless the agent exited, a second prompt answered well.
fn scenario(middle: Vec<Value>, end: Value, second: bool) -> Vec<Value> {
    let mut steps = vec![
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
        tool_started(),
    ];
    steps.extend(middle);
    steps.push(end);
    if second {
        steps.extend(answered("p2", "Second answer."));
    }
    steps
}

/// The answer after the tool call, then `end_turn`.
fn answer_and_end() -> Value {
    json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}})
}

fn after_read() -> Value {
    json!({"update": {"update": {"sessionUpdate": "agent_message_chunk", "messageId": "m-1",
        "content": {"type": "text", "text": "After the read."}}}})
}

/// Starts a host on `name`'s scenario, sends the first prompt and waits for its end.
/// Gives the stop and the turn's text with the rest.
fn first_turn(
    name: &str,
    steps: Vec<Value>,
) -> (Dir, Running, ThreadId, Events, (StopReason, String)) {
    let dir = Dir::new(name, &[]);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    write_scenario(&dir, &folder, name, steps);
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the scan.");
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert!(events.turn_ended(turn).is_some());
    let text = events.turn_text(turn);
    (dir, host, thread, events, (ended.stop, text))
}

/// The tool calls a fresh view of `thread` shows, as the room reads them.
fn tool_calls(host: &Running, thread: ThreadId) -> Vec<ToolCallEntry> {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until("the view's reset", |view| view.patches().next().is_some());
    let Some(ViewPatchBody::Reset { view_window }) = view.patches().next().map(|patch| &patch.body)
    else {
        panic!("the view did not start with a reset: {}", view.dump());
    };
    view_window
        .entries
        .iter()
        .filter_map(|entry| match entry {
            ViewEntry::ToolCall(call) => Some(call.as_ref().clone()),
            _ => None,
        })
        .collect()
}

/// How many `ToolCallUpdated` the stream holds.
fn tool_updates(events: &Events) -> usize {
    events.count(|event| matches!(event, DomainEvent::ToolCallUpdated(_)))
}

#[test]
fn an_oversize_tool_output_mid_turn_is_skipped_and_the_tool_ends_completed() {
    let warns = Warns::keep();
    let huge = tool_finished_line(&"A".repeat(MAX_LINE_BYTES + 1));
    let steps = scenario(
        vec![json!({"send_line": huge}), after_read()],
        answer_and_end(),
        true,
    );
    let (_dir, host, thread, mut events, (stop, text)) = first_turn("oversize_live", steps);
    // The turn carried on past the dropped line and ended well.
    assert_eq!(stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(failures(&events), 0, "{}", events.dump());
    assert!(text.contains("After the read."), "{}", events.dump());
    // The tool call the dropped line would have finished is completed, with no output.
    let calls = tool_calls(&host, thread);
    assert_eq!(calls.len(), 1, "{calls:#?}");
    assert_eq!(calls[0].status, ToolCallStatus::Completed);
    assert!(calls[0].content.is_empty(), "{:?}", calls[0].content);
    assert!(calls[0].finished_at.is_some());
    // The connection lives: the next prompt goes to the same agent and ends well.
    let (second, _) = host.prompt(thread, "And then?");
    let ended = events.until_turn_ended(second);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(attached(&events), ["sess-1"], "{}", events.dump());
    host.shutdown();
    let skipped = skips(&warns);
    assert_eq!(skipped.len(), 1, "{skipped:#?}");
    assert!(
        skipped[0].contains("method=\"session/update\""),
        "{}",
        skipped[0]
    );
    let lines = warns.lock().unwrap();
    assert!(
        lines.iter().all(|line| !line.contains("AAAAAAAA")),
        "content logged"
    );
}

#[test]
fn a_normal_stop_gains_no_tool_update() {
    let finished = json!({"send_line": tool_finished_line("page text")});
    let steps = scenario(vec![finished, after_read()], answer_and_end(), false);
    let (_dir, host, thread, events, (stop, _)) = first_turn("oversize_normal", steps);
    assert_eq!(stop, StopReason::EndTurn, "{}", events.dump());
    // Only the agent's own update: the end adds none to a tool call already finished.
    assert_eq!(tool_updates(&events), 1, "{}", events.dump());
    let calls = tool_calls(&host, thread);
    assert_eq!(calls.len(), 1, "{calls:#?}");
    assert_eq!(calls[0].status, ToolCallStatus::Completed);
    assert_eq!(calls[0].content.len(), 1, "{:?}", calls[0].content);
    host.shutdown();
}

#[test]
fn a_failed_turn_leaves_its_tool_calls_as_they_stood() {
    let exit = json!({"exit": {"code": 1}});
    let (_dir, host, thread, events, (stop, _)) =
        first_turn("oversize_failed", scenario(vec![], exit, false));
    assert_eq!(stop, StopReason::Failed, "{}", events.dump());
    let failed = events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) => Some(failed.code),
        _ => None,
    });
    assert_eq!(failed, Some(ErrorCode::AgentExited), "{}", events.dump());
    // No update from the end: the tool call stays as the agent left it.
    assert_eq!(tool_updates(&events), 0, "{}", events.dump());
    let calls = tool_calls(&host, thread);
    assert_eq!(calls.len(), 1, "{calls:#?}");
    assert_eq!(calls[0].status, ToolCallStatus::InProgress);
    host.shutdown();
}
