//! The fence between matters at the interface (`actor/turn/outside.rs`, `refused_call.rs`): a
//! permission request into another matter's folder is rejected unasked, the agent is steered the
//! other-matter note, and the lawyer sees one `OtherMatterRefused` notice naming that matter,
//! once a turn; a tool call that fails as refused on a path in another matter is told the same
//! way, without a note, and a failed Read there whatever its words; once the conversation may
//! read that matter, a refusal there is the plain outside-the-matter notice, with nothing to
//! offer.

use std::path::Path;

use clauseo_protocol::{DomainEvent, HostCommand, NoticeKind, StopReason, StreamId};
use serde_json::{Value, json};

use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// The host's note to the agent (`outside.rs`'s `OTHER_MATTER_NOTE`).
const NOTE: &str = "Clauseo blocked that request: it reaches another matter's folder. Tell the \
    lawyer that folder belongs to another matter; they can let this conversation read that \
    matter's files. The lawyer did not decline it.";

fn options() -> Value {
    json!([
        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
    ])
}

/// A Read request `id` for `file`, and the reject the agent must get.
fn ask(id: &str, file: &Path) -> [Value; 2] {
    let file = file.display().to_string();
    let call = json!({"toolCallId": format!("toolu_{id}"), "title": "Read", "kind": "read",
        "status": "pending", "rawInput": {"file_path": file}});
    [
        json!({"request": {"id": id, "method": "session/request_permission",
            "params": {"sessionId": "sess-1", "toolCall": call, "options": options()}}}),
        json!({"expect_response": {"id": id,
            "result": {"outcome": {"outcome": "selected", "optionId": "reject"}}}}),
    ]
}

/// A Read call `id` of `file` that fails with Claude Code's deny-rule words.
fn refused_read(id: &str, file: &Path) -> [Value; 2] {
    let file = file.display().to_string();
    let words = format!("Permission to read {file} has been denied.");
    failed_call(id, "read", &json!({"file_path": file}), &words)
}

/// A call `id` of `kind` with `input` that fails with `words`.
fn failed_call(id: &str, kind: &str, input: &Value, words: &str) -> [Value; 2] {
    let title = if kind == "read" { "Read" } else { "Bash" };
    [
        json!({"update": {"update": {"sessionUpdate": "tool_call", "toolCallId": id,
            "title": title, "kind": kind, "status": "pending",
            "rawInput": input}}}),
        json!({"update": {"update": {"sessionUpdate": "tool_call_update", "toolCallId": id,
            "status": "failed", "content": [{"type": "content",
                "content": {"type": "text", "text": words}}]}}}),
    ]
}

fn handshake(name: &str) -> Vec<Value> {
    vec![
        json!({"scenario": name, "description": "Refusals in another matter's folder",
            "on_stdin_close": "exit"}),
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
    ]
}

