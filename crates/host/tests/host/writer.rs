//! Conversations in one matter answer at the same time: a second thread's turn runs while the
//! first's waits on a permission, with no `TurnQueued`; undo, restore, relocate and forget are
//! refused while any turn runs; a turn's before-turn checkpoint leaves the papers a running
//! turn wrote out of its outside edits; a crash while two turns run ends both at the next start.
//! A turn waiting for a restore or an undo is tested in the unit rig (`turn/tests/together.rs`).

use clauseo_protocol::{
    CheckpointKind, DomainEvent, ErrorCode, FileChoice, FileDecision, HostCommand, MatterId,
    MatterPath, RestoreRequest, StopReason, StreamId, ThreadId, TurnId,
};

use crate::observe::{Events, started};
use crate::permissions::{KINDS_ANSWERS, PERM, answer_from, until_first_request};
use crate::support::{Dir, FAKE, Running, Scenario};
use crate::versions::matter_events;
use crate::write;

/// An agent that writes `notes/a.txt` and then waits on `perm-1`.
const WRITES: &str = "writes-agent";

/// Thread A (`permission-kinds`) waits at `perm-1`; thread B (`prompt-stream`) in the same
/// matter is made, its prompt not sent.
struct TwoThreads {
    matter: MatterId,
    folder: std::path::PathBuf,
    a: ThreadId,
    b: ThreadId,
    turn_a: TurnId,
    events_a: Events,
    events_b: Events,
}

fn dir(test: &str) -> Dir {
    Dir::new(
        test,
        &[
            (PERM, Scenario::Bundled("permission-kinds")),
            (FAKE, Scenario::Bundled("prompt-stream")),
            (WRITES, Scenario::Bundled("write-then-wait")),
        ],
    )
}

