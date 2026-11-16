//! The file actions a run leaves unfinished (`last_exit.rs`): an undo's record is durable before
//! R8 touches a file, a crash keeps it and the next start names it in `LastExit` (and clears
//! it, so a start after one with no host stop names nothing), a finished undo leaves none, and an
//! unclean exit with nothing recorded says only that. `unclean` is the desktop's flag alone: rows
//! after a clean quit are named with it false.

use clauseo_protocol::{
    CommandEnvelope, ExpiryReason, FileChoice, FileDecision, HostCommand, InterruptedAction,
    InterruptedKind, LastExit, MatterId, MatterPath, QueryResult, StopReason, ThreadId, TurnId,
};
use futures::channel::oneshot;

use super::failures::until_ended;
use super::rig::{Rig, command_id};
use crate::actor::input::{Input, Request};
use crate::actor::turn::{End, end};

/// A Scratch thread whose ended turn wrote `brief.txt`, so it has a step to undo.
fn step(rig: &mut Rig) -> (ThreadId, TurnId) {
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    rig.write_file("brief.txt", "The lease ends in May.");
    let done = End {
        stop: StopReason::EndTurn,
        failure: None,
        expiry: ExpiryReason::TurnEnded,
        detach: None,
        setting: None,
    };
    end::end_turn(&mut rig.actor, thread, done);
    until_ended(rig, thread);
    (thread, turn)
}

/// Sends `UndoStep` for `brief.txt`.
fn undo(
    rig: &mut Rig,
    thread: ThreadId,
    turn: TurnId,
) -> oneshot::Receiver<Result<clauseo_protocol::CommandOutcome, crate::error::Error>> {
    let command = CommandEnvelope {
        command_id: command_id(20),
        command: HostCommand::UndoStep {
            thread,
            turn,
            files: vec![FileDecision {
                path: MatterPath::new("brief.txt").unwrap(),
                choice: FileChoice::Undo,
            }],
        },
    };
    let (reply, answer) = oneshot::channel();
    rig.actor.on_request(Request::Dispatch { command, reply });
    answer
}

/// What `LastExit` answers now.
fn last_exit(rig: &mut Rig) -> LastExit {
    let (reply, mut answer) = oneshot::channel();
    crate::actor::last_exit::answer(&rig.actor, reply);
    match answer.try_recv() {
        Ok(Some(Ok(QueryResult::LastExit(view)))) => view,
        other => panic!("LastExit answered {other:?}"),
    }
}

fn pending(rig: &mut Rig) -> Vec<clauseo_store::PendingAction> {
    let store = rig.actor.store.clone();
    rig.run(async move { store.pending_actions().await })
        .unwrap()
}

#[test]
fn an_undo_is_recorded_before_r8_runs_and_named_after_a_crash() {
    let mut rig = Rig::start("interrupted-undo-crash", "");
    let (thread, turn) = step(&mut rig);
    let matter: MatterId = rig.matter();
    let _answer = undo(&mut rig, thread, turn);
    rig.hold_next("the undo's record", |_, input| {
        matches!(input, Input::Recorded(_))
    });
    // Durable, and no file touched yet: the job goes to R8 only once this input is read.
    let rows = pending(&mut rig);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, InterruptedKind::Undo);
    assert_eq!(rows[0].files.map(clauseo_protocol::Count::get), Some(1));
    assert_eq!(rows[0].thread, Some(thread));
    assert!(rig.scratch_file("brief.txt").exists(), "R8 has not run");

    // The process dies here: nothing after the record runs, so no clear either. The rig
    // stands in for the death by dropping the held input and letting go of the writer.
    rig.held.clear();
    rig.actor.work.matter_jobs -= 1;
    if let Some(entry) = rig.actor.matters.get_mut(&matter) {
        entry.turns.holder = None;
    }
    let mut rig = Rig::open_with(rig.close(), 1, None, |config| {
        config
            .with_idle_limit(std::time::Duration::MAX)
            .with_live_agents(usize::MAX)
            .with_last_run_unclean(true)
    });
    let view = last_exit(&mut rig);
    assert!(view.unclean, "the desktop said the last run did not quit");
    assert_eq!(
        view.interrupted,
        vec![InterruptedAction {
            kind: InterruptedKind::Undo,
            matter,
            thread: Some(thread),
            files: Some(clauseo_protocol::Count::new(1).unwrap()),
        }]
    );
    assert_eq!(pending(&mut rig), Vec::new(), "start-up cleared the record");
    // The start after names nothing.
    let mut rig = rig.restart(2);
    let view = last_exit(&mut rig);
    assert!(!view.unclean);
    assert_eq!(view.interrupted, Vec::new());
    rig.finish();
}

