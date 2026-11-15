//! The offline-shell note when its steer cannot go or does not land (`shell_network.rs`):
//! Codex's curl is declined and the lawyer sees one `ShellNetworkBlocked` notice as always, and
//! the note goes first in the thread's next prompt, once. A codex-acp without native steering
//! gets no steer at all; one whose steer answers `promptRequired` gets the steer, then the note
//! again in the next prompt.

use clauseo_protocol::{DomainEvent, NoticeKind, StopReason, StreamId};
use serde_json::{Value, json};

use crate::permissions::answer_from;
use crate::support::{Dir, Scenario, matter_with_thread};
use crate::write;

/// The steered note (`shell_network.rs`'s `AGENT_NOTE`).
const NOTE: &str = "Clauseo declined that command: your shell has no internet. Use your web \
    search or web fetch tool instead, or, if you have the browser, save a file by clicking its \
    link there.";

/// The owed note (`shell_network.rs`'s `OWED_NOTE`).
const OWED: &str = "Note from Clauseo: earlier, Clauseo declined a command of yours that \
    needed the internet, because your shell has no internet. Use your web search or web fetch \
    tool instead, or, if you have the browser, save a file by clicking its link there.";

/// The steered note in a session without web search.
const NOTE_NO_SEARCH: &str = "Clauseo declined that command: your shell has no internet. If you \
    have the browser, save a file by clicking its link there.";

/// The owed note in a session without web search.
const OWED_NO_SEARCH: &str = "Note from Clauseo: earlier, Clauseo declined a command of yours \
    that needed the internet, because your shell has no internet. If you have the browser, save \
    a file by clicking its link there.";

const CODEX: &str = "codex-acp";

/// codex-acp's options for a plain command approval.
fn options() -> Value {
    json!([
        {"optionId": "allow_once", "name": "Yes, proceed", "kind": "allow_once"},
        {"optionId": "decline", "name": "No, continue without running it", "kind": "reject_once"},
        {"optionId": "cancel", "name": "No, and tell Codex what to do differently",
            "kind": "reject_once"},
    ])
}

/// A command approval `id` for `command`.
fn request(id: &str, command: &str) -> Value {
    json!({"request": {"id": id, "method": "session/request_permission",
        "params": {"sessionId": "sess-1", "toolCall": {"toolCallId": format!("tc-{id}"),
            "kind": "execute", "status": "pending", "title": command,
            "rawInput": {"command": command, "cwd": "/m"}}, "options": options()}}})
}

/// The answer the agent must get to `id`.
fn answered(id: &str, option: &str) -> Value {
    json!({"expect_response": {"id": id,
        "result": {"outcome": {"outcome": "selected", "optionId": option}}}})
}

/// A prompt `as` whose blocks are exactly `texts`.
fn prompt(name: &str, texts: &[&str]) -> Value {
    let blocks: Vec<Value> = texts
        .iter()
        .map(|text| json!({"type": "text", "text": text}))
        .collect();
    json!({"expect": {"method": "session/prompt", "as": name,
        "params": {"sessionId": "sess-1", "prompt": blocks}}})
}

/// The scenario `name`: a codex-acp with native steering when `steering`, whose first turn's
/// curl is declined (its steer answered `promptRequired` when `steering`), then an `rm -rf` the
/// lawyer allows (by then the owed note is recorded); the second prompt starts with the owed
/// note, the third has none. The notes offer web search only when `search`.
fn scenario(name: &str, steering: bool, search: bool) -> String {
    let (note, owed) = if search {
        (NOTE, OWED)
    } else {
        (NOTE_NO_SEARCH, OWED_NO_SEARCH)
    };
    let meta = if steering {
        json!({"steering": {"supported": true}})
    } else {
        json!({})
    };
    let mut lines = vec![
        json!({"scenario": name, "description": "Codex's curl declined; the note owed to the \
            next prompt", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init",
            "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "@agentclientprotocol/codex-acp", "title": "Codex",
                "version": "2.2.2"}, "_meta": meta}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1", "modes": {
            "currentModeId": "workspace-write", "availableModes": [
                {"id": "read-only", "name": "Read-only"},
                {"id": "workspace-write", "name": "Workspace access"},
                {"id": "agent", "name": "Auto review"}]}}}}),
        prompt("p1", &["Save the judgment."]),
        request("curl-1", "curl -O https://sci.gov.in/a.pdf"),
    ];
    if steering {
        lines.push(
            json!({"expect": {"method": "_session/steering", "as": "steer",
            "params": {"sessionId": "sess-1", "prompt": [{"type": "text", "text": note}]}}}),
        );
        lines.push(answered("curl-1", "decline"));
        lines.push(
            json!({"respond": {"to": "steer", "result": {"outcome": "promptRequired",
            "reason": "noRunningTurn"}, "raw": true}}),
        );
    } else {
        lines.push(answered("curl-1", "decline"));
    }
    lines.extend([
        request("rm-1", "rm -rf build"),
        answered("rm-1", "allow_once"),
        json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}),
        prompt("p2", &[owed, "And the order."]),
        json!({"respond": {"to": "p2", "result": {"stopReason": "end_turn"}}}),
        prompt("p3", &["Thanks."]),
        json!({"respond": {"to": "p3", "result": {"stopReason": "end_turn"}}}),
    ]);
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

/// Runs `scenario(name, steering, search)` with `tools.web_search` set to `search`: three
/// prompts, each turn ending `EndTurn`, one notice of the kind `search` gives.
fn owed_note_reaches_the_next_prompt(name: &str, steering: bool, search: bool) {
    let dir = Dir::new(name, &[]);
    let file = dir.data().with_file_name(format!("{name}.jsonl"));
    write::write(&file, scenario(name, steering, search).as_bytes());
    let tools = json!({"tools": {"web_search": search}});
    dir.set_settings(&[(CODEX, Scenario::Written(file))], tools);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    for (index, text) in ["Save the judgment.", "And the order.", "Thanks."]
        .into_iter()
        .enumerate()
    {
        let (turn, _) = host.prompt(thread, text);
        if index == 0 {
            answer_from(&host, &mut events, thread, 0, &["allow_once"]);
        }
        let ended = events.until_turn_ended(turn);
        assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    }
    events.assert_no_mismatch();
    let kinds: Vec<_> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.kind.clone()),
            _ => None,
        })
        .collect();
    let kind = if search {
        NoticeKind::ShellNetworkBlocked
    } else {
        NoticeKind::ShellNetworkBlockedNoWebSearch
    };
    assert_eq!(kinds, [Some(kind)], "{}", events.dump());
    // The owed note is no lawyer's message.
    let dump = events.dump();
    assert!(!dump.contains("Note from Clauseo"), "{dump}");
    host.shutdown();
}

#[test]
fn without_native_steering_the_note_goes_first_in_the_next_prompt() {
    owed_note_reaches_the_next_prompt("shell_offline_owed_nosteer", false, true);
}

#[test]
fn a_steer_that_does_not_land_leaves_the_note_for_the_next_prompt() {
    owed_note_reaches_the_next_prompt("shell_offline_owed_missed", true, true);
}

/// With web search off, the lawyer's notice and both notes to Codex offer no web search.
#[test]
fn without_web_search_no_word_offers_it() {
    owed_note_reaches_the_next_prompt("shell_offline_owed_nosearch", true, false);
}
