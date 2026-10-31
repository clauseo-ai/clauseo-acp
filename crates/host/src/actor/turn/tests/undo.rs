//! `UndoStep` whose commit fails: the writer and the undo note wait until `StepUndone` is
//! durable, and the commit is sent once more when the stream drains.

use clauseo_protocol::{
    CommandEnvelope, DomainEvent, ExpiryReason, FileChoice, FileDecision, HostCommand, MatterPath,
    StopReason,
};
use futures::channel::oneshot;

use super::failures::until_ended;
use super::rig::{Rig, command_id};
use crate::actor::Actor;
use crate::actor::input::{Input, Request};
use crate::actor::turn::{End, end};
use crate::pipeline::tests::support::notice;

/// Whether `input` is the result of a commit holding an event `pick` chooses.
fn result_of(actor: &Actor, input: &Input, pick: impl Fn(&DomainEvent) -> bool) -> bool {
    matches!(input, Input::Committed { ticket, .. }
        if actor.pipeline.events(*ticket).iter().any(|e| pick(&e.event)))
}

#[test]
fn a_failed_undo_commit_keeps_the_writer_and_is_retried() {
    let mut rig = Rig::start("turn-undo-retried", "");
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    rig.write_file("brief.txt", "The lease ends in May.");
    let done = End {
        stop: StopReason::EndTurn,
        failure: None,
        expiry: ExpiryReason::TurnEnded,
        detach: None,
    };
    end::end_turn(&mut rig.actor, thread, done);
    until_ended(&mut rig, thread);
    let matter = rig.matter();

    // The undo's commit fails while a commit behind it keeps the stream blocked.
    rig.fail(1, |event| matches!(event, DomainEvent::StepUndone(_)));
    let undo = HostCommand::UndoStep {
        thread,
        turn,
        files: vec![FileDecision {
            path: MatterPath::new("brief.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    };
    let (reply, mut answer) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(20),
        command: undo,
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.hold_next("the undo's result", |actor, input| {
        result_of(actor, input, |event| {
            matches!(event, DomainEvent::StepUndone(_))
        })
    });
    rig.commit(thread, vec![notice(9)]);
    rig.hold_next("the result behind it", |actor, input| {
        result_of(actor, input, |event| *event == notice(9))
    });
    let undone = rig.held.remove(0);
    rig.actor.on_input(undone);
    assert!(
        matches!(answer.try_recv(), Ok(Some(Err(_)))),
        "the command gets the failure"
    );
    assert!(!rig.scratch_file("brief.txt").exists(), "R8 undid the file");
    assert_eq!(
        rig.actor.matters[&matter].turns.holder,
        Some(thread),
        "the writer waits for the retry"
    );
    let state = &rig.actor.threads[&thread].turn;
    assert!(state.note.as_ref().is_none_or(Vec::is_empty));

    rig.release_held();
    rig.settle_commits();
    let steps = rig
        .log(thread)
        .into_iter()
        .filter(|event| matches!(event, DomainEvent::StepUndone(_)))
        .count();
    assert_eq!(steps, 1, "the retry logged the undo");
    assert_eq!(rig.actor.matters[&matter].turns.holder, None);
    let note = rig.actor.threads[&thread]
        .turn
        .note
        .clone()
        .unwrap_or_default();
    let names: Vec<_> = note.iter().map(MatterPath::as_str).collect();
    assert_eq!(names, ["brief.txt"]);
    rig.finish();
}
