//! `RestoreVersion` whose log commit fails: the writer waits for the retry, the files stay
//! restored, and the note reaches the matter's threads whether the retry lands or not. A
//! conversation whose stream is blocked does not keep the matter's stream from recording it.

use clauseo_protocol::{
    CheckpointKind, CommandEnvelope, CommandOutcome, DomainEvent, ExpiryReason, HostCommand,
    MatterPath, RestoreRequest, StopReason, StreamId, ThreadId,
};
use futures::channel::oneshot;

use super::failures::until_ended;
use super::rig::{Rig, command_id};
use crate::actor::input::{Input, Request};
use crate::actor::turn::{End, end};
use crate::pipeline::tests::support::notice;
use crate::undo_note::Told;

/// A Scratch thread whose one turn rewrote `brief.txt`; returns the thread and the restore
/// command that puts the after-turn version back.
fn rewritten(rig: &mut Rig) -> (ThreadId, HostCommand) {
    let thread = rig.thread();
    rig.running_turn(thread);
    rig.write_file("brief.txt", "The lease ends in June.");
    let done = End {
        stop: StopReason::EndTurn,
        failure: None,
        expiry: ExpiryReason::TurnEnded,
        detach: None,
    };
    end::end_turn(&mut rig.actor, thread, done);
    until_ended(rig, thread);
    // The lawyer edits the file after the turn; the restore puts the turn's version back.
    rig.write_file("brief.txt", "The lease ends in July.");
    let before = rig
        .recorded()
        .into_iter()
        .find(|recorded| recorded.kind() == CheckpointKind::AfterTurn)
        .expect("the after-turn checkpoint recorded brief.txt")
        .checkpoint();
    let restore = HostCommand::RestoreVersion {
        thread: None,
        undoes: None,
        matter: rig.matter(),
        checkpoint: before,
        scope: RestoreRequest::Matter,
    };
    (thread, restore)
}

/// The restore of the after-turn version again.
fn restore_again(rig: &mut Rig) -> HostCommand {
    let before = rig
        .recorded()
        .into_iter()
        .find(|recorded| recorded.kind() == CheckpointKind::AfterTurn)
        .unwrap()
        .checkpoint();
    HostCommand::RestoreVersion {
        thread: None,
        undoes: None,
        matter: rig.matter(),
        checkpoint: before,
        scope: RestoreRequest::Matter,
    }
}

fn restores(rig: &mut Rig) -> usize {
    rig.matter_log()
        .iter()
        .filter(|event| matches!(event, DomainEvent::VersionRestored(_)))
        .count()
}

/// The files the restores the thread's next note tells name, held or in the note already.
fn noted(rig: &Rig, thread: ThreadId) -> Vec<String> {
    let turn = &rig.actor.threads[&thread].turn;
    let told = turn.note.iter().flatten().filter_map(|told| match told {
        Told::Restored(restore) => Some(restore),
        Told::Undone(_) => None,
    });
    turn.restored
        .iter()
        .chain(told)
        .flat_map(|restore| restore.back.iter().chain(&restore.removed))
        .map(|path| MatterPath::as_str(path).to_owned())
        .collect()
}

#[test]
fn a_failed_restore_commit_keeps_the_writer_and_is_retried() {
    let mut rig = Rig::start("turn-restore-retried", "");
    let (thread, restore) = rewritten(&mut rig);
    let matter = rig.matter();

    rig.fail(1, |event| matches!(event, DomainEvent::VersionRestored(_)));
    assert!(
        rig.dispatch(20, restore).is_err(),
        "the command gets the failure"
    );
    let brief = std::fs::read_to_string(rig.scratch_file("brief.txt")).unwrap();
    assert_eq!(brief, "The lease ends in June.", "R8 restored the file");

    rig.settle_commits();
    assert_eq!(restores(&mut rig), 1, "the retry logged the restore");
    assert_eq!(rig.actor.matters[&matter].turns.holder, None);
    assert!(rig.actor.matters[&matter].turns.restore.is_none());
    assert_eq!(noted(&rig, thread), ["brief.txt"]);
    rig.finish();
}

