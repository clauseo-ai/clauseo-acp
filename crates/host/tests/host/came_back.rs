//! Papers an undo removed that come back (R8's `Undone.deleted`): a sync client puts a file
//! back after Clauseo's undo deleted it. The host looks 10 s and 60 s after the undo is logged
//! (host clock), logs one `Notice { kind: FilesCameBack }` per check that finds files, each file
//! once, and takes them out of the undo note the agent's next prompt opens with. The test plays
//! the sync client: it writes the file into the folder between `ManualClock::advance` calls, or
//! arms `bring_back_after_delete` for a file back before the undo's own checkpoint.
//!
//! A check stats on the matter's worker thread. To see that a check found nothing, the tests
//! pass [`barrier`]: two round trips through the actor (the due check is handled by then), then
//! a `CompareVersions` query, which the same worker answers after the check's stat.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_protocol::{
    CheckpointId, CommandOutcome, DomainEvent, FileChoice, FileDecision, HostCommand, HostQuery,
    MatterId, MatterPath, NoticeKind, NoticeLevel, StopReason, StreamId, ThreadId,
};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};
use crate::write;

/// The first check after the undo is logged (`came_back::FIRST_CHECK`).
const FIRST: Duration = Duration::from_secs(10);
/// The last check after the undo is logged (`came_back::LAST_CHECK`).
const LAST: Duration = Duration::from_secs(60);
/// From the first check to the last.
const BETWEEN: Duration = Duration::from_secs(50);
/// Just before the first check.
const JUST_BEFORE: Duration = Duration::from_millis(9_999);

/// A matter whose one turn's files were undone, with the checks armed.
struct Undone {
    // Before `dir`: a host still running when a test fails is aborted before its folder goes.
    host: Running,
    events: Events,
    matter: MatterId,
    thread: ThreadId,
    folder: PathBuf,
    /// The checkpoint the undo wrote.
    checkpoint: CheckpointId,
    dir: Dir,
}

/// Runs one turn of `scenario` in a matter seeded with `seed`, then undoes `undo` of it;
/// `before_undo` gets the folder just before the undo. Returns once both checks are armed.
fn undone(
    test: &str,
    scenario: &'static str,
    seed: &[(&str, &str)],
    undo: &[&str],
    before_undo: impl FnOnce(&Path),
) -> Undone {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (matter, thread, folder) = matter_with_thread(&dir, &host, "m", seed, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Write the notes.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    before_undo(&folder);
    let outcome = host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: undo
            .iter()
            .map(|path| FileDecision {
                path: path_of(path),
                choice: FileChoice::Undo,
            })
            .collect(),
    });
    assert!(
        matches!(outcome, CommandOutcome::StepUndone { .. }),
        "{outcome:?}"
    );
    events.until("StepUndone", |e| {
        e.position(|event| matches!(event, DomainEvent::StepUndone(_)))
            .is_some()
    });
    let checkpoint = events
        .events()
        .find_map(|event| match event {
            DomainEvent::StepUndone(undone) => Some(undone.checkpoint),
            _ => None,
        })
        .unwrap();
    // The last check is armed right after the first, once the undo's commit is durable.
    host.wait_armed(LAST);
    Undone {
        host,
        events,
        matter,
        thread,
        folder,
        checkpoint,
        dir,
    }
}

fn path_of(path: &str) -> MatterPath {
    MatterPath::new(path).unwrap()
}

