//! Turns of two threads of one matter against a restore holding the matter: both wait for it
//! with `TurnQueued` behind their own threads, held by the restore, and both start, together,
//! when it ends; a cancel of one while it waits ends it once; an abort of the host keeps a
//! waiting turn queued.

use clauseo_protocol::{
    CommandEnvelope, CommandOutcome, DomainEvent, HostCommand, MatterHold, PromptBlock,
    RestoreRequest, StopReason, ThreadId, TurnId,
};
use futures::channel::oneshot;

use super::rig::{Rig, command_id, kind};
use crate::actor::input::{Input, MatterTag, Request};
use crate::actor::turn::{Holder, TurnPhase};
use crate::error::Error;

/// Every prompt ends at once; a new agent plays it from the start.
const ANSWER: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
"#;

type Answer = oneshot::Receiver<Result<CommandOutcome, Error>>;

fn send_prompt(rig: &mut Rig, n: u128, thread: ThreadId) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Read the brief.".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
    };
    let accepted = rig.dispatch(n, command).unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = accepted else {
        panic!("not accepted: {accepted:?}");
    };
    turn
}

/// A `RestoreVersion` of the matter's first version, sent as command 20 after a file was
/// added, with its matter job's reply held back: the restore holds the matter.
fn hold_a_restore(rig: &mut Rig) -> Answer {
    rig.write_file("brief.txt", "The lease ends in June.");
    let command = HostCommand::RestoreVersion {
        thread: None,
        undoes: None,
        matter: rig.matter(),
        checkpoint: rig.last_recorded(),
        scope: RestoreRequest::Matter,
    };
    let (reply, answer) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(20),
        command,
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.hold_next("the restore's reply", |_, input| {
        matches!(
            input,
            Input::Matter {
                tag: MatterTag::Restore(_),
                ..
            }
        )
    });
    let matter = rig.matter();
    assert_eq!(
        rig.actor.matters[&matter].turns.holder,
        Some(Holder::Restore)
    );
    answer
}

/// Handles inputs until each of `threads` has its turn waiting for the matter.
fn until_queued(rig: &mut Rig, threads: &[ThreadId]) {
    rig.run_until("the turns queued", |actor| {
        threads.iter().all(|thread| {
            actor.threads[thread]
                .turn
                .current
                .as_ref()
                .is_some_and(|current| current.phase == TurnPhase::Queued)
        }) && actor.work.commits == 0
    });
}

/// `turn`'s `TurnQueued` threads, in the order logged.
fn queued_behind(log: &[DomainEvent], turn: TurnId) -> Vec<ThreadId> {
    let behind = |event: &DomainEvent| {
        if let DomainEvent::TurnQueued(queued) = event
            && queued.turn() == turn
        {
            return Some(queued.behind());
        }
        None
    };
    log.iter().filter_map(behind).collect()
}

/// The hold of each `TurnQueued` of `turn` in `log`, in order.
fn holds(log: &[DomainEvent], turn: TurnId) -> Vec<Option<MatterHold>> {
    let hold = |event: &DomainEvent| {
        if let DomainEvent::TurnQueued(queued) = event
            && queued.turn() == turn
        {
            return Some(queued.hold());
        }
        None
    };
    log.iter().filter_map(hold).collect()
}

fn started(log: &[DomainEvent], turn: TurnId) -> bool {
    log.iter()
        .any(|event| matches!(event, DomainEvent::TurnStarted(started) if started.turn == turn))
}

fn ends(log: &[DomainEvent], turn: TurnId) -> Vec<StopReason> {
    let stop = |event: &DomainEvent| {
        if let DomainEvent::TurnEnded(ended) = event
            && ended.turn == turn
        {
            return Some(ended.stop);
        }
        None
    };
    log.iter().filter_map(stop).collect()
}

