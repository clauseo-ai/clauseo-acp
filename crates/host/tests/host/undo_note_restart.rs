//! The undo note survives a restart of the host: the next prompt the agent gets after an undo
//! starts with it, once, whether the host restarted between the undo and that prompt, or a
//! prompt that should have carried it was refused or failed first. The prompt after the one
//! that carried it has no note.
//!
//! Each run's agent expects exactly the prompts that may reach it, text and all, so a note left
//! out, or sent twice, is a mismatch.

use clauseo_host::codex_input::CODEX_MAX_INPUT_CHARS;
use clauseo_protocol::{
    CheckpointKind, DomainEvent, ErrorCode, FileChoice, FileDecision, HostCommand, MatterPath,
    RestoreRequest, StopReason, StreamId, ThreadId,
};
use serde_json::{Value, json};

use crate::codex::{CODEX, turn_failed};
use crate::codex_cap::written;
use crate::limits_host::CLAUDE;
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};
use crate::versions::matter_events;
use crate::write;

/// The bundled scenarios' folder.
const SCENARIOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fake_agent/scenarios");
pub(crate) const NEXT: &str = "Then summarise the rent review.";
pub(crate) const AFTER: &str = "Now list the parties.";

/// The note after `files` were undone, in that order (`crates/host/src/undo_note.rs`).
pub(crate) fn note(files: &str) -> String {
    format!(
        "Note from Clauseo: since your last turn, the lawyer put these files back to an earlier \
         version, and removed those it did not have: {files}. Read them again before relying \
         on what you remember about them."
    )
}

/// A `session/prompt` labelled `label` whose blocks are exactly `texts`.
pub(crate) fn prompt_exactly(label: &str, texts: &[&str]) -> Value {
    let blocks: Vec<Value> = texts
        .iter()
        .map(|text| json!({"type": "text", "text": text}))
        .collect();
    json!({"expect": {"method": "session/prompt", "as": label,
        "params": {"sessionId": "sess-1", "prompt": blocks}}})
}

pub(crate) fn end_turn(label: &str) -> Value {
    json!({"respond": {"to": label, "result": {"stopReason": "end_turn"}}})
}

/// A prompt labelled `label` and answered `end_turn`, whose blocks are exactly `texts`.
pub(crate) fn answered(label: &str, texts: &[&str]) -> [Value; 2] {
    [prompt_exactly(label, texts), end_turn(label)]
}

/// A turn that edits `file`, as either adapter may report it.
pub(crate) fn edits(label: &str, file: &str) -> Vec<Value> {
    vec![
        json!({"expect": {"method": "session/prompt", "as": label,
            "params": {"sessionId": "sess-1"}}}),
        json!({"write_file": {"path": file, "content": "final\n"}}),
        json!({"update": {"update": {"sessionUpdate": "tool_call",
            "toolCallId": format!("{label}-edit"), "title": format!("Edit {file}"),
            "kind": "edit", "status": "completed",
            "content": [{"type": "diff", "path": format!("{{{{cwd}}}}/{file}"),
                "oldText": "draft\n", "newText": "final\n"}],
            "locations": [{"path": format!("{{{{cwd}}}}/{file}")}]}}}),
        end_turn(label),
    ]
}

/// The agent's lines that reopen `sess-1` after a restart: Codex resumes it, Claude loads it.
fn reopen(agent: &str) -> Vec<Value> {
    let lines = |name: &str| -> Vec<Value> {
        let text = std::fs::read_to_string(format!("{SCENARIOS}/{name}")).unwrap();
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    };
    if agent == CODEX {
        // Initialize, the resume and the mode, as the bundled scenario plays them.
        lines("codex-resume.jsonl")[1..7].to_vec()
    } else {
        let handshake = lines("common/claude-handshake.jsonl");
        let mut modes = handshake[3]["respond"]["result"].clone();
        modes.as_object_mut().unwrap().remove("sessionId");
        vec![
            handshake[0].clone(),
            handshake[1].clone(),
            json!({"expect": {"method": "session/load", "as": "load",
                "params": {"sessionId": "sess-1", "cwd": "{{any}}"}}}),
            json!({"respond": {"to": "load", "result": modes}}),
        ]
    }
}

/// The handshake file of `agent`'s first run.
pub(crate) fn handshake(agent: &str) -> &'static str {
    if agent == CODEX {
        "codex-handshake.jsonl"
    } else {
        "claude-handshake.jsonl"
    }
}

