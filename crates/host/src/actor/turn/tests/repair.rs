//! Checkpoint ids the matter's log lost: an after-turn checkpoint R8 made whose commit never
//! landed is recorded at the next start with the turn's changes, and a reused id the log does
//! not hold is recorded before a new turn names it. The start-up repair of a turn whose end
//! failed twice takes the turn's own after-turn checkpoint, or an empty step, never a
//! checkpoint or folder changes of another thread's turn that ran (or was left open) after it.

use clauseo_protocol::{
    CheckpointId, CheckpointKind, CheckpointRecorded, CommandOutcome, DomainEvent, ErrorCode,
    FileChange, FileChoice, FileDecision, HostCommand, MatterPath, ThreadId, TurnEnded, TurnId,
};

use super::failures::{end_failing, until_ended};
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::{TurnPhase, steps};

/// The `TurnEnded` of `turn` in `log`.
fn ended(log: &[DomainEvent], turn: TurnId) -> Option<TurnEnded> {
    log.iter().find_map(|event| {
        if let DomainEvent::TurnEnded(ended) = event
            && ended.turn == turn
        {
            Some(*ended)
        } else {
            None
        }
    })
}

/// The records of checkpoint `id` in the matter's log.
fn records_of(recorded: &[CheckpointRecorded], id: CheckpointId) -> usize {
    recorded.iter().filter(|r| r.checkpoint() == id).count()
}

/// A running turn of a new thread that writes brief.txt, whose end fails twice: R8 made the
/// after-turn checkpoint, the log never recorded it, and the turn is still open.
fn lost_after_turn(rig: &mut Rig) -> (ThreadId, TurnId) {
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    rig.write_file("brief.txt", "The lease ends in May.");
    end_failing(rig, thread, 2);
    assert!(ended(&rig.log(thread), turn).is_none(), "the turn is open");
    assert_eq!(rig.recorded().len(), 1, "only the import is recorded");
    (thread, turn)
}

/// The paths of every `FilesChanged` in `log`, and whether each is flagged as possibly holding
/// the lawyer's edits.
fn changed(log: &[DomainEvent]) -> Vec<(String, bool)> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::FilesChanged(changed) = event {
                Some(changed.files.clone())
            } else {
                None
            }
        })
        .flatten()
        .map(|file| {
            (
                file.path().as_str().to_owned(),
                file.may_include_lawyer_edits(),
            )
        })
        .collect()
}

/// A turn of a new thread (made by command 2) that takes the free writer at step 4 and writes
/// `name` while its `TurnStarted` commit is in flight; with no agent the prompt then fails at
/// once and the turn ends with its own after-turn checkpoint.
fn another_turn_writes(rig: &mut Rig, name: &str) -> (ThreadId, TurnId) {
    let second = rig.thread_by(2);
    let turn = rig.current_turn(second, TurnPhase::Queued);
    let matter = rig.matter();
    rig.actor.matters.get_mut(&matter).unwrap().turns.holder = Some(second);
    steps::before_turn(&mut rig.actor, second, turn);
    let started = |actor: &Actor, input: &Input| {
        matches!(input, Input::Committed { ticket, .. }
            if actor.pipeline.events(*ticket).iter()
                .any(|e| matches!(e.event, DomainEvent::TurnStarted(_))))
    };
    rig.hold_next("the second turn's TurnStarted commit", started);
    rig.write_file(name, "Signed by both parties.");
    rig.release_held();
    until_ended(rig, second);
    let log = rig.log(second);
    let end = ended(&log, turn).expect("the second turn ended");
    assert!(end.checkpoint_after.is_some());
    // No tool call named it, so it is flagged as possibly the lawyer's.
    assert_eq!(changed(&log), [(name.to_owned(), true)]);
    (second, turn)
}