#[test]
fn a_restore_commit_failing_twice_still_notes_the_files() {
    let mut rig = Rig::start("turn-restore-gave-up", "");
    let (thread, restore) = rewritten(&mut rig);
    let matter = rig.matter();

    rig.fail(2, |event| matches!(event, DomainEvent::VersionRestored(_)));
    assert!(
        rig.dispatch(20, restore).is_err(),
        "the command gets the failure"
    );
    rig.settle_commits();
    assert_eq!(restores(&mut rig), 0, "neither commit landed");
    let brief = std::fs::read_to_string(rig.scratch_file("brief.txt")).unwrap();
    assert_eq!(brief, "The lease ends in June.", "the files stay restored");
    assert_eq!(
        rig.actor.matters[&matter].turns.holder, None,
        "the writer passed on"
    );
    assert_eq!(
        noted(&rig, thread),
        ["brief.txt"],
        "the agent is still told"
    );

    // The same restore again writes nothing: R8 reuses its restore checkpoint, which the log
    // never recorded, so the host records it late before naming it.
    let command = restore_again(&mut rig);
    let again = rig.dispatch(21, command).unwrap();
    let CommandOutcome::VersionRestored { checkpoint } = again else {
        panic!("RestoreVersion answered {again:?}");
    };
    rig.settle_commits();
    let late = rig
        .recorded()
        .into_iter()
        .find(|recorded| recorded.checkpoint() == checkpoint)
        .expect("the reused restore checkpoint is recorded late");
    assert_eq!(late.kind(), CheckpointKind::Restore);
    let (seqs, row) = rig.checkpoint_seqs_and_row(checkpoint);
    assert_eq!(seqs.len(), 1, "recorded exactly once: {seqs:?}");
    let row = row.expect("the late record has its row");
    assert_eq!(
        (row.id, row.matter, row.seq.get()),
        (checkpoint, matter, seqs[0])
    );
    assert_eq!(row.commit_oid.len(), 40, "{}", row.commit_oid);
    assert_eq!(
        restores(&mut rig),
        0,
        "a restore that wrote nothing logs no VersionRestored"
    );
    rig.finish();
}

#[test]
fn a_restore_asked_from_a_blocked_conversation_is_still_recorded_by_the_matter() {
    let mut rig = Rig::start("turn-restore-blocked-thread", "");
    let (thread, restore) = rewritten(&mut rig);
    let HostCommand::RestoreVersion {
        matter,
        checkpoint,
        scope,
        undoes,
        ..
    } = restore
    else {
        panic!("rewritten gives a restore");
    };
    let restore = HostCommand::RestoreVersion {
        thread: Some(thread),
        undoes,
        matter,
        checkpoint,
        scope,
    };
    // The conversation's first commit fails while the second is in flight: its stream stays
    // blocked through the restore.
    let stream = StreamId::Thread(thread);
    rig.fail(1, |event| *event == notice(1));
    rig.commit(thread, vec![notice(1)]);
    rig.commit(thread, vec![notice(2)]);
    let behind = |actor: &crate::actor::Actor, input: &Input| {
        matches!(input, Input::Committed { ticket, .. }
            if actor.pipeline.events(*ticket).iter().any(|e| e.event == notice(2)))
    };
    // The second's answer is held, so its stream drains only when the test lets it go.
    rig.hold_next("the second commit's answer", behind);
    rig.run_until("the block", |actor| {
        actor.pipeline.blocked(stream).is_some()
    });

    // The restore's commit is built while the conversation's stream is blocked; it is
    // released, in send order, once the held commit returns.
    let (reply, mut answer) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(20),
        command: restore,
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.run_until("the restore's log sent", |actor| {
        actor.matters[&matter].turns.restore.is_some()
    });
    assert!(
        rig.actor.pipeline.blocked(stream).is_some(),
        "still blocked"
    );
    let brief = std::fs::read_to_string(rig.scratch_file("brief.txt")).unwrap();
    assert_eq!(brief, "The lease ends in June.", "R8 restored the file");

    rig.release_held();
    rig.settle_commits();
    let outcome = answer
        .try_recv()
        .unwrap()
        .expect("the restore was answered");
    assert!(
        matches!(outcome, Ok(CommandOutcome::VersionRestored { .. })),
        "{outcome:?}"
    );
    assert_eq!(
        restores(&mut rig),
        1,
        "the matter's stream records the restore"
    );
    assert_eq!(rig.actor.matters[&matter].turns.holder, None);
    assert!(rig.actor.matters[&matter].turns.restore.is_none());
    assert_eq!(noted(&rig, thread), ["brief.txt"]);
    let line = rig
        .log(thread)
        .into_iter()
        .any(|event| matches!(event, DomainEvent::VersionRestored(_)));
    assert!(!line, "the blocked conversation's line was skipped");
    rig.finish();
}