fn text_of(lines: &[Value]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

/// The thread's notices' kinds.
/// No `MattersUnfenced` among them: the fake is no Claude, whose Windows shell rules alone are
/// reported left out (`outside/fence.rs`'s `after_meta`), so on Windows too there is none.
fn kinds(events: &crate::observe::Events) -> Vec<Option<NoticeKind>> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.kind.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_request_into_another_matter_is_rejected_noted_and_named_once() {
    let dir = Dir::new("other_matter_told", &[]);
    let folder = dir.folder("m", &[("a.txt", "a")]);
    let smith = dir.folder("Smith", &[("lease.pdf", "x")]);
    let lease = dunce::canonicalize(smith.join("lease.pdf")).unwrap();
    let mut lines = handshake("other-matter-told");
    lines.extend(ask("one", &lease));
    lines.push(
        json!({"expect": {"method": "_session/steering", "as": "steer",
        "params": {"sessionId": "sess-1", "prompt": [{"type": "text", "text": NOTE}],
            "_meta": {"steering": {"idleBehavior": "promptRequired"}}}}}),
    );
    lines.push(json!({"respond": {"to": "steer", "result": {"outcome": "injected"}, "raw": true}}));
    lines.extend(ask("two", &lease));
    lines.push(json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}));
    let file = folder.with_file_name("other-matter-told.jsonl");
    write::write(&file, text_of(&lines).as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);

    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read Smith's lease.");
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(events.permission_requests().is_empty(), "{}", events.dump());
    let named = NoticeKind::OtherMatterRefused {
        matter: other,
        name: "Smith".to_owned(),
    };
    assert_eq!(kinds(&events), [Some(named)], "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_refused_read_in_another_matter_is_named_and_an_invited_one_is_not() {
    let dir = Dir::new("other_matter_refused_call", &[]);
    let folder = dir.folder("m", &[("a.txt", "a")]);
    let smith = dir.folder("Smith", &[("lease.pdf", "x")]);
    let jones = dir.folder("Jones", &[("deed.pdf", "y")]);
    let lease = dunce::canonicalize(smith.join("lease.pdf")).unwrap();
    let deed = dunce::canonicalize(jones.join("deed.pdf")).unwrap();
    let mut lines = handshake("other-matter-refused-call");
    lines.extend(refused_read("t1", &lease));
    lines.extend(refused_read("t2", &deed));
    lines.push(json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}));
    let file = folder.with_file_name("other-matter-refused-call.jsonl");
    write::write(&file, text_of(&lines).as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);

    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let invited = host.create_matter("Jones", &jones);
    let thread = host.create_thread(matter, FAKE);
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: invited,
    });
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read both.");
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let named = NoticeKind::OtherMatterRefused {
        matter: other,
        name: "Smith".to_owned(),
    };
    assert_eq!(
        kinds(&events),
        [Some(named), Some(NoticeKind::OutsideMatterBlocked)],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// `path` with a drive letter, if it has one, in lower case (`c:\…`).
fn lower_drive(path: &Path) -> String {
    let text = path.display().to_string();
    match text.as_bytes() {
        [drive, b':', ..] => format!("{}{}", char::from(*drive).to_ascii_lowercase(), &text[1..]),
        _ => text,
    }
}

/// Claude Code's words drift, so they are not the only sign: a failed Read in a matter the
/// conversation is not invited into is named whatever it says (a directory deny's words with
/// the drive in lower case; words no release has used), and so is a failed command whose text
/// names such a matter's folder; a failed Read in the conversation's own matter is not.
#[test]
fn a_failed_read_or_command_in_another_matter_is_named_whatever_its_words() {
    let dir = Dir::new("other_matter_any_words", &[]);
    let folder = dir.folder("m", &[("a.txt", "a")]);
    let smith = dir.folder("Smith", &[("lease.pdf", "x")]);
    let jones = dir.folder("Jones", &[("deed.pdf", "y")]);
    let doe = dir.folder("Doe", &[("will.pdf", "z")]);
    let own = dunce::canonicalize(&folder).unwrap();
    let lease = dunce::canonicalize(smith.join("lease.pdf")).unwrap();
    let deed = dunce::canonicalize(jones.join("deed.pdf")).unwrap();
    let will = dunce::canonicalize(doe.join("will.pdf")).unwrap();
    let directory = "<tool_use_error>File is in a directory that is denied by your permission \
        settings.</tool_use_error>";
    let mut lines = handshake("other-matter-any-words");
    let missing = own.join("gone.txt").display().to_string();
    lines.extend(failed_call(
        "t0",
        "read",
        &json!({"file_path": missing}),
        "File does not exist.",
    ));
    let lower = lower_drive(&lease);
    lines.extend(failed_call(
        "t1",
        "read",
        &json!({"file_path": lower}),
        directory,
    ));
    let deed = deed.display().to_string();
    lines.extend(failed_call(
        "t2",
        "read",
        &json!({"file_path": deed}),
        "Access blocked by policy",
    ));
    let words = format!("ls: cannot open '{}': no luck", will.display());
    lines.extend(failed_call(
        "t3",
        "execute",
        &json!({"command": "ls x"}),
        &words,
    ));
    lines.push(json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}));
    let file = folder.with_file_name("other-matter-any-words.jsonl");
    write::write(&file, text_of(&lines).as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);

    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let smith = host.create_matter("Smith", &smith);
    let jones = host.create_matter("Jones", &jones);
    let doe = host.create_matter("Doe", &doe);
    let thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read them.");
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let named = |matter, name: &str| {
        Some(NoticeKind::OtherMatterRefused {
            matter,
            name: name.to_owned(),
        })
    };
    assert_eq!(
        kinds(&events),
        [
            named(smith, "Smith"),
            named(jones, "Jones"),
            named(doe, "Doe")
        ],
        "{}",
        events.dump()
    );
    host.shutdown();
}