/// Writes the scenario of a run after a restart: `agent` reopens `sess-1`, then plays `steps`.
pub(crate) fn reopened(dir: &Dir, name: &str, agent: &str, steps: Vec<Value>) -> Scenario {
    let mut text = json!({"scenario": name, "description": "the undo note's reopened agent",
        "on_stdin_close": "exit"})
    .to_string();
    text.push('\n');
    for step in reopen(agent).into_iter().chain(steps) {
        text.push_str(&step.to_string());
        text.push('\n');
    }
    let file = dir.data().join("scenarios").join(format!("{name}.jsonl"));
    write::write(&file, text.as_bytes());
    Scenario::Written(file)
}

/// Undoes `turn`'s edit of `file` and waits for the `StepUndone`.
pub(crate) fn undo(
    host: &Running,
    events: &mut Events,
    thread: ThreadId,
    turn: clauseo_protocol::TurnId,
    file: &str,
) {
    let before = events.count(|event| matches!(event, DomainEvent::StepUndone(_)));
    host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: vec![FileDecision {
            path: MatterPath::new(file).unwrap(),
            choice: FileChoice::Undo,
        }],
    });
    events.until("StepUndone", |e| {
        e.count(|event| matches!(event, DomainEvent::StepUndone(_))) > before
    });
}

/// Runs a prompt `text` to its end; returns how it stopped.
pub(crate) fn run_prompt(
    host: &Running,
    events: &mut Events,
    thread: ThreadId,
    text: &str,
) -> StopReason {
    let (turn, _) = host.prompt(thread, text);
    events.until_turn_ended(turn).stop
}