#[test]
fn a_clean_quit_with_a_row_names_it_and_is_not_unclean() {
    let mut rig = Rig::start("interrupted-clean-with-row", "");
    let matter: MatterId = rig.matter();
    let at = rig.actor.now().wall;
    let row = crate::actor::last_exit::action(matter, InterruptedKind::Restore, None, None, at);
    let store = rig.actor.store.clone();
    rig.run(async move { store.put_pending_action(row).await })
        .unwrap();
    // A restore still running at an idle Quit: the desktop's flag says clean, the row stays.
    let mut rig = rig.restart(1);
    let view = last_exit(&mut rig);
    assert!(
        !view.unclean,
        "the lawyer quit: not \"closed unexpectedly\""
    );
    assert_eq!(view.interrupted.len(), 1);
    assert_eq!(view.interrupted[0].kind, InterruptedKind::Restore);
    rig.finish();
}

/// A regression test of start-up's delete: a run whose host never stopped (`restart_unstopped`)
/// leaves nothing for the start after, since the start before it already cleared the rows.
#[test]
fn start_up_clears_the_rows_so_a_run_that_never_stopped_leaves_none() {
    let mut rig = Rig::start("interrupted-unstopped", "");
    let matter: MatterId = rig.matter();
    let at = rig.actor.now().wall;
    let row = crate::actor::last_exit::action(matter, InterruptedKind::Undo, Some(1), None, at);
    let store = rig.actor.store.clone();
    rig.run(async move { store.put_pending_action(row).await })
        .unwrap();
    // The start that names it, then a clean quit that does not stop the host (macOS Quit when
    // idle, a Windows logoff, a stop whose wait ran out).
    let mut rig = rig.restart(1);
    assert_eq!(last_exit(&mut rig).interrupted.len(), 1);
    let mut rig = rig.restart_unstopped(2);
    let view = last_exit(&mut rig);
    assert!(!view.unclean);
    assert_eq!(view.interrupted, Vec::new());
    assert_eq!(pending(&mut rig), Vec::new());
    rig.finish();
}

#[test]
fn a_finished_undo_leaves_no_record() {
    let mut rig = Rig::start("interrupted-undo-finished", "");
    let (thread, turn) = step(&mut rig);
    let mut answer = undo(&mut rig, thread, turn);
    rig.run_until("the undo's answer", |_| false_until_answered(&mut answer));
    rig.settle_commits();
    assert!(!rig.scratch_file("brief.txt").exists(), "R8 undid the file");
    // The writer passed on; its record's delete is a matter job until it comes back.
    rig.run_until("the record's delete", |actor| actor.work.matter_jobs == 0);
    assert_eq!(pending(&mut rig), Vec::new());
    let mut rig = rig.restart(1);
    let view = last_exit(&mut rig);
    assert!(!view.unclean);
    assert_eq!(view.interrupted, Vec::new());
    rig.finish();
}

/// Whether the command has answered, `Ok`.
fn false_until_answered(
    answer: &mut oneshot::Receiver<Result<clauseo_protocol::CommandOutcome, crate::error::Error>>,
) -> bool {
    match answer.try_recv() {
        Ok(Some(result)) => {
            assert!(result.is_ok(), "the undo failed: {result:?}");
            true
        }
        _ => false,
    }
}

#[test]
fn an_unclean_exit_with_nothing_recorded_says_only_that() {
    let rig = Rig::start("interrupted-generic", "");
    let dir = rig.close();
    let mut rig = Rig::open_with(dir, 1, None, |config| {
        config
            .with_idle_limit(std::time::Duration::MAX)
            .with_live_agents(usize::MAX)
            .with_last_run_unclean(true)
    });
    let view = last_exit(&mut rig);
    assert!(view.unclean);
    assert_eq!(view.interrupted, Vec::new());
    rig.finish();
}

#[test]
fn a_record_right_after_a_finish_waits_for_the_delete() {
    use crate::actor::{Actor, last_exit};
    use crate::error::Error;
    let mut rig = Rig::start("interrupted-delete-then-put", "");
    let matter: MatterId = rig.matter();
    let at = rig.actor.now().wall;
    let first = last_exit::action(matter, InterruptedKind::Undo, Some(1), None, at);
    last_exit::record(
        &mut rig.actor,
        first,
        Box::new(|_: &mut Actor, _: Result<(), Error>| {}),
    );
    rig.run_until("the first put", |actor| actor.work.matter_jobs == 0);
    // The writer passes on and the next action is recorded before the delete has come back:
    // the put is held, so the delete cannot land after it and lose the new row.
    last_exit::finished(&mut rig.actor, matter);
    let second = last_exit::action(matter, InterruptedKind::Restore, None, None, at);
    let ran = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = std::rc::Rc::clone(&ran);
    last_exit::record(
        &mut rig.actor,
        second,
        Box::new(move |_: &mut Actor, _: Result<(), Error>| seen.set(true)),
    );
    assert_eq!(
        rig.actor.work.matter_jobs, 1,
        "only the delete is on its way"
    );
    rig.run_until("the delete, then the put", |actor| {
        actor.work.matter_jobs == 0
    });
    assert!(ran.get(), "the second action went ahead");
    let rows = pending(&mut rig);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, InterruptedKind::Restore);
    last_exit::finished(&mut rig.actor, matter);
    rig.run_until("the last delete", |actor| actor.work.matter_jobs == 0);
    rig.finish();
}