/// Builds [`TwoThreads`], A playing `agent`.
fn a_waits(dir: &Dir, host: &Running, agent: &str) -> TwoThreads {
    let folder = dir.folder("m", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("m", &folder);
    let a = host.create_thread(matter, agent);
    let b = host.create_thread(matter, FAKE);
    let mut events_a = host.events(StreamId::Thread(a));
    let events_b = host.events(StreamId::Thread(b));
    let (turn_a, _) = host.prompt(a, "Edit the four clauses.");
    until_first_request(&mut events_a);
    TwoThreads {
        matter,
        folder,
        a,
        b,
        turn_a,
        events_a,
        events_b,
    }
}

fn queued(events: &Events, turn: TurnId) -> usize {
    events.count(|event| matches!(event, DomainEvent::TurnQueued(q) if q.turn() == turn))
}

#[test]
fn two_conversations_answer_at_the_same_time() {
    let dir = dir("two_conversations_answer_at_the_same_time");
    let host = dir.start(0);
    let mut t = a_waits(&dir, &host, PERM);

    // B's whole turn runs while A waits on its permission.
    let (turn_b, _) = host.prompt(t.b, "Summarise the indemnity clause.");
    let ended_b = t.events_b.until_turn_ended(turn_b);
    assert_eq!(ended_b.stop, StopReason::EndTurn, "{}", t.events_b.dump());
    assert_eq!(t.events_b.count(|event| started(event, turn_b)), 1);
    assert_eq!(queued(&t.events_b, turn_b), 0, "{}", t.events_b.dump());
    t.events_a.drain_ready();
    assert!(t.events_a.turn_ended(t.turn_a).is_none(), "A still waits");

    answer_from(&host, &mut t.events_a, t.a, 0, &KINDS_ANSWERS);
    let ended_a = t.events_a.until_turn_ended(t.turn_a);
    assert_eq!(ended_a.stop, StopReason::EndTurn);
    assert_eq!(queued(&t.events_a, t.turn_a), 0, "{}", t.events_a.dump());
    t.events_a.assert_no_mismatch();
    t.events_b.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn undo_restore_relocate_and_forget_wait_for_every_running_turn() {
    let dir = dir("undo_restore_relocate_and_forget_wait");
    let host = dir.start(0);
    let mut t = a_waits(&dir, &host, PERM);
    let (turn_b, _) = host.prompt(t.b, "Summarise the indemnity clause.");
    t.events_b.until_turn_ended(turn_b);
    let import = matter_events(&host, t.matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    let head = matter_events(&host, t.matter).head();

    // B's turn has ended, but A's still runs in the matter.
    let undo = HostCommand::UndoStep {
        thread: t.b,
        turn: turn_b,
        files: vec![FileDecision {
            path: MatterPath::new("brief.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    };
    host.refused(undo, ErrorCode::MatterTurnRunning);
    let restore = HostCommand::RestoreVersion {
        thread: Some(t.b),
        undoes: None,
        matter: t.matter,
        checkpoint: import,
        scope: RestoreRequest::Matter,
    };
    let error = host.refused(restore, ErrorCode::MatterTurnRunning);
    assert!(error.to_string().contains("a turn is running"), "{error}");
    let relocate = HostCommand::RelocateMatter {
        matter: t.matter,
        folder: host.folder_ref(&t.folder),
    };
    host.refused(relocate, ErrorCode::MatterTurnRunning);
    let forget = HostCommand::ForgetMatter { matter: t.matter };
    host.refused(forget, ErrorCode::MatterTurnRunning);
    assert_eq!(
        matter_events(&host, t.matter).head(),
        head,
        "a refused command logged"
    );

    answer_from(&host, &mut t.events_a, t.a, 0, &KINDS_ANSWERS);
    t.events_a.until_turn_ended(t.turn_a);
    t.events_a.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn a_running_turns_papers_are_not_the_next_turns_outside_edits() {
    let dir = dir("a_running_turns_papers_are_not_outside_edits");
    let host = dir.start(0);
    // A has written notes/a.txt, and waits.
    let mut t = a_waits(&dir, &host, WRITES);
    write::write(&t.folder.join("lawyer.txt"), b"Call the client.\n");

    let (turn_b, _) = host.prompt(t.b, "Summarise the indemnity clause.");
    let ended_b = t.events_b.until_turn_ended(turn_b);
    assert_eq!(ended_b.stop, StopReason::EndTurn, "{}", t.events_b.dump());
    host.settle();
    let log = matter_events(&host, t.matter);
    let outside: Vec<Vec<&str>> = log
        .events()
        .filter_map(|event| match event {
            DomainEvent::OutsideEdits(edits) => {
                Some(edits.files.iter().map(|f| f.path().as_str()).collect())
            }
            _ => None,
        })
        .collect();
    assert_eq!(outside, [["lawyer.txt"]], "{}", log.dump());

    // notes/a.txt is A's: on its card, measured from A's own start, though B's before-turn
    // checkpoint already holds it.
    answer_from(&host, &mut t.events_a, t.a, 0, &["allow-once"]);
    let ended_a = t.events_a.until_turn_ended(t.turn_a);
    assert_eq!(ended_a.stop, StopReason::EndTurn, "{}", t.events_a.dump());
    let changed = t
        .events_a
        .files_changed(t.turn_a)
        .unwrap_or_else(|| panic!("no FilesChanged: {}", t.events_a.dump()));
    let note = changed
        .files
        .iter()
        .find(|file| file.path().as_str() == "notes/a.txt")
        .unwrap_or_else(|| panic!("notes/a.txt is not A's: {changed:?}"));
    assert!(!note.may_include_lawyer_edits());
    assert_eq!(note.also_changed_by(), []);
    t.events_a.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn a_crash_while_two_turns_run_ends_both_at_the_next_start() {
    let dir = dir("a_crash_while_two_turns_run");
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("m", &folder);
    let threads = [
        host.create_thread(matter, PERM),
        host.create_thread(matter, PERM),
    ];
    let mut turns = Vec::new();
    for thread in threads {
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Edit the four clauses.");
        until_first_request(&mut events);
        assert_eq!(queued(&events, turn), 0, "{}", events.dump());
        turns.push(turn);
    }
    host.abort();

    let host = dir.start(1);
    for (thread, turn) in threads.into_iter().zip(turns) {
        let mut events = host.events(StreamId::Thread(thread));
        events.until_caught_up();
        let ended = events
            .turn_ended(turn)
            .unwrap_or_else(|| panic!("not ended: {}", events.dump()));
        assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    }
    host.shutdown();
}
