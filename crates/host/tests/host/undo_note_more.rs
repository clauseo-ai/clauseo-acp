//! More of the undo note across restarts (`undo_note_restart.rs`): a restore read from the log
//! and from memory in one run is told once, and not again after a restart; a lawyer's cancel
//! counts the turn as told when the agent did something in it, and not when it was silent.
//!
//! Each run's agent expects exactly the prompts that may reach it, text and all.

use clauseo_protocol::{
    CheckpointKind, DomainEvent, HostCommand, MatterPath, RestoreRequest, StopReason, StreamId,
    ThreadId,
};
use serde_json::{Value, json};

use crate::codex_cap::written;
use crate::limits_host::CLAUDE;
use crate::observe::Events;
use crate::support::{Dir, Running, matter_with_thread};
use crate::undo_note_restart::{
    AFTER, NEXT, answered, edits, note, prompt_exactly, reopened, run_prompt, undo,
};
use crate::versions::matter_events;

const LATER: &str = "Then check the dates.";

/// Runs `text` to its end, which must be `EndTurn`.
fn ends(host: &Running, events: &mut Events, thread: ThreadId, text: &str) {
    let stop = run_prompt(host, events, thread, text);
    assert_eq!(stop, StopReason::EndTurn, "{}", events.dump());
}

/// Starts run `run` with the agent reopening its session to play `steps`.
fn run(dir: &Dir, run: u16, name: &str, steps: Vec<Value>) -> Running {
    dir.set_agents(&[(CLAUDE, reopened(dir, name, CLAUDE, steps))]);
    dir.start(run)
}

/// Shuts `host` down once its agent has played every step it expected.
fn finish(host: Running, mut events: Events) {
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}

/// Run 0 turns once (editing brief.txt); run 1 restores brief.txt before any prompt, so the
/// note holds it from memory and reads it from the matter's log too: the prompt after tells it
/// once, the next not at all, and neither does run 2's, though the clock stood still in run 1.
#[test]
fn a_restore_told_by_a_prompt_is_told_once_and_not_after_a_restart() {
    let dir = Dir::new("undo_note_restore_told", &[]);
    let first = written(
        &dir,
        "restore-told-0",
        "claude-handshake.jsonl",
        edits("p1", "brief.txt"),
    );
    dir.set_agents(&[(CLAUDE, first)]);
    let host = dir.start(0);
    let files = [("brief.txt", "draft\n")];
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    ends(&host, &mut events, thread, "Finish the brief.");
    finish(host, events);

    // Run 1's clock stands an hour after run 0's start (`Dir::clock`).
    let restored = "Note from Clauseo: The lawyer restored brief.txt from Versions; it's back as \
        of 2026-09-21 15:13 UTC. Read them again before relying on what you remember about them.";
    let mut steps = answered("p2", &[restored, NEXT]).to_vec();
    steps.extend(answered("p3", &[AFTER]));
    let host = run(&dir, 1, "restore-told-1", steps);
    let mut events = host.events(StreamId::Thread(thread));
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
    ends(&host, &mut events, thread, NEXT);
    ends(&host, &mut events, thread, AFTER);
    finish(host, events);

    let host = run(&dir, 2, "restore-told-2", answered("p4", &[LATER]).to_vec());
    let mut events = host.events(StreamId::Thread(thread));
    ends(&host, &mut events, thread, LATER);
    finish(host, events);
}

/// A new conversation, no prompt yet: brief.txt is changed outside Clauseo and restored, then
/// the host restarts. No turn has carried the note, so the restores since the conversation was
/// created are read from the matter's log: its first prompt tells the restore once.
#[test]
fn a_restore_before_a_new_conversation_s_first_prompt_survives_a_restart() {
    let dir = Dir::new("undo_note_restore_new", &[]);
    let idle = written(&dir, "restore-new-0", "claude-handshake.jsonl", Vec::new());
    dir.set_agents(&[(CLAUDE, idle)]);
    let host = dir.start(0);
    let files = [("brief.txt", "draft\n")];
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    crate::write::write(&folder.join("brief.txt"), b"changed\n");
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
    host.shutdown();

    // Run 0's clock stands at its start (`Dir::clock`).
    let restored = "Note from Clauseo: The lawyer restored brief.txt from Versions; it's back as \
        of 2026-09-21 14:13 UTC. Read them again before relying on what you remember about them.";
    let mut steps = answered("p1", &[restored, NEXT]).to_vec();
    steps.extend(answered("p2", &[AFTER]));
    let first = written(&dir, "restore-new-1", "claude-handshake.jsonl", steps);
    dir.set_agents(&[(CLAUDE, first)]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    ends(&host, &mut events, thread, NEXT);
    ends(&host, &mut events, thread, AFTER);
    finish(host, events);
}

/// Run 0: a turn edits brief.txt and is undone; the next prompt carries the note, the agent
/// sends `update` and the lawyer cancels. The prompt after carries the note again unless
/// `acted`, and so does the first prompt after a restart, by the rule the log is read by.
fn cancelled(test: &str, update: &Value, acted: bool) {
    let dir = Dir::new(test, &[]);
    let brief = note("brief.txt");
    let mut steps = edits("p1", "brief.txt");
    steps.extend([
        prompt_exactly("p2", &[&brief, "First try."]),
        json!({"update": {"update": update}}),
        json!({"expect": {"method": "session/cancel", "params": {"sessionId": "sess-1"}}}),
        json!({"respond": {"to": "p2", "result": {"stopReason": "cancelled"}}}),
    ]);
    let again: Vec<&str> = if acted {
        vec![NEXT]
    } else {
        vec![&brief, NEXT]
    };
    steps.extend(answered("p3", &again));
    let scenario = written(&dir, "undo-note-cancel", "claude-handshake.jsonl", steps);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let files = [("brief.txt", "draft\n")];
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (edited, _) = host.prompt(thread, "Finish the brief.");
    assert_eq!(events.until_turn_ended(edited).stop, StopReason::EndTurn);
    undo(&host, &mut events, thread, edited, "brief.txt");
    let (tried, _) = host.prompt(thread, "First try.");
    let heard = |e: &Events| {
        e.events().any(|event| match event {
            DomainEvent::ToolCallStarted(call) => call.turn == tried,
            DomainEvent::CommandsAvailable(_) => true,
            _ => false,
        })
    };
    events.until("the agent's update", heard);
    host.ok(HostCommand::Cancel { thread });
    assert_eq!(events.until_turn_ended(tried).stop, StopReason::Cancelled);
    ends(&host, &mut events, thread, NEXT);
    finish(host, events);

    // Run 1: the turn that took the note last was p3 either way, so it is not told again.
    let steps = answered("p4", &[AFTER]).to_vec();
    let host = run(&dir, 1, "undo-note-cancel-1", steps);
    let mut events = host.events(StreamId::Thread(thread));
    ends(&host, &mut events, thread, AFTER);
    finish(host, events);
}

#[test]
fn a_cancel_after_the_agent_acted_counts_the_note_as_told() {
    let update = json!({"sessionUpdate": "tool_call", "toolCallId": "t2-read",
        "title": "Read brief.txt", "kind": "read", "status": "pending"});
    cancelled("undo_note_cancel_acted", &update, true);
}

#[test]
fn a_cancel_while_the_agent_was_silent_keeps_the_note() {
    let update = json!({"sessionUpdate": "available_commands_update",
        "availableCommands": [{"name": "review", "description": "Review a paper"}]});
    cancelled("undo_note_cancel_silent", &update, false);
}
