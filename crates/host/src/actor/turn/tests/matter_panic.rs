//! A matter job that panics, as the actor sees it: the turn or query waiting on it is answered
//! with a failure, the work count and the writer are let go, and the matter's next checkpoint
//! or listing works on the same run. A worker whose thread ended is started again.

use clauseo_protocol::{
    CheckpointKind, DomainEvent, ErrorCode, ExpiryReason, HostQuery, Principal, StopReason,
};
use futures::channel::oneshot;

use super::failures::until_ended;
use super::rig::{Rig, kind};
use crate::actor::input::Request;
use crate::actor::turn::{End, end, writer_busy};
use crate::error::Error;
use crate::matter_worker::{MatterWorker, hook};

/// Where the rig's matters keep their histories: the key a test's armed panic fires by, so it
/// never fires in another test's rig.
fn matters_root(rig: &Rig) -> std::path::PathBuf {
    rig.actor.effective.data_dir().join("matters")
}

/// A turn that ended on its own.
fn done() -> End {
    End {
        stop: StopReason::EndTurn,
        failure: None,
        expiry: ExpiryReason::TurnEnded,
        detach: None,
    }
}

#[test]
fn a_checkpoint_that_panics_fails_its_turn_and_the_next_turn_saves() {
    let mut rig = Rig::start("matter-panic-after-turn", "");
    let thread = rig.thread();
    let matter = rig.matter();
    let _first = rig.running_turn(thread);
    rig.write_file("brief.txt", "The lease ends in May.");
    hook::panic_next(&matters_root(&rig), "AfterTurn", 1);
    end::end_turn(&mut rig.actor, thread, done());
    // Waits for no turn and no matter job in flight: the panicking job was answered.
    until_ended(&mut rig, thread);
    assert_eq!(rig.actor.work.matter_jobs, 0);
    assert!(!writer_busy(&rig.actor, matter), "the writer is free");
    let log = rig.log(thread);
    assert!(
        log.iter()
            .any(|event| matches!(event, DomainEvent::TurnFailed(_))),
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );

    let second = rig.running_turn(thread);
    rig.write_file("brief.txt", "The lease ends in June.");
    end::end_turn(&mut rig.actor, thread, done());
    until_ended(&mut rig, thread);
    assert_eq!(rig.actor.work.matter_jobs, 0);
    let recorded = rig.recorded();
    let last = recorded.last().unwrap();
    assert_eq!(last.kind(), CheckpointKind::AfterTurn);
    assert_eq!(
        last.turn(),
        Some(second),
        "the next turn's checkpoint is taken and recorded"
    );
    rig.finish();
}

/// A `ListPapers` of the rig's matter, and its answer.
fn list_papers(rig: &mut Rig) -> Result<(), Error> {
    let (reply, mut answer) = oneshot::channel();
    let query = HostQuery::ListPapers {
        matter: rig.matter(),
    };
    rig.actor.on_request(Request::Query {
        principal: Principal::LocalDesktop,
        query,
        reply,
    });
    let mut got = None;
    rig.run_until("the listing", |actor| {
        if let Ok(Some(result)) = answer.try_recv() {
            got = Some(result.map(drop));
        }
        got.is_some() && actor.work.matter_jobs == 0
    });
    got.unwrap()
}

#[test]
fn a_listing_that_panics_is_answered_and_the_next_one_lists() {
    let mut rig = Rig::start("matter-panic-listing", "");
    hook::panic_next(&matters_root(&rig), "ListPapers", 1);
    let failed = list_papers(&mut rig).unwrap_err();
    assert_eq!(failed.code(), ErrorCode::Internal);
    assert_eq!(rig.actor.work.matter_jobs, 0);
    list_papers(&mut rig).unwrap();
    rig.finish();
}

#[test]
fn a_worker_whose_thread_ended_is_started_again() {
    let mut rig = Rig::start("matter-worker-ended", "");
    let matter = rig.matter();
    let entry = rig.actor.matters.get_mut(&matter).unwrap();
    entry.worker = Some(MatterWorker::ended());
    list_papers(&mut rig).unwrap();
    let entry = &rig.actor.matters[&matter];
    assert!(entry.worker.as_ref().is_some_and(MatterWorker::is_alive));
    rig.finish();
}