#[test]
fn a_lost_after_turn_checkpoint_is_recorded_at_the_next_start() {
    let mut rig = Rig::start("turn-repair-lost-after", "");
    let (thread, turn) = lost_after_turn(&mut rig);

    let mut rig = rig.restart(1);
    let recorded = rig.recorded();
    let [_import, after] = recorded.as_slice() else {
        panic!("{recorded:?}");
    };
    assert_eq!(after.kind(), CheckpointKind::AfterTurn);
    assert_eq!((after.thread(), after.turn()), (Some(thread), Some(turn)));
    let log = rig.log(thread);
    let end = ended(&log, turn).unwrap();
    assert_eq!(end.checkpoint_after, Some(after.checkpoint()));
    let failed = log.iter().any(|event| {
        matches!(event, DomainEvent::TurnFailed(failed) if failed.code == ErrorCode::ThreadInterrupted)
    });
    assert!(failed);
    let files: Vec<_> = log
        .iter()
        .filter_map(|event| {
            if let DomainEvent::FilesChanged(changed) = event {
                Some(changed.files.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect();
    let paths: Vec<_> = files.iter().map(|file| file.path().as_str()).collect();
    assert_eq!(paths, ["brief.txt"]);
    assert!(files.iter().all(FileChange::may_include_lawyer_edits));

    // A third start finds the turn ended and logs nothing.
    let (thread_len, matter_len) = (log.len(), rig.matter_log().len());
    let mut rig = rig.restart(2);
    assert_eq!(rig.log(thread).len(), thread_len);
    assert_eq!(rig.matter_log().len(), matter_len);

    // The step can be undone.
    let undo = HostCommand::UndoStep {
        thread,
        turn,
        files: vec![FileDecision {
            path: MatterPath::new("brief.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    };
    let undone = rig.dispatch(20, undo).unwrap();
    assert!(
        matches!(undone, CommandOutcome::StepUndone { .. }),
        "{undone:?}"
    );
    assert!(!rig.scratch_file("brief.txt").exists());
    rig.finish();
}

#[test]
fn a_reused_checkpoint_the_log_lost_is_recorded_before_turn_started() {
    let mut rig = Rig::start("turn-reused-unknown", "");
    let (first, first_turn) = lost_after_turn(&mut rig);

    // Another thread's turn takes the free writer; R8 reuses the lost checkpoint at step 4.
    let second = rig.thread_by(2);
    let turn = rig.current_turn(second, TurnPhase::Queued);
    let matter = rig.matter();
    rig.actor.matters.get_mut(&matter).unwrap().turns.holder = Some(second);
    steps::before_turn(&mut rig.actor, second, turn);
    // With no agent the prompt fails at once and the turn ends.
    until_ended(&mut rig, second);

    let recorded = rig.recorded();
    let [_import, lost] = recorded.as_slice() else {
        panic!("{recorded:?}");
    };
    assert_eq!(lost.kind(), CheckpointKind::AfterTurn);
    assert_eq!(
        (lost.thread(), lost.turn()),
        (Some(first), Some(first_turn))
    );
    let log = rig.log(second);
    let started = log.iter().find_map(|event| {
        if let DomainEvent::TurnStarted(started) = event {
            Some(started.checkpoint_before)
        } else {
            None
        }
    });
    assert_eq!(started, Some(lost.checkpoint()));
    // Step 7 reuses it again and finds it recorded.
    assert_eq!(
        ended(&log, turn).and_then(|end| end.checkpoint_after),
        Some(lost.checkpoint())
    );
    assert_eq!(records_of(&recorded, lost.checkpoint()), 1);
    rig.finish();
}

#[test]
fn the_repair_takes_the_turns_own_checkpoint_after_another_threads_turn() {
    let mut rig = Rig::start("turn-repair-own-after", "");
    let (first, first_turn) = lost_after_turn(&mut rig);
    // The writer is free: another thread's turn in the matter changes notes.txt.
    let (second, second_turn) = another_turn_writes(&mut rig, "notes.txt");
    let recorded = rig.recorded();
    let own = recorded
        .iter()
        .find(|r| r.kind() == CheckpointKind::AfterTurn && r.turn() == Some(first_turn))
        .expect("the second turn's start recorded the first turn's checkpoint")
        .checkpoint();
    let second_after = ended(&rig.log(second), second_turn)
        .and_then(|end| end.checkpoint_after)
        .unwrap();
    assert_ne!(own, second_after);

    let mut rig = rig.restart(1);
    let log = rig.log(first);
    let end = ended(&log, first_turn).expect("the repair ended the turn");
    assert_eq!(end.checkpoint_after, Some(own), "the turn's own checkpoint");
    assert_eq!(changed(&log), [("brief.txt".to_owned(), true)]);
    assert_eq!(records_of(&rig.recorded(), own), 1);

    // Undoing the first turn leaves the second thread's file alone.
    let undo = HostCommand::UndoStep {
        thread: first,
        turn: first_turn,
        files: vec![FileDecision {
            path: MatterPath::new("brief.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    };
    let undone = rig.dispatch(20, undo).unwrap();
    assert!(
        matches!(undone, CommandOutcome::StepUndone { .. }),
        "{undone:?}"
    );
    assert!(!rig.scratch_file("brief.txt").exists());
    assert!(rig.scratch_file("notes.txt").exists());
    rig.finish();
}

/// A turn that changes nothing, so R8 reuses its before-turn checkpoint at its end, which fails
/// twice; then another thread's turn writes notes.txt. Returns the open turn, its before-turn
/// checkpoint and the other turn's after-turn checkpoint.
fn reused_end_then_another_turn(rig: &mut Rig) -> (ThreadId, TurnId, CheckpointId, CheckpointId) {
    let first = rig.thread();
    let first_turn = rig.running_turn(first);
    let before = rig.last_recorded();
    end_failing(rig, first, 2);
    assert!(
        ended(&rig.log(first), first_turn).is_none(),
        "the turn is open"
    );
    let (second, second_turn) = another_turn_writes(rig, "notes.txt");
    let second_after = ended(&rig.log(second), second_turn)
        .and_then(|end| end.checkpoint_after)
        .unwrap();
    (first, first_turn, before, second_after)
}

#[test]
fn a_reused_end_then_another_threads_turn_never_takes_its_checkpoint() {
    let mut rig = Rig::start("turn-repair-reused-after", "");
    let (first, first_turn, before, second_after) = reused_end_then_another_turn(&mut rig);

    let mut rig = rig.restart(1);
    let log = rig.log(first);
    let end = ended(&log, first_turn).expect("the repair ended the turn");
    assert_ne!(end.checkpoint_after, Some(second_after));
    assert_eq!(end.checkpoint_after, Some(before), "an empty step");
    assert_eq!(changed(&log), []);
    rig.finish();
}

#[test]
fn a_reused_end_then_another_turn_and_lawyer_edits_gives_an_empty_step() {
    let mut rig = Rig::start("turn-repair-reused-edited", "");
    let (first, first_turn, before, _) = reused_end_then_another_turn(&mut rig);
    // The lawyer edits the folder before the restart; no checkpoint holds it yet.
    rig.write_file("memo.txt", "Call the landlord.");
    let checkpoints = rig.recorded().len();

    let mut rig = rig.restart(1);
    let log = rig.log(first);
    let end = ended(&log, first_turn).expect("the repair ended the turn");
    assert_eq!(end.checkpoint_after, Some(before), "an empty step");
    assert_eq!(changed(&log), []);
    // The lawyer's edit is left for the next checkpoint to find as outside edits.
    assert_eq!(rig.recorded().len(), checkpoints);
    rig.finish();
}

#[test]
fn a_crash_before_the_after_turn_checkpoint_takes_one_at_the_next_start() {
    let mut rig = Rig::start("turn-repair-crash-before-after", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    let before = rig.last_recorded();
    rig.write_file("brief.txt", "The lease ends in May.");
    // The host stops mid-turn: nothing ends it, nothing else runs.
    rig.actor.threads.get_mut(&thread).unwrap().turn.current = None;

    let mut rig = rig.restart(1);
    let recorded = rig.recorded();
    let [_import, after] = recorded.as_slice() else {
        panic!("{recorded:?}");
    };
    assert_eq!(after.kind(), CheckpointKind::AfterTurn);
    assert_eq!((after.thread(), after.turn()), (Some(thread), Some(turn)));
    assert_ne!(after.checkpoint(), before);
    let log = rig.log(thread);
    let end = ended(&log, turn).expect("the repair ended the turn");
    assert_eq!(end.checkpoint_after, Some(after.checkpoint()));
    // No tool call named it, so the new checkpoint flags it.
    assert_eq!(changed(&log), [("brief.txt".to_owned(), true)]);
    rig.finish();
}

/// A's turn changes nothing and its end fails twice; then C, another thread, starts a turn from
/// the same before-turn checkpoint, writes notes.txt, and the host stops mid-turn. With
/// `c_thread_first`, C's thread is made first, so start-up looks at it first.
fn two_open_turns(c_thread_first: bool) -> (Rig, [(ThreadId, TurnId); 2], CheckpointId) {
    let name = if c_thread_first {
        "turn-repair-two-open-c-first"
    } else {
        "turn-repair-two-open-a-first"
    };
    let mut rig = Rig::start(name, "");
    let (a, c) = if c_thread_first {
        let c = rig.thread_by(2);
        (rig.thread(), c)
    } else {
        let a = rig.thread();
        (a, rig.thread_by(2))
    };
    let a_turn = rig.running_turn(a);
    let before = rig.last_recorded();
    end_failing(&mut rig, a, 2);
    assert!(ended(&rig.log(a), a_turn).is_none(), "A's turn is open");
    let c_turn = rig.running_turn(c);
    rig.write_file("notes.txt", "Signed by both parties.");
    // The host stops mid-turn: nothing ends C's turn.
    rig.actor.threads.get_mut(&c).unwrap().turn.current = None;
    let rig = rig.restart(1);
    (rig, [(a, a_turn), (c, c_turn)], before)
}

/// A gets an empty step; C, the turn that started last, gets the new checkpoint with notes.txt.
fn assert_two_open_turns_repaired(c_thread_first: bool) {
    let (mut rig, [(a, a_turn), (c, c_turn)], before) = two_open_turns(c_thread_first);
    let log = rig.log(a);
    let end = ended(&log, a_turn).expect("A's turn ended");
    assert_eq!(end.checkpoint_after, Some(before), "A's step is empty");
    assert_eq!(changed(&log), []);

    let recorded = rig.recorded();
    let [_import, after] = recorded.as_slice() else {
        panic!("{recorded:?}");
    };
    assert_eq!(after.kind(), CheckpointKind::AfterTurn);
    assert_eq!((after.thread(), after.turn()), (Some(c), Some(c_turn)));
    let log = rig.log(c);
    let end = ended(&log, c_turn).expect("C's turn ended");
    assert_eq!(end.checkpoint_after, Some(after.checkpoint()));
    assert_eq!(changed(&log), [("notes.txt".to_owned(), true)]);
    rig.finish();
}

#[test]
fn two_open_turns_the_earlier_thread_first_get_their_own_steps() {
    assert_two_open_turns_repaired(false);
}

#[test]
fn two_open_turns_the_later_thread_first_get_their_own_steps() {
    assert_two_open_turns_repaired(true);
}