/// Run 1 on the same data folder: the agent plays `steps` after reopening its session, `NEXT`
/// and then `AFTER` are sent, and both must end `EndTurn` with no mismatch.
pub(crate) fn after_restart(
    dir: &Dir,
    thread: ThreadId,
    agent: &str,
    name: &str,
    steps: Vec<Value>,
) {
    let scenario = reopened(dir, name, agent, steps);
    dir.set_agents(&[(agent, scenario)]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(
        run_prompt(&host, &mut events, thread, NEXT),
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert_eq!(
        run_prompt(&host, &mut events, thread, AFTER),
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}

/// Run 0: one turn edits brief.txt and is undone; the host shuts down before any prompt.
fn undone_then_restarted(agent: &str, test: &str) {
    let dir = Dir::new(test, &[]);
    let scenario = written(
        &dir,
        "undo-note-run0",
        handshake(agent),
        edits("p1", "brief.txt"),
    );
    dir.set_agents(&[(agent, scenario)]);
    let host = dir.start(0);
    let files = [("brief.txt", "draft\n")];
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &files, agent);
    let mut events = host.events(StreamId::Thread(thread));
    let (edited, _) = host.prompt(thread, "Finish the brief.");
    assert_eq!(events.until_turn_ended(edited).stop, StopReason::EndTurn);
    undo(&host, &mut events, thread, edited, "brief.txt");
    events.assert_no_mismatch();
    host.shutdown();

    let brief = note("brief.txt");
    let mut steps = answered("p2", &[&brief, NEXT]).to_vec();
    steps.extend(answered("p3", &[AFTER]));
    after_restart(&dir, thread, agent, "undo-note-run1", steps);
}

#[test]
fn the_undo_note_survives_a_restart_before_the_next_codex_prompt() {
    undone_then_restarted(CODEX, "undo_note_restart_codex");
}

#[test]
fn the_undo_note_survives_a_restart_before_the_next_claude_prompt() {
    undone_then_restarted(CLAUDE, "undo_note_restart_claude");
}

/// Two turns edit two files, both are undone, the host restarts: the next prompt names both,
/// once each, in the order they were undone.
#[test]
fn two_undos_then_a_restart_are_told_once() {
    let dir = Dir::new("undo_note_restart_two", &[]);
    let mut steps = edits("p1", "brief.txt");
    steps.extend(edits("p2", "memo.txt"));
    let scenario = written(&dir, "undo-note-two", "claude-handshake.jsonl", steps);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let files = [("brief.txt", "draft\n"), ("memo.txt", "draft\n")];
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Finish the brief.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    let (second, _) = host.prompt(thread, "Finish the memo.");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    undo(&host, &mut events, thread, second, "memo.txt");
    undo(&host, &mut events, thread, first, "brief.txt");
    events.assert_no_mismatch();
    host.shutdown();

    let both = note("memo.txt, brief.txt");
    let mut steps = answered("p3", &[&both, NEXT]).to_vec();
    steps.extend(answered("p4", &[AFTER]));
    after_restart(&dir, thread, CLAUDE, "undo-note-two-run1", steps);
}

/// Codex: the prompt that should carry the note is refused before it is sent (over Codex's cap
/// with it), the host restarts, and the next prompt carries the note.
#[test]
fn a_refused_codex_prompt_then_a_restart_keeps_the_note() {
    let dir = Dir::new("undo_note_restart_refused", &[]);
    let scenario = written(
        &dir,
        "undo-note-refused",
        "codex-handshake.jsonl",
        edits("p1", "brief.txt"),
    );
    dir.set_agents(&[(CODEX, scenario)]);
    let host = dir.start(0);
    let files = [("brief.txt", "draft\n")];
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &files, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (edited, _) = host.prompt(thread, "Finish the brief.");
    assert_eq!(events.until_turn_ended(edited).stop, StopReason::EndTurn);
    undo(&host, &mut events, thread, edited, "brief.txt");
    let (refused, _) = host.prompt(thread, &"a".repeat(CODEX_MAX_INPUT_CHARS - 10));
    assert_eq!(events.until_turn_ended(refused).stop, StopReason::Failed);
    let failed = turn_failed(&events, refused).expect("the turn failed");
    assert_eq!(failed.code, ErrorCode::PromptTooLong, "{}", events.dump());
    events.assert_no_mismatch();
    host.shutdown();

    let brief = note("brief.txt");
    let mut steps = answered("p2", &[&brief, NEXT]).to_vec();
    steps.extend(answered("p3", &[AFTER]));
    after_restart(&dir, thread, CODEX, "undo-note-refused-run1", steps);
}

/// Claude: the prompt carrying the note fails at the agent (a JSON-RPC error, nothing said), so
/// the note is kept: the next prompt in the same run carries it again, that one fails too, the
/// host restarts, and the prompt after the restart carries it, once.
#[test]
fn a_failed_claude_prompt_then_a_restart_keeps_the_note() {
    let dir = Dir::new("undo_note_restart_failed", &[]);
    let brief = note("brief.txt");
    let mut steps = edits("p1", "brief.txt");
    for (label, text) in [("p2", "First try."), ("p3", "Second try.")] {
        steps.push(prompt_exactly(label, &[&brief, text]));
        steps.push(json!({"respond": {"to": label,
            "error": {"code": -32603, "message": "Internal error"}}}));
    }
    let scenario = written(&dir, "undo-note-failed", "claude-handshake.jsonl", steps);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let files = [("brief.txt", "draft\n")];
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (edited, _) = host.prompt(thread, "Finish the brief.");
    assert_eq!(events.until_turn_ended(edited).stop, StopReason::EndTurn);
    undo(&host, &mut events, thread, edited, "brief.txt");
    for text in ["First try.", "Second try."] {
        let stop = run_prompt(&host, &mut events, thread, text);
        assert_eq!(stop, StopReason::Failed, "{}", events.dump());
    }
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();

    let mut steps = answered("p4", &[&brief, NEXT]).to_vec();
    steps.extend(answered("p5", &[AFTER]));
    after_restart(&dir, thread, CLAUDE, "undo-note-failed-run1", steps);
}

/// A Versions restore of brief.txt, then a restart before any prompt: the next prompt tells the
/// restore once (from its `VersionRestored`, which lists the file), and the one after does not.
#[test]
fn a_restore_then_a_restart_is_told_once() {
    let dir = Dir::new("undo_note_restart_restore", &[]);
    let scenario = written(
        &dir,
        "undo-note-restore",
        "claude-handshake.jsonl",
        edits("p1", "brief.txt"),
    );
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let files = [("brief.txt", "draft\n")];
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(
        run_prompt(&host, &mut events, thread, "Finish the brief."),
        StopReason::EndTurn
    );
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    host.ok(HostCommand::RestoreVersion {
        thread: None,
        undoes: None,
        matter,
        checkpoint: import,
        scope: RestoreRequest::Files {
            paths: vec![MatterPath::new("brief.txt").unwrap()],
        },
    });
    let brief = std::fs::read_to_string(folder.join("brief.txt")).unwrap();
    assert_eq!(brief, "draft\n", "the restore put brief.txt back");
    events.assert_no_mismatch();
    host.shutdown();

    // Run 0's clock stands at its start (`Dir::clock`), as in `versions.rs`'s restore note.
    let restored = "Note from Clauseo: The lawyer restored brief.txt from Versions; it's back as \
        of 2026-09-21 14:13 UTC. Read them again before relying on what you remember about them.";
    let mut steps = answered("p2", &[restored, NEXT]).to_vec();
    steps.extend(answered("p3", &[AFTER]));
    after_restart(&dir, thread, CLAUDE, "undo-note-restore-run1", steps);
}
