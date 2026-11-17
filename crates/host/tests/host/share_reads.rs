//! Claude Code's own ask before a read of a matter stored by its network address
//! (`actor/turn/asks/share_reads.rs`), on Windows through the admin share (`\\localhost\C$\…`):
//! a Read, Grep and Glob inside the matter are allowed unseen; a Write there still reaches the
//! lawyer; a Read of another share is refused unseen as before; and with an administrator policy
//! tier (`managed-settings.json` with content) every read reaches the lawyer.
#![cfg(windows)]

use std::path::{Component, Path, PathBuf, Prefix};

use clauseo_protocol::{FolderSupportKind, StopReason, StreamId};
use serde_json::{Value, json};

use crate::permissions::answer_from;
use crate::support::{Dir, Scenario};
use crate::write;

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// claude-agent-acp's options for a network-address ask (no "always").
fn options() -> Value {
    json!([
        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
    ])
}

/// `folder` through the admin share of its drive: `C:\x\y` as `\\localhost\C$\x\y`.
fn through_share(folder: &Path) -> PathBuf {
    let mut parts = folder.components();
    let Some(Component::Prefix(prefix)) = parts.next() else {
        panic!("{folder:?} has no drive");
    };
    let letter = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => char::from(letter),
        other => panic!("{folder:?} is not on a drive letter: {other:?}"),
    };
    let mut share = PathBuf::from(format!(r"\\localhost\{letter}$\"));
    share.extend(parts.filter(|part| !matches!(part, Component::RootDir)));
    share
}

/// Tool call `id` of Claude Code's tool `tool` with `input`, started with its name, then its
/// permission request, and the answer the agent must get.
fn ask(id: &str, tool: &str, kind: &str, input: &Value, answer: &str) -> Vec<Value> {
    let call = json!({"toolCallId": id, "title": tool, "kind": kind, "status": "pending",
        "rawInput": input});
    let mut started = call.clone();
    started["sessionUpdate"] = json!("tool_call");
    started["name"] = json!(tool);
    vec![
        json!({"update": {"update": started}}),
        json!({"request": {"id": id, "method": "session/request_permission",
            "params": {"sessionId": "sess-1", "toolCall": call, "options": options()}}}),
        json!({"expect_response": {"id": id,
            "result": {"outcome": {"outcome": "selected", "optionId": answer}}}}),
    ]
}

/// A Claude session in default mode whose one turn plays `asks`.
fn scenario(asks: Vec<Vec<Value>>) -> String {
    let mut lines = vec![
        json!({"scenario": "share-reads", "description": "Claude Code's network-address asks \
            in a matter stored by its address", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1",
            "modes": {"currentModeId": "default", "availableModes": [
                {"id": "default", "name": "Manual"}, {"id": "plan", "name": "Plan"}]}}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
    ];
    lines.extend(asks.into_iter().flatten());
    lines.push(json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}));
    lines
        .iter()
        .flat_map(|line| [line.to_string(), "\n".to_owned()])
        .collect()
}

/// A host whose Claude thread plays `asks(matter)` in a matter at the admin share, with
/// `managed`'s text in `managed-settings.json` when given, and the lawyer answering `lawyer`
/// in order. Skipped on a machine with no admin share (never on CI).
fn run(test: &str, managed: Option<&str>, asks: fn(&str) -> Vec<Vec<Value>>, lawyer: &[&str]) {
    let dir = Dir::new(test, &[]);
    let local = dir.folder("m", &[("brief.docx", "x")]);
    let folder = through_share(&local);
    if !folder.is_dir() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI's Windows runners have the admin share, and this run has none: {folder:?}"
        );
        eprintln!("skipped: this machine has no admin share at {folder:?}");
        return;
    }
    if let Some(text) = managed {
        write::write(&dir.home().join("managed-settings.json"), text.as_bytes());
    }
    let file = local.with_file_name("share-reads.jsonl");
    let text = folder.to_str().unwrap();
    write::write(&file, scenario(asks(text)).as_bytes());
    dir.set_agents(&[(CLAUDE, Scenario::Written(file))]);
    let host = dir.start_with(0, |config| {
        config
            .with_folder_support(Some(FolderSupportKind::Network))
            .with_pre_write_guard(PathBuf::from(r"C:\clauseo\clauseo-desktop.exe"))
    });
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the brief.");
    answer_from(&host, &mut events, thread, 0, lawyer);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        events.permission_requests().len(),
        lawyer.len(),
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// Read, Grep and Glob inside the matter, allowed unseen; a Write there, for the lawyer, who
/// rejects it; a Read of another share, refused unseen.
fn reads_and_a_change(matter: &str) -> Vec<Vec<Value>> {
    let brief = format!(r"{matter}\brief.docx");
    vec![
        ask(
            "t-read",
            "Read",
            "read",
            &json!({"file_path": brief}),
            "allow",
        ),
        ask(
            "t-grep",
            "Grep",
            "search",
            &json!({"pattern": "x", "path": matter}),
            "allow",
        ),
        ask(
            "t-glob",
            "Glob",
            "search",
            &json!({"pattern": "**/*.docx", "path": matter}),
            "allow",
        ),
        ask(
            "t-write",
            "Write",
            "edit",
            &json!({"file_path": format!(r"{matter}\new.docx"), "content": "x"}),
            "reject",
        ),
        ask(
            "t-other",
            "Read",
            "read",
            &json!({"file_path": r"\\localhost\ADMIN$\win.ini"}),
            "reject",
        ),
    ]
}

/// One Read inside the matter, which the lawyer allows.
fn one_read(matter: &str) -> Vec<Vec<Value>> {
    let brief = format!(r"{matter}\brief.docx");
    vec![ask(
        "t-read",
        "Read",
        "read",
        &json!({"file_path": brief}),
        "allow",
    )]
}

#[test]
fn reads_inside_a_matter_stored_by_its_address_are_allowed_unseen() {
    run("share_reads", None, reads_and_a_change, &["reject"]);
}

#[test]
fn with_an_administrator_policy_every_read_reaches_the_lawyer() {
    let policy = r#"{"permissions": {"ask": ["Read"]}}"#;
    run("share_reads_admin", Some(policy), one_read, &["allow"]);
}
