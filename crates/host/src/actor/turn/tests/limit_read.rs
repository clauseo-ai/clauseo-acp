//! The Codex fallback's wait for a limits read (`end.rs`): the end commit waits for the read
//! even once the after-turn checkpoint has answered, and the read, the 10 s bound or shutdown
//! ends the turn exactly once. The rig's agent has no kind, so there is no limits model:
//! `Limits::at_limit` answers nothing and the turn fails as before; the host tests in
//! `tests/host/limits_stop.rs` cover the at-limit answer.

use clauseo_protocol::{AgentKind, DomainEvent, ErrorCode, StopReason, ThreadId, TurnId};

use super::failures::until_ended;
use super::rig::{Rig, kind};
use crate::actor::turn::{End, TurnPhase, end};

/// A running turn of a new thread whose prompt failed with a JSON-RPC error and waits for its
/// read: steps 6 and 7 have run and the after-turn checkpoint has answered.
fn held(rig: &mut Rig) -> (ThreadId, TurnId) {
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    let current = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .current
        .as_mut();
    current.unwrap().limit_read = Some(end::LimitRead::default());
    let failed = End::failed(
        ErrorCode::AgentProtocolError,
        Some("-32603".to_owned()),
        None,
    );
    end::end_turn(&mut rig.actor, thread, failed);
    rig.run_until("the after-turn checkpoint", |actor| {
        let current = actor.threads[&thread].turn.current.as_deref();
        let read = current.and_then(|current| current.limit_read.as_ref());
        read.is_some_and(end::LimitRead::holds_checkpoint) && actor.work.commits == 0
    });
    assert_waiting(rig, thread, turn);
    (thread, turn)
}

/// The turn is still `Ending`, waiting, and nothing ended it in the log.
fn assert_waiting(rig: &mut Rig, thread: ThreadId, turn: TurnId) {
    let current = rig.actor.threads[&thread].turn.current.as_deref();
    let phase = current.map(|current| (current.turn, current.phase));
    assert_eq!(phase, Some((turn, TurnPhase::Ending)));
    let log = rig.log(thread);
    assert!(
        !log.iter()
            .any(|event| matches!(event, DomainEvent::TurnEnded(_))),
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
}

/// One `TurnEnded { Failed }` for `turn`, after `TurnFailed { agent.protocol_error }`.
fn assert_failed_once(log: &[DomainEvent], turn: TurnId) {
    let ends = log
        .iter()
        .filter(|event| matches!(event, DomainEvent::TurnEnded(ended) if ended.turn == turn))
        .count();
    assert_eq!(ends, 1, "{:?}", log.iter().map(kind).collect::<Vec<_>>());
    let [
        ..,
        DomainEvent::TurnFailed(failed),
        DomainEvent::TurnEnded(ended),
    ] = log
    else {
        panic!("{:?}", log.iter().map(kind).collect::<Vec<_>>());
    };
    assert_eq!(failed.code, ErrorCode::AgentProtocolError);
    assert_eq!(ended.stop, StopReason::Failed);
}

#[test]
fn the_end_waits_for_the_read_then_ends_once() {
    let mut rig = Rig::start("limit-read-done", "");
    let (thread, turn) = held(&mut rig);
    // A Claude read is not the one it waits for.
    end::limits_read(&mut rig.actor, AgentKind::Claude);
    rig.settle_commits();
    assert_waiting(&mut rig, thread, turn);

    end::limits_read(&mut rig.actor, AgentKind::Codex);
    until_ended(&mut rig, thread);
    end::limits_read(&mut rig.actor, AgentKind::Codex);
    rig.settle_commits();
    assert_failed_once(&rig.log(thread), turn);
    rig.finish();
}

#[test]
fn the_end_waits_at_most_until_its_timer() {
    let mut rig = Rig::start("limit-read-timer", "");
    let (thread, turn) = held(&mut rig);
    let other: TurnId = rig.actor.ids.mint(rig.actor.now().wall).unwrap();
    end::on_limit_read_timer(&mut rig.actor, thread, other);
    rig.settle_commits();
    assert_waiting(&mut rig, thread, turn);

    end::on_limit_read_timer(&mut rig.actor, thread, turn);
    until_ended(&mut rig, thread);
    assert_failed_once(&rig.log(thread), turn);
    rig.finish();
}

#[test]
fn shutdown_ends_a_turn_waiting_for_its_read() {
    let mut rig = Rig::start("limit-read-shutdown", "");
    let (thread, turn) = held(&mut rig);
    let mut rig = rig.restart(1);
    assert_failed_once(&rig.log(thread), turn);
    rig.finish();
}
