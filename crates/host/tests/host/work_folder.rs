//! The conversation's work folder across a host restart, for an agent with codex-acp's options
//! (its only reject, `cancel`, ends the answer `OutsideMatter`): the relaunched agent's request
//! for the file it wrote in its work folder before reaches the lawyer and its answer ends as
//! asked; another conversation's agent asking for the same file is refused unseen and its
//! answer stops.

use clauseo_protocol::{StopReason, StreamId};
use serde_json::{Value, json};

use crate::permissions::answer_from;
use crate::profile_rig::path_text;
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// codex-acp's options for a command.
fn options() -> Value {
    json!([
        {"optionId": "allow_once", "name": "Yes, proceed", "kind": "allow_once"},
        {"optionId": "cancel", "name": "No, and tell Codex what to do differently",
            "kind": "reject_once"},
    ])
}

/// The handshake, one prompt, then `steps`, as the text of the scenario `name`.
fn scenario(name: &str, steps: Vec<Value>) -> String {
    let mut lines = vec![
        json!({"scenario": name, "description": "The work folder after a restart",
            "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": false}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
    ];
    lines.extend(steps);
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

/// A Read of `path`, asked with codex-acp's options, answered `answer`, then the prompt's end
/// with `stop`.
fn read(path: &str, answer: &str, stop: &str) -> Vec<Value> {
    vec![
        json!({"request": {"id": "read", "method": "session/request_permission", "params": {
            "sessionId": "sess-1", "options": options(), "toolCall": {"toolCallId": "call-read",
            "title": format!("Read {path}"), "kind": "read", "status": "pending",
            "locations": [{"path": path}], "rawInput": {"file_path": path}}}}}),
        json!({"expect_response": {"id": "read",
            "result": {"outcome": {"outcome": "selected", "optionId": answer}}}}),
        json!({"respond": {"to": "p1", "result": {"stopReason": stop}}}),
    ]
}

#[test]
fn a_relaunch_reaches_its_work_folder_and_another_conversation_does_not() {
    let dir = Dir::new("work_folder_restart", &[]);
    let folder = dir.folder("m", &[("a.txt", "a")]);
    let play = |name: &str, steps: Vec<Value>| {
        let file = folder.with_file_name(format!("{name}.jsonl"));
        write::write(&file, scenario(name, steps).as_bytes());
        dir.set_agents(&[(FAKE, Scenario::Written(file))]);
    };
    let end = json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}});
    play("answer", vec![end]);
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let (thread, other) = (
        host.create_thread(matter, FAKE),
        host.create_thread(matter, FAKE),
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Draft the reply.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let state = dir.data().join("agent-state").join(matter.to_string());
    let work = state.join("work").join(thread.to_string());
    assert!(work.is_dir(), "made at the launch");
    // What the agent wrote there.
    write::write(&work.join("draft.md"), b"draft");
    host.shutdown();
    assert!(work.join("draft.md").exists(), "kept when the agent ends");
    let draft = path_text(&work.join("draft.md"));

    // The relaunched agent reads its draft: the lawyer is asked, and the answer ends.
    play("own", read(&draft, "allow_once", "end_turn"));
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the draft.");
    answer_from(&host, &mut events, thread, 0, &["allow_once"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    host.shutdown();

    // Another conversation of the matter asks for it: refused unseen, the answer stops.
    play("other", read(&draft, "cancel", "cancelled"));
    let host = dir.start(2);
    let mut events = host.events(StreamId::Thread(other));
    let (turn, _) = host.prompt(other, "Read the other draft.");
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::OutsideMatter, "{}", events.dump());
    assert!(events.permission_requests().is_empty(), "{}", events.dump());
    assert!(state.join("work").join(other.to_string()).is_dir());
    host.shutdown();
}
