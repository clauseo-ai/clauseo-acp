//! A request Clauseo's data fence refuses after the lawyer allowed another in the same turn
//! (`actor/turn/outside.rs`): the agent gets the reject-once option, then, as a steer into the
//! running turn, the host's note that Clauseo blocked it and the lawyer did not decline it; the
//! lawyer sees one `OutsideMatterBlocked` notice, never a permission row for it. The agent
//! retrying the request gets the reject again, and no second note or notice. The note is never
//! logged as a message.

use std::path::Path;

use clauseo_protocol::{DomainEvent, NoticeKind, NoticeLevel, StopReason, StreamId};
use serde_json::{Value, json};

use crate::permissions::answer_from;
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// The host's note to the agent (`outside.rs`'s `AGENT_NOTE`).
const NOTE: &str =
    "Clauseo blocked that request: it reaches outside this matter. The lawyer did not decline it.";

/// The notice's fallback words (`outside.rs`'s `NOTICE_TEXT`).
const NOTICE: &str = "Clauseo blocked a request: it reaches outside this matter.";

/// claude-agent-acp's options, in forced ask (no `allow_always`).
fn options() -> Value {
    json!([
        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
    ])
}

/// A Bash request `id` for `command`, and the answer the agent must get.
fn ask(id: &str, command: &str, answer: &str) -> [Value; 2] {
    let call = json!({"toolCallId": format!("toolu_{id}"), "title": command, "kind": "execute",
        "status": "pending", "rawInput": {"command": command, "description": "Run it"}});
    [
        json!({"request": {"id": id, "method": "session/request_permission",
            "params": {"sessionId": "sess-1", "toolCall": call, "options": options()}}}),
        json!({"expect_response": {"id": id,
            "result": {"outcome": {"outcome": "selected", "optionId": answer}}}}),
    ]
}

/// A claude-agent-acp with steering: one request the lawyer allows, then two into the data
/// folder at `data`, the note expected between them.
fn scenario(data: &Path) -> String {
    let skill = format!(
        "cat '{}/skill-packs/x/skills/brief/SKILL.md'",
        data.display()
    );
    let mut lines = vec![
        json!({"scenario": "outside-told", "description": "An allowed request, then two into \
            the data folder: rejected, one note steered, one notice", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true,
                "_meta": {"claudeCode": {"promptQueueing": true}}},
            "authMethods": [], "agentInfo": {"name": "fake_agent", "version": "0.0.0"},
            "_meta": {"steering": {"supported": true}}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
    ];
    lines.extend(ask("version", "stetdoc --version", "allow"));
    lines.extend(ask("fence-1", &skill, "reject"));
    lines.push(
        json!({"expect": {"method": "_session/steering", "as": "steer",
        "params": {"sessionId": "sess-1", "prompt": [{"type": "text", "text": NOTE}],
            "_meta": {"steering": {"idleBehavior": "promptRequired"}}}}}),
    );
    lines.push(json!({"respond": {"to": "steer", "result": {"outcome": "injected"}, "raw": true}}));
    lines.extend(ask("fence-2", &skill, "reject"));
    lines.push(json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}));
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

#[test]
fn a_fence_refusal_after_an_allow_is_told_as_clauseos_block() {
    let dir = Dir::new("outside_told", &[]);
    let data = dir.data().to_owned();
    write::write(
        &data.join("skill-packs/x/skills/brief/SKILL.md"),
        b"# brief\n",
    );
    let folder = dir.folder("m", &[("a.txt", "a")]);
    let file = folder.with_file_name("outside-told.jsonl");
    write::write(&file, scenario(&data).as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);

    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the three commands.");
    answer_from(&host, &mut events, thread, 0, &["allow"]);
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // Only the allowed request reached the lawyer: no row reads "Allowed once." for the others.
    assert_eq!(events.permission_requests().len(), 1, "{}", events.dump());
    let notices: Vec<_> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(notices.len(), 1, "{}", events.dump());
    assert_eq!(notices[0].kind, Some(NoticeKind::OutsideMatterBlocked));
    assert_eq!(notices[0].level, NoticeLevel::Warning);
    assert_eq!(notices[0].text, NOTICE);
    // The note is no lawyer's message: the prompt is the only one logged.
    let messages = events
        .events()
        .filter(|event| matches!(event, DomainEvent::UserMessage(_)))
        .count();
    assert_eq!(messages, 1, "{}", events.dump());
    host.shutdown();
}