#[test]
fn turns_waiting_for_a_restore_all_start_together_when_it_ends() {
    let mut rig = Rig::start("together-restore", ANSWER);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    let matter = rig.matter();
    let mut answer = hold_a_restore(&mut rig);
    let turn_a = send_prompt(&mut rig, 10, first);
    let turn_b = send_prompt(&mut rig, 11, second);
    until_queued(&mut rig, &[first, second]);
    for (thread, turn) in [(first, turn_a), (second, turn_b)] {
        let log = rig.log(thread);
        assert_eq!(queued_behind(&log, turn), [thread], "behind its own thread");
        assert_eq!(
            holds(&log, turn),
            [Some(MatterHold::Restore)],
            "held by the restore"
        );
        assert!(!started(&log, turn));
    }

    // The restore ends: both turns start at once, side by side.
    rig.release_held();
    let mut together = 0;
    rig.run_until("both turns ended", |actor| {
        together = together.max(actor.matters[&matter].turns.running.len());
        [first, second].iter().all(|thread| {
            let state = &actor.threads[thread].turn;
            state.current.is_none() && state.closing.is_none()
        }) && actor.work.commits == 0
            && actor.work.matter_jobs == 0
    });
    assert!(
        matches!(answer.try_recv(), Ok(Some(Ok(_)))),
        "the restore is answered"
    );
    assert_eq!(together, 2, "the two turns did not run side by side");
    for (thread, turn) in [(first, turn_a), (second, turn_b)] {
        let log = rig.log(thread);
        let kinds = log.iter().map(kind).collect::<Vec<_>>();
        // No second `TurnQueued` behind the other thread's turn.
        assert_eq!(queued_behind(&log, turn), [thread], "{kinds:?}");
        assert!(started(&log, turn), "{kinds:?}");
        assert_eq!(ends(&log, turn), [StopReason::EndTurn], "{kinds:?}");
    }
    let turns = &rig.actor.matters[&matter].turns;
    assert!(turns.holder.is_none() && turns.running.is_empty() && turns.queue.is_empty());
    rig.finish();
}

#[test]
fn a_turn_cancelled_while_it_waits_for_a_restore_ends_once() {
    let mut rig = Rig::start("together-restore-cancel", ANSWER);
    let (first, second) = (rig.thread(), rig.thread_by(2));
    let matter = rig.matter();
    let _answer = hold_a_restore(&mut rig);
    let turn_a = send_prompt(&mut rig, 10, first);
    let turn_b = send_prompt(&mut rig, 11, second);
    until_queued(&mut rig, &[first, second]);

    let cancelled = rig.dispatch(12, HostCommand::Cancel { thread: second });
    assert_eq!(cancelled.unwrap(), CommandOutcome::Done);
    rig.run_until("the cancelled turn ended", |actor| {
        let state = &actor.threads[&second].turn;
        state.current.is_none() && state.closing.is_none() && actor.work.commits == 0
    });
    assert_eq!(
        rig.actor.matters[&matter].turns.queue,
        [first],
        "the cancelled turn left the queue"
    );

    rig.release_held();
    rig.run_until("the first turn ended", |actor| {
        let state = &actor.threads[&first].turn;
        state.current.is_none()
            && state.closing.is_none()
            && actor.work.commits == 0
            && actor.work.matter_jobs == 0
    });
    let log = rig.log(second);
    let kinds = log.iter().map(kind).collect::<Vec<_>>();
    assert!(!started(&log, turn_b), "{kinds:?}");
    assert_eq!(ends(&log, turn_b), [StopReason::Cancelled], "{kinds:?}");
    assert_eq!(ends(&rig.log(first), turn_a), [StopReason::EndTurn]);
    rig.finish();
}

#[test]
fn a_turn_waiting_for_a_restore_stays_queued_across_a_stop() {
    let mut rig = Rig::start("together-restore-restart", ANSWER);
    let thread = rig.thread();
    let _answer = hold_a_restore(&mut rig);
    let turn = send_prompt(&mut rig, 10, thread);
    until_queued(&mut rig, &[thread]);

    // The process ends here, as an abort ends it, with the restore's reply never read: the
    // turn's `TurnQueued` is durable and it has no `TurnStarted`.
    rig.held.clear();
    let mut rig = rig.restart_unstopped(1);
    let log = rig.log(thread);
    let kinds = log.iter().map(kind).collect::<Vec<_>>();
    // As before turns of a matter ran together: still queued behind its own thread, behind no
    // turn, not started, not ended, and the thread idle.
    assert_eq!(queued_behind(&log, turn), [thread], "{kinds:?}");
    let last = log.iter().rev().find_map(|event| {
        if let DomainEvent::TurnQueued(queued) = event
            && queued.turn() == turn
        {
            return Some(queued.behind_turn());
        }
        None
    });
    assert_eq!(last, Some(None), "{kinds:?}");
    assert!(!started(&log, turn), "{kinds:?}");
    assert!(
        ends(&log, turn).is_empty(),
        "a queued turn is never ended: {kinds:?}"
    );
    assert_eq!(
        super::idle::status(&rig.actor, thread),
        Some(clauseo_protocol::ThreadStatus::Idle)
    );
    let turns = &rig.actor.matters[&rig.matter()].turns;
    assert!(turns.holder.is_none() && turns.running.is_empty());
    rig.finish();
}
