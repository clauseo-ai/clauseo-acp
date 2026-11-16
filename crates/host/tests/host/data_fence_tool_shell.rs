//! The data fence reads a shell command as the tool that runs it does (`agent/data_fence`'s
//! `Shell`, from the tool call's `name`): Claude Code's `PowerShell` tool's `ls -r` of the data
//! folder's parent is `Get-ChildItem -Recurse`, which walks into the data folder, and is
//! rejected unseen; the same `ls -r` from its `Bash` tool only lists the parent in reverse, and
//! waits for the lawyer as before.

use std::path::Path;

use clauseo_protocol::{StopReason, StreamId};
use serde_json::{Value, json};

use crate::permissions::answer_from;
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// claude-agent-acp's options.
fn options() -> Value {
    json!([
        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
    ])
}

/// Tool call `id` of the tool `tool` running `command`, started, then its permission request;
/// `answer`: the answer the agent must get, when the host gives one unseen.
fn ask(id: &str, tool: &str, command: &str, answer: Option<&str>) -> Vec<Value> {
    let call = json!({"toolCallId": id, "title": command, "kind": "execute", "status": "pending",
        "rawInput": {"command": command, "description": "List it"}});
    let mut started = call.clone();
    started["sessionUpdate"] = json!("tool_call");
    started["name"] = json!(tool);
    let mut lines = vec![
        json!({"update": {"update": started}}),
        json!({"request": {"id": id, "method": "session/request_permission",
            "params": {"sessionId": "sess-1", "toolCall": call, "options": options()}}}),
    ];
    let outcome = answer.unwrap_or("allow");
    lines.push(json!({"expect_response": {"id": id,
        "result": {"outcome": {"outcome": "selected", "optionId": outcome}}}}));
    lines
}

/// The scenario: `ls -r` of `parent` (the data folder's) from PowerShell, rejected unseen, then
/// from Bash, which the lawyer allows.
fn scenario(parent: &Path) -> String {
    let command = format!("ls -r '{}'", parent.display());
    let mut lines = vec![
        json!({"scenario": "data-fence-tool-shell", "description": "ls -r of the data folder's \
            parent: PowerShell's rejected unseen, Bash's for the lawyer",
            "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
    ];
    lines.extend(ask("toolu_ps", "PowerShell", &command, Some("reject")));
    lines.extend(ask("toolu_bash", "Bash", &command, None));
    lines.push(json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}));
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

#[test]
fn ls_r_of_the_data_folders_parent_is_read_as_its_tools_shell() {
    let dir = Dir::new("data_fence_tool_shell", &[]);
    let data = dir.data().to_owned();
    write::write(&data.join("settings.json"), b"{}");
    let parent = data.parent().unwrap().to_owned();
    let folder = dir.folder("m", &[("a.txt", "a")]);
    let file = folder.with_file_name("data-fence-tool-shell.jsonl");
    write::write(&file, scenario(&parent).as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);

    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "List the folder.");
    answer_from(&host, &mut events, thread, 0, &["allow"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // Only Bash's request reached the lawyer.
    assert_eq!(events.permission_requests().len(), 1, "{}", events.dump());
    host.shutdown();
}
