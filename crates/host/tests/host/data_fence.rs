//! A permission request that touches Clauseo's own data folder is rejected for the lawyer, who
//! never sees it (`agent/data_fence.rs`, decision record 0030): a Read of a skill pack's source,
//! a Bash `cat` of it, a path whose `..` leads there, and one through a link in the matter
//! folder. A request for a path elsewhere waits for the lawyer as before. The agent's
//! `agent ended` line counts the refusals and names no path. The log is the process's global
//! subscriber, so this module holds one test: nextest runs it in a process of its own.

use std::path::Path;

use clauseo_host::log_file;
use clauseo_protocol::{StopReason, StreamId};
use clauseo_settings::Locations;
use serde_json::{Value, json};

use crate::permissions::answer_from;
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// claude-agent-acp's options.
fn options() -> Value {
    json!([
        {"optionId": "allow_always", "name": "Always Allow", "kind": "allow_always"},
        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
    ])
}

/// A request `id` whose tool call holds `call`, and the answer the agent must get.
fn ask(id: &str, call: &Value, answer: &str) -> [Value; 2] {
    let mut call = call.clone();
    call["toolCallId"] = json!(format!("toolu_{id}"));
    call["status"] = json!("pending");
    [
        json!({"request": {"id": id, "method": "session/request_permission",
            "params": {"sessionId": "sess-1", "toolCall": call, "options": options()}}}),
        json!({"expect_response": {"id": id,
            "result": {"outcome": {"outcome": "selected", "optionId": answer}}}}),
    ]
}

/// A Read of `path`, in claude-agent-acp's shape.
fn read(path: &str) -> Value {
    json!({"title": "Read", "kind": "read", "locations": [{"path": path}],
        "rawInput": {"file_path": path}})
}

/// A Bash command, in claude-agent-acp's shape: no locations.
fn bash(command: &str) -> Value {
    json!({"title": command, "kind": "execute",
        "rawInput": {"command": command, "description": "Read the skill"}})
}

/// The scenario: four requests into the data folder at `data` (whose folder is named `root` in
/// the temp folder), each rejected, then one for a file beside the matter, which the lawyer
/// allows.
fn scenario(data: &Path, root: &str) -> String {
    let data = data.display();
    let skill = format!("{data}/skill-packs/x/skills/brief/SKILL.md");
    let mut lines = vec![
        json!({"scenario": "data-fence", "description": "Four requests into the data folder, \
            rejected unseen; one beside the matter, for the lawyer", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
    ];
    let dotdot = format!("{{{{cwd}}}}/../../{root}/data/run/s-9/../../skill-packs/x/SKILL.md");
    for (id, call, answer) in [
        ("read", read(&skill), "reject"),
        (
            "cat",
            bash(&format!("cat '{skill}' | head -n 20")),
            "reject",
        ),
        ("dotdot", read(&dotdot), "reject"),
        (
            "link",
            bash("sed -n 1,5p packs/x/skills/brief/SKILL.md"),
            "reject",
        ),
        ("outside", read("{{cwd}}/../outside/x.txt"), "allow"),
    ] {
        lines.extend(ask(id, &call, answer));
    }
    lines.push(json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}));
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

#[cfg(unix)]
#[test]
fn a_request_into_the_data_folder_is_rejected_unseen_and_counted() {
    let dir = Dir::new("data_fence_rejects", &[]);
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    let data = dir.data().to_owned();
    write::write(
        &data.join("skill-packs/x/skills/brief/SKILL.md"),
        b"# brief\n",
    );
    let folder = dir.folder("m", &[("a.txt", "a")]);
    dir.folder("outside", &[("x.txt", "x")]);
    write::symlink(&data.join("skill-packs"), &folder.join("packs"));
    let root = data
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let file = folder.with_file_name("data-fence.jsonl");
    write::write(&file, scenario(&data, root).as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);

    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the skill.");
    answer_from(&host, &mut events, thread, 0, &["allow"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // Only the request beside the matter reached the lawyer.
    assert_eq!(events.permission_requests().len(), 1, "{}", events.dump());
    host.shutdown();

    guard.flush();
    let mut text = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        text.push_str(&std::fs::read_to_string(locations.log_dir().join(name)).unwrap_or_default());
    }
    let ended: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("agent ended"))
        .collect();
    assert_eq!(ended.len(), 1, "{text}");
    assert!(ended[0].contains("denied_data_folder=4"), "{text}");
    for needle in [data.to_str().unwrap(), "skill-packs", "SKILL.md", root] {
        assert!(!text.contains(needle), "the log holds {needle:?}:\n{text}");
    }
}