/// The `FilesCameBack` notices logged so far: (files, more, text), each checked to be `Info`.
fn came_back(events: &Events) -> Vec<(Vec<MatterPath>, u32, String)> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => match &notice.kind {
                Some(NoticeKind::FilesCameBack { files, more }) => {
                    assert_eq!(notice.level, NoticeLevel::Info, "{notice:?}");
                    Some((files.clone(), *more, notice.text.clone()))
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Waits until `n` came-back notices are in.
fn until_notices(events: &mut Events, n: usize) {
    events.until(&format!("{n} came-back notices"), |e| {
        came_back(e).len() >= n
    });
}

/// After it, a check that was due has had its stat answered (see the module doc).
fn barrier(host: &Running, matter: MatterId, checkpoint: CheckpointId) {
    host.settle();
    host.settle();
    host.query(HostQuery::CompareVersions {
        matter,
        from: checkpoint,
        to: Some(checkpoint),
    })
    .unwrap();
    host.settle();
}

impl Undone {
    /// Moves the clock by `by`, waits out the check it makes due, and reads what was logged.
    fn advance(&mut self, by: Duration) {
        self.host.clock.advance(by);
        barrier(&self.host, self.matter, self.checkpoint);
        self.events.drain_ready();
    }

    /// The sync client: writes `path` into the matter folder again.
    fn put_back(&self, path: &str) {
        write::write(&self.folder.join(path), b"put back by the sync client\n");
    }

    /// The next prompt, whose undo note the scenario checks word for word.
    fn prompt_again(&mut self) {
        let (turn, _) = self.host.prompt(self.thread, "Carry on.");
        let ended = self.events.until_turn_ended(turn);
        self.host.settle();
        self.events.drain_ready();
        self.events.assert_no_mismatch();
        assert_eq!(ended.stop, StopReason::EndTurn, "{}", self.events.dump());
    }

    fn shutdown(self) {
        let Self { host, dir, .. } = self;
        host.shutdown();
        drop(dir);
    }
}

const ONE: &str = "1 paper came back after the undo. Clauseo didn't remove it again.";
const NOTES: [(&str, &str); 1] = [("brief.txt", "Clause 4 draft\n")];
const BOTH: [&str; 2] = ["notes/a.txt", "notes/b.txt"];

#[test]
fn came_back_by_the_first_check_is_told_once_and_left_out_of_the_note() {
    let mut undone = undone(
        "came_back_first_check",
        "undo-came-back",
        &NOTES,
        &BOTH,
        |_| {},
    );
    assert!(!undone.folder.join("notes/a.txt").exists());
    assert!(!undone.folder.join("notes/b.txt").exists());
    undone.put_back("notes/a.txt");
    undone.host.settle();
    undone.events.drain_ready();
    assert_eq!(came_back(&undone.events), [], "a notice before 10 s");

    undone.host.clock.advance(FIRST);
    until_notices(&mut undone.events, 1);
    let notices = came_back(&undone.events);
    assert_eq!(
        notices,
        [(vec![path_of("notes/a.txt")], 0, ONE.to_owned())],
        "{}",
        undone.events.dump()
    );
    assert!(
        undone.folder.join("notes/a.txt").exists(),
        "the file that came back was removed again"
    );
    // The last check finds notes/a.txt reported already and notes/b.txt still gone.
    undone.advance(BETWEEN);
    assert_eq!(
        came_back(&undone.events).len(),
        1,
        "{}",
        undone.events.dump()
    );
    // The scenario checks the note names notes/b.txt only.
    undone.prompt_again();
    undone.shutdown();
}

#[test]
fn came_back_before_the_undo_checkpoint_is_told_at_the_first_check() {
    let mut undone = undone(
        "came_back_already_back",
        "undo-came-back",
        &NOTES,
        &BOTH,
        |folder| clauseo_matters::test_support::bring_back_after_delete(folder.join("notes/a.txt")),
    );
    assert!(
        undone.folder.join("notes/a.txt").exists(),
        "the hook did not bring the file back"
    );
    assert!(!undone.folder.join("notes/b.txt").exists());
    undone.host.settle();
    undone.events.drain_ready();
    assert_eq!(came_back(&undone.events), [], "a notice before 10 s");
    undone.advance(JUST_BEFORE);
    assert_eq!(came_back(&undone.events), [], "a notice before 10 s");

    undone.host.clock.advance(Duration::from_millis(1));
    until_notices(&mut undone.events, 1);
    assert_eq!(
        came_back(&undone.events),
        [(vec![path_of("notes/a.txt")], 0, ONE.to_owned())]
    );
    undone.prompt_again();
    undone.shutdown();
}

#[test]
fn came_back_by_the_last_check_is_told_then_the_watch_ends() {
    let mut undone = undone(
        "came_back_last_check",
        "undo-came-back",
        &NOTES,
        &BOTH,
        |_| {},
    );
    undone.advance(FIRST);
    assert_eq!(came_back(&undone.events), [], "nothing was back at 10 s");

    undone.put_back("notes/a.txt");
    undone.host.clock.advance(BETWEEN);
    until_notices(&mut undone.events, 1);
    assert_eq!(
        came_back(&undone.events),
        [(vec![path_of("notes/a.txt")], 0, ONE.to_owned())]
    );

    // After the last check nothing is looked for.
    undone.put_back("notes/b.txt");
    undone.advance(Duration::from_secs(600));
    assert_eq!(
        came_back(&undone.events).len(),
        1,
        "{}",
        undone.events.dump()
    );
    undone.prompt_again();
    undone.shutdown();
}

#[test]
fn came_back_after_a_reversed_rename_names_the_new_name() {
    let mut undone = undone(
        "came_back_rename",
        "undo-came-back-rename",
        &NOTES,
        &["final.txt"],
        |folder| {
            assert!(folder.join("final.txt").exists());
            assert!(!folder.join("brief.txt").exists());
        },
    );
    assert!(undone.folder.join("brief.txt").exists(), "the rename stood");
    assert!(
        !undone.folder.join("final.txt").exists(),
        "the rename stood"
    );
    undone.put_back("final.txt");
    undone.host.clock.advance(FIRST);
    until_notices(&mut undone.events, 1);
    assert_eq!(
        came_back(&undone.events),
        [(vec![path_of("final.txt")], 0, ONE.to_owned())]
    );
    undone.shutdown();
}

#[test]
fn came_back_names_five_and_counts_the_rest() {
    const SEVEN: [&str; 7] = [
        "notes/g.txt",
        "notes/f.txt",
        "notes/e.txt",
        "notes/d.txt",
        "notes/c.txt",
        "notes/b.txt",
        "notes/a.txt",
    ];
    let mut undone = undone(
        "came_back_cap",
        "undo-came-back-seven",
        &NOTES,
        &SEVEN,
        |_| {},
    );
    for path in SEVEN {
        undone.put_back(path);
    }
    undone.host.clock.advance(FIRST);
    until_notices(&mut undone.events, 1);
    let first_five = [
        "notes/a.txt",
        "notes/b.txt",
        "notes/c.txt",
        "notes/d.txt",
        "notes/e.txt",
    ];
    assert_eq!(
        came_back(&undone.events),
        [(
            first_five.map(path_of).to_vec(),
            2,
            "7 papers came back after the undo. Clauseo didn't remove them again.".to_owned()
        )]
    );
    undone.advance(BETWEEN);
    assert_eq!(
        came_back(&undone.events).len(),
        1,
        "{}",
        undone.events.dump()
    );
    undone.shutdown();
}

#[test]
fn came_back_none_logs_nothing_and_keeps_the_note() {
    let mut undone = undone(
        "came_back_none",
        "undo-came-back-none",
        &NOTES,
        &BOTH,
        |_| {},
    );
    undone.advance(FIRST);
    undone.advance(BETWEEN);
    undone.advance(Duration::from_secs(600));
    assert_eq!(came_back(&undone.events), [], "{}", undone.events.dump());
    // The scenario checks the note still names both files.
    undone.prompt_again();
    undone.shutdown();
}

#[test]
fn came_back_checks_end_with_shutdown() {
    let undone = undone(
        "came_back_shutdown",
        "undo-came-back",
        &NOTES,
        &BOTH,
        |_| {},
    );
    undone.put_back("notes/a.txt");
    let Undone {
        host,
        mut events,
        thread,
        dir,
        ..
    } = undone;
    // Returns with both checks armed and never due.
    host.shutdown();
    events.drain_ready();
    assert_eq!(came_back(&events), [], "{}", events.dump());

    // Nothing is looked for after a restart either: the watch lived in memory only.
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    host.clock.advance(Duration::from_secs(600));
    host.settle();
    host.settle();
    events.drain_ready();
    assert_eq!(came_back(&events), [], "{}", events.dump());
    host.shutdown();
    drop(dir);
}
