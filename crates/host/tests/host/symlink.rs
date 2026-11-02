//! A matter folder reached through a symlink (macOS `/tmp` is `/private/tmp`): the agent works in
//! the folder's real path, as Claude Code does, and names its edits through it. Those edits are
//! the agent's: the tool calls name them inside the matter, `FilesChanged` does not flag them as
//! possibly the lawyer's, undo puts them back, and the next turn keeps the same agent
//! (clauseo-ai/clauseo-acp-private#231).

use std::path::Path;

use clauseo_protocol::{
    CommandOutcome, DomainEvent, FileChoice, FileDecision, HostCommand, MatterPath, StopReason,
    StreamId, ToolLocation,
};
use serde_json::{Value, json};

use crate::support::{Dir, FAKE, Scenario};
use crate::turn::change_label;
use crate::write;

/// One `tool_call` update that edited `path` (absolute), with `path` as its diff and location.
fn edit(id: &str, path: &str, old: Option<&str>, new: &str) -> Value {
    let mut diff = json!({"type": "diff", "path": path, "newText": new});
    if let Some(old) = old {
        diff["oldText"] = json!(old);
    }
    json!({"update": {"update": {
        "sessionUpdate": "tool_call", "toolCallId": id, "title": format!("Edit {id}"),
        "name": "Edit", "kind": "edit", "status": "completed",
        "content": [diff], "locations": [{"path": path}],
    }}})
}

/// A scenario whose one turn edits `brief.txt`, naming it by `real` (the folder's real path),
/// and writes `notes/a.txt`, naming it by `{{cwd}}/../real/notes/a.txt`: a spelling no text
/// comparison with the session folder places inside it, which only resolving the path does.
fn scenario(real: &Path) -> String {
    let real = real.display();
    let lines = [
        json!({"scenario": "symlinked-matter", "description": "One turn edits brief.txt and \
            writes notes/a.txt, naming both through the matter folder's real path; a second turn \
            in the same session",
            "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"cwd": "{{any}}"}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
        json!({"write_file": {"path": "brief.txt", "content": "Clause 4 final\n"}}),
        edit(
            "t1-edit-brief",
            &format!("{real}/brief.txt"),
            Some("Clause 4 draft\n"),
            "Clause 4 final\n",
        ),
        json!({"write_file": {"path": "notes/a.txt", "content": "Alpha\n"}}),
        edit("t1-write-a", "{{cwd}}/../real/notes/a.txt", None, "Alpha\n"),
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk",
            "messageId": "m-answer-1",
            "content": {"type": "text", "text": "Edited the brief and wrote a note."}}}}),
        json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}),
        json!({"expect": {"method": "session/prompt", "as": "p2",
            "params": {"sessionId": "sess-1"}}}),
        json!({"respond": {"to": "p2", "result": {"stopReason": "end_turn"}}}),
    ];
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

#[test]
fn symlinked_matter_edits_are_the_agents() {
    let dir = Dir::new("symlinked_matter_edits_are_the_agents", &[]);
    let real = dir.folder("real", &[("brief.txt", "Clause 4 draft\n")]);
    let link = real.with_file_name("link");
    write::symlink(&real, &link);
    // The temp folder may itself sit behind a symlink (macOS `/var`); the agent sees the end.
    let resolved = dunce::canonicalize(&real).unwrap();
    let file = real.with_file_name("symlinked-matter.jsonl");
    write::write(&file, scenario(&resolved).as_bytes());
    dir.set_agents(&[(FAKE, Scenario::Written(file))]);
    let host = dir.start(0);
    let matter = host.create_matter("m", &link);
    let thread = host.create_thread(matter, FAKE);

    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Finish the brief.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());

    let changed = events
        .events()
        .find_map(|event| match event {
            DomainEvent::FilesChanged(changed) if changed.turn == turn => Some(changed.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no FilesChanged: {}", events.dump()));
    let mut files: Vec<(&str, &str, bool)> = changed
        .files
        .iter()
        .map(|f| {
            (
                f.path().as_str(),
                change_label(f),
                f.may_include_lawyer_edits(),
            )
        })
        .collect();
    files.sort_unstable();
    assert_eq!(
        files,
        [
            ("brief.txt", "modified", false),
            ("notes/a.txt", "added", false)
        ]
    );

    // The edit named through the real path is inside the matter, as the lawyer sees it.
    let brief = MatterPath::new("brief.txt").unwrap();
    let locations = events
        .events()
        .find_map(|event| match event {
            DomainEvent::ToolCallStarted(started) if started.acp_id == "t1-edit-brief" => {
                Some(started.locations.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no ToolCallStarted: {}", events.dump()));
    assert_eq!(
        locations,
        [ToolLocation::Inside {
            path: brief.clone(),
            line: None
        }]
    );

    let note = MatterPath::new("notes/a.txt").unwrap();
    let outcome = host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: vec![
            FileDecision {
                path: brief.clone(),
                choice: FileChoice::Undo,
            },
            FileDecision {
                path: note.clone(),
                choice: FileChoice::Undo,
            },
        ],
    });
    assert!(
        matches!(outcome, CommandOutcome::StepUndone { .. }),
        "{outcome:?}"
    );
    events.until("StepUndone", |e| {
        e.position(|event| matches!(event, DomainEvent::StepUndone(_)))
            .is_some()
    });
    let undone = events
        .events()
        .find_map(|event| match event {
            DomainEvent::StepUndone(undone) => Some(undone.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(undone.restored, [brief, note]);
    assert!(undone.kept.is_empty(), "{undone:?}");
    let read = |path: &str| std::fs::read_to_string(real.join(path)).ok();
    assert_eq!(read("brief.txt").as_deref(), Some("Clause 4 draft\n"));
    assert_eq!(read("notes/a.txt"), None);

    // The agent started in the folder's one spelling, so the next turn reuses it: the folder
    // check before a turn (`restart_if_moved`) does not take the spellings for a move.
    let (second, _) = host.prompt(thread, "Carry on.");
    let ended = events.until_turn_ended(second);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let sessions = events.count(|event| matches!(event, DomainEvent::AgentSessionAttached(_)));
    let detached = events.count(|event| matches!(event, DomainEvent::AgentSessionDetached(_)));
    assert_eq!((sessions, detached), (1, 0), "{}", events.dump());
    host.shutdown();
}
