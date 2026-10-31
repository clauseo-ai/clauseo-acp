//! Checkpoint ids the matter's log lost: an after-turn checkpoint R8 made whose commit never
//! landed is recorded at the next start with the turn's changes, and a reused id the log does
//! not hold is recorded before a new turn names it.

use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, CommandOutcome, DomainEvent, ErrorCode, FileChange,
    FileChoice, FileDecision, HostCommand, MatterPath, ThreadId, TurnEnded, TurnId,
};

use super::failures::{end_failing, until_ended};
use super::rig::Rig;
use crate::actor::turn::{Phase, steps};

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
fn records_of(recorded: &[CheckpointRecorded], id: clauseo_protocol::CheckpointId) -> usize {
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
    let turn = rig.current_turn(second, Phase::Queued);
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
