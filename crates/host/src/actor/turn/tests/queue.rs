//! The queue against the thread's undo and a failed remove: a queue resumed while the undo holds
//! the matter's writer starts only once the undo is logged, and a remove whose commit fails keeps
//! the turn queued.

use clauseo_protocol::{
    CommandEnvelope, CommandOutcome, DomainEvent, FileChoice, FileDecision, HostCommand,
    MatterPath, PromptBlock, TurnId,
};
use futures::channel::oneshot;

use super::failures::until_ended;
use super::rig::{Rig, command_id};
use crate::actor::Actor;
use crate::actor::input::{Input, Request};
use crate::actor::turn::{End, Holder, TurnPhase, end, queue};

/// Queues a prompt behind the running turn of `thread` as command `n`; returns its turn.
fn queue_prompt(rig: &mut Rig, n: u128, thread: clauseo_protocol::ThreadId) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "And clause 5?".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
    };
    match rig.dispatch(n, command) {
        Ok(CommandOutcome::PromptAccepted { turn, .. }) => turn,
        other => panic!("SendPrompt answered {other:?}"),
    }
}

/// The agent's steps after its handshake: one prompt, answered `end_turn`.
const PROMPT_ENDS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// Whether `input` is the result of a commit holding an event `pick` chooses.
fn result_of(actor: &Actor, input: &Input, pick: impl Fn(&DomainEvent) -> bool) -> bool {
    matches!(input, Input::Committed { ticket, .. }
        if actor.pipeline.events(*ticket).iter().any(|e| pick(&e.event)))
}

#[test]
fn a_queue_resumed_during_the_threads_undo_starts_after_it() {
    let mut rig = Rig::start("queue-undo-resume", PROMPT_ENDS);
    let thread = rig.thread();
    let first = rig.running_turn(thread);
    let queued = queue_prompt(&mut rig, 30, thread);
    rig.write_file("brief.txt", "The lease ends in May.");
    end::end_turn(&mut rig.actor, thread, End::cancelled(None));
    until_ended(&mut rig, thread);
    rig.settle_commits();
    assert!(
        queue::queue_held(&rig.actor, thread),
        "a cancel holds the queue"
    );
    let matter = rig.matter();

    // The undo holds the writer until its `StepUndone` is durable; that result is held back.
    let undo = HostCommand::UndoStep {
        thread,
        turn: first,
        files: vec![FileDecision {
            path: MatterPath::new("brief.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    };
    let (reply, mut answer) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(31),
        command: undo,
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.hold_next("the undo's result", |actor, input| {
        result_of(actor, input, |event| {
            matches!(event, DomainEvent::StepUndone(_))
        })
    });
    assert_eq!(
        rig.actor.matters[&matter].turns.holder,
        Some(Holder::Thread(thread))
    );

    // Resumed meanwhile (its receipt is answered once the undo's commit, ahead of it, returns):
    // nothing starts while the undo holds the writer.
    let (reply, mut resumed) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(32),
        command: HostCommand::ResumeQueue { thread },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.run_holding(
        "the resume ran",
        |_, input| matches!(input, Input::Committed { .. }),
        |actor| !queue::queue_held(actor, thread),
    );
    assert!(rig.actor.threads[&thread].turn.current.is_none());
    assert_eq!(queue::queued_turns(&rig.actor, thread), [queued]);

    // Once the undo is logged the queued turn starts, and while it runs the writer is its own.
    rig.release_held();
    let mut wrong_holder = None;
    rig.run_until("the queued turn ended", |actor| {
        let state = &actor.threads[&thread].turn;
        if let Some(current) = state.current.as_deref()
            && matches!(
                current.phase,
                TurnPhase::Checkpointing | TurnPhase::Starting | TurnPhase::Prompting
            )
            && actor.matters[&matter].turns.holder != Some(Holder::Thread(thread))
        {
            wrong_holder = Some(actor.matters[&matter].turns.holder);
        }
        state.current.is_none()
            && state.closing.is_none()
            && queue::queued_turns(actor, thread).is_empty()
    });
    assert!(
        wrong_holder.is_none(),
        "the writer was not the turn's: {wrong_holder:?}"
    );
    assert!(
        matches!(resumed.try_recv(), Ok(Some(Ok(CommandOutcome::Done)))),
        "the resume is answered"
    );
    assert!(
        matches!(answer.try_recv(), Ok(Some(Ok(_)))),
        "the undo is answered"
    );
    let log = rig.log(thread);
    let undone = log
        .iter()
        .position(|event| matches!(event, DomainEvent::StepUndone(_)))
        .unwrap();
    let ran = log
        .iter()
        .position(|event| {
            matches!(event, DomainEvent::TurnStarted(t) if t.turn == queued)
                || matches!(event, DomainEvent::TurnEnded(t) if t.turn == queued)
        })
        .unwrap();
    assert!(
        undone < ran,
        "the queued turn ran before the undo was logged: {log:?}"
    );
    rig.finish();
}

#[test]
fn a_remove_whose_commit_fails_keeps_the_turn_queued() {
    let mut rig = Rig::start("queue-remove-fails", "");
    let thread = rig.thread();
    let first = rig.running_turn(thread);
    let queued = queue_prompt(&mut rig, 30, thread);
    rig.fail(
        1,
        move |event| matches!(event, DomainEvent::TurnEnded(t) if t.turn == queued),
    );
    let removed = rig.dispatch(
        31,
        HostCommand::RemoveQueuedPrompt {
            thread,
            turn: queued,
        },
    );
    assert!(removed.is_err(), "{removed:?}");
    rig.settle_commits();
    assert_eq!(queue::queued_turns(&rig.actor, thread), [queued]);

    // Once the stream drains, a second remove goes through and the turn is gone.
    let removed = rig.dispatch(
        32,
        HostCommand::RemoveQueuedPrompt {
            thread,
            turn: queued,
        },
    );
    assert_eq!(removed.unwrap(), CommandOutcome::Done);
    rig.settle_commits();
    assert_eq!(queue::queued_turns(&rig.actor, thread), []);
    let ends = rig
        .log(thread)
        .into_iter()
        .filter(|event| matches!(event, DomainEvent::TurnEnded(t) if t.turn == queued))
        .count();
    assert_eq!(ends, 1);
    assert_eq!(
        rig.actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .map(|c| c.turn),
        Some(first)
    );
    rig.finish();
}
