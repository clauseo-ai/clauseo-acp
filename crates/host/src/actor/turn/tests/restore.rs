//! `RestoreVersion` whose log commit fails: the writer waits for the retry, the files stay
//! restored, and the note reaches the matter's threads whether the retry lands or not.

use clauseo_protocol::{
    CheckpointKind, CommandOutcome, DomainEvent, ExpiryReason, HostCommand, MatterPath,
    RestoreRequest, StopReason, ThreadId,
};

use super::failures::until_ended;
use super::rig::Rig;
use crate::actor::turn::{End, end};

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

fn noted(rig: &Rig, thread: ThreadId) -> Vec<String> {
    rig.actor.threads[&thread]
        .turn
        .restored
        .iter()
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
    assert_eq!(
        restores(&mut rig),
        0,
        "a restore that wrote nothing logs no VersionRestored"
    );
    rig.finish();
}
