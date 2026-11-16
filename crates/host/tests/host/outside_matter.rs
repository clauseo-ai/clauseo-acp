//! A turn the agent cancels right after the host refused, unasked, a request into Clauseo's
//! data folder (`asks.rs`'s data-folder refusal: Codex's only reject option, `cancel`, ends the
//! turn) ends `OutsideMatter`, not the lawyer's `Cancelled`, and keeps the answer so far. A Stop
//! after such a refusal still ends the turn `Cancelled`.

use std::path::{Path, PathBuf};

use clauseo_protocol::{CommandOutcome, HostCommand, StopReason, StreamId};
use serde_json::{Value, json};

use crate::permissions::until_first_request;
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// The words the agent says before it asks.
const PARTIAL: &str = "I need to read another matter's file. ";

/// codex-acp's options for a command: `cancel` is its only reject option.
fn options() -> Value {
    json!([
        {"optionId": "allow_once", "name": "Yes, proceed", "kind": "allow_once"},
        {"optionId": "allow_for_session", "name": "Yes, and don't ask again for this command",
            "kind": "allow_always"},
        {"optionId": "cancel", "name": "No, and tell Codex what to do differently",
            "kind": "reject_once"},
    ])
}

/// A permission request `id` for a Read of `path`.
fn request(id: &str, path: &str) -> Value {
    json!({"request": {"id": id, "method": "session/request_permission", "params": {
        "sessionId": "sess-1", "options": options(), "toolCall": {"toolCallId": format!("call-{id}"),
        "title": format!("Read {path}"), "kind": "read", "status": "pending",
        "locations": [{"path": path}], "rawInput": {"file_path": path}}}}})
}

/// The handshake and the first prompt's words, a request into the data folder at `data` that
/// the host must answer `cancel`, then `rest`.
fn scenario(data: &Path, rest: Vec<Value>) -> String {
    let skill = format!("{}/skill-packs/x/skills/brief/SKILL.md", data.display());
    let mut lines = vec![
        json!({"scenario": "outside-matter", "description": "A request into the data folder, \
            refused unseen with cancel", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk",
            "messageId": "m-1", "content": {"type": "text", "text": PARTIAL}}}}),
        request("fence", &skill),
        json!({"expect_response": {"id": "fence",
            "result": {"outcome": {"outcome": "selected", "optionId": "cancel"}}}}),
    ];
    lines.extend(rest);
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

/// A folder whose agent plays `scenario(data, rest)`, and its matter folder.
fn fenced_dir(test: &str, rest: Vec<Value>) -> (Dir, PathBuf) {
    let dir = Dir::new(test, &[]);
    let data = dir.data().to_owned();
    write::write(
        &data.join("skill-packs/x/skills/brief/SKILL.md"),
        b"# brief\n",
    );
    let folder = dir.folder("m", &[("a.txt", "a")]);
    let file = folder.with_file_name("outside-matter.jsonl");
    write::write(&file, scenario(&data, rest).as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);
    (dir, folder)
}

#[test]
fn an_agent_cancel_after_a_data_folder_refusal_is_outside_matter() {
    let rest = vec![json!({"respond": {"to": "p1", "result": {"stopReason": "cancelled"}}})];
    let (dir, folder) = fenced_dir("outside_matter_ends_turn", rest);
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the other matter's brief.");
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::OutsideMatter, "{}", events.dump());
    // The lawyer never saw the request, and the answer so far is kept.
    assert!(events.permission_requests().is_empty(), "{}", events.dump());
    assert_eq!(events.turn_text(turn), PARTIAL, "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_stop_after_a_data_folder_refusal_is_cancelled() {
    let rest = vec![
        request("beside", "{{cwd}}/../outside/x.txt"),
        json!({"note": "Either order: the stopped request and the cancel notification.",
        "all": [
            {"expect_response": {"id": "beside", "result": {"outcome": {"outcome": "cancelled"}}}},
            {"expect": {"method": "session/cancel", "params": {"sessionId": "sess-1"}}},
        ]}),
        json!({"respond": {"to": "p1", "result": {"stopReason": "cancelled"}}}),
    ];
    let (dir, folder) = fenced_dir("outside_matter_then_stop", rest);
    dir.folder("outside", &[("x.txt", "x")]);
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the other matter's brief.");
    // The request beside the matter reaches the lawyer, who stops the answer instead.
    until_first_request(&mut events);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::Cancelled, "{}", events.dump());
    host.shutdown();
}
