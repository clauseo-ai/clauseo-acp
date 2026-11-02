//! A pending stop at the limit during shutdown (`limits/stops.rs`), on the turn rig's actor
//! with a limits model started by hand for Claude from a stored plan at its limit: shutdown
//! decides the stop from `at_limit`, a failed commit of it is retried at the unblock before the
//! actor exits, and a stop whose retries all failed does not hold shutdown.

use std::sync::Arc;

use clauseo_limits::{ReadLimits, ReadOutcome};
use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, DomainEvent, Finite,
    LimitSource, LimitStopReason, LimitWindow, PlanLabel, StopReason, ThreadId, Timestamp,
    TurnEnded, TurnId, TurnStoppedAtLimit, WindowKind,
};

use super::rig::{Rig, kind};
use crate::actor::limits::{self, for_tests};

/// How many failed commits of one stop the unblock retries (`UNBLOCK_RETRIES` in `stops.rs`).
const UNBLOCK_RETRIES: usize = 2;

/// A reader whose reads never answer.
struct Silent;

impl ReadLimits for Silent {
    fn read(
        &self,
        _agent: AgentKind,
    ) -> std::pin::Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        Box::pin(futures::future::pending())
    }
}

/// Claude's plan with its five-hour window full, resetting at `resets_at`.
fn full(checked_at: Timestamp, resets_at: Timestamp) -> AccountLimits {
    AccountLimits {
        agent: AgentKind::Claude,
        account: AccountRef("ada@example.com".to_owned()),
        plan: Some(PlanLabel("Max".to_owned())),
        billing: Billing::Plan,
        windows: vec![LimitWindow {
            id: "five_hour".to_owned(),
            kind: WindowKind::Session,
            label: "5-hour limit".to_owned(),
            used_percent: Finite::new(100.0).unwrap(),
            resets_at: Some(resets_at),
            duration_mins: Some(300),
            warning: false,
        }],
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Fresh { checked_at },
    }
}

/// A thread whose turn ended `AtLimit` (durable) with a `Temporary` stop waiting for a read,
/// Claude at its limit until `resets_at`.
fn pending(rig: &mut Rig) -> (ThreadId, TurnId, Timestamp) {
    let now = rig.actor.now().wall;
    let resets_at = Timestamp::new(now.as_millis() + 600_000).unwrap();
    for_tests::start(
        &mut rig.actor,
        Arc::new(Silent),
        vec![full(now, resets_at)],
        vec![AgentKind::Claude],
    );
    let thread = rig.thread();
    let turn = rig.running_turn(thread);
    rig.actor.threads.get_mut(&thread).unwrap().turn.current = None;
    let ended = TurnEnded {
        turn,
        stop: StopReason::AtLimit,
        usage: None,
        checkpoint_after: None,
    };
    rig.commit(thread, vec![ended.into()]);
    rig.settle_commits();
    limits::stop_later(
        &mut rig.actor,
        AgentKind::Claude,
        thread,
        turn,
        LimitStopReason::Temporary,
    );
    assert!(limits::holds_stop(&rig.actor, thread));
    (thread, turn, resets_at)
}

fn stops(log: &[DomainEvent]) -> Vec<TurnStoppedAtLimit> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::TurnStoppedAtLimit(stop) = event {
                Some(stop.clone())
            } else {
                None
            }
        })
        .collect()
}

fn is_stop(event: &DomainEvent) -> bool {
    matches!(event, DomainEvent::TurnStoppedAtLimit(_))
}

#[test]
fn a_stop_whose_commit_fails_during_shutdown_is_retried_before_the_actor_exits() {
    let mut rig = Rig::start("limit-stop-shutdown-retry", "");
    let (thread, turn, resets_at) = pending(&mut rig);
    rig.fail(1, is_stop);
    rig.shut_down();
    assert!(
        !limits::holds_stop(&rig.actor, thread),
        "the stop is still pending"
    );
    let log = rig.log(thread);
    assert_eq!(
        stops(&log),
        [TurnStoppedAtLimit {
            turn,
            window: Some("five_hour".to_owned()),
            resets_at: Some(resets_at),
            reason: LimitStopReason::Temporary,
        }],
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
    rig.finish();
}

#[test]
fn a_stop_whose_retries_all_fail_does_not_hold_shutdown() {
    let mut rig = Rig::start("limit-stop-shutdown-gives-up", "");
    let (thread, _, _) = pending(&mut rig);
    rig.fail(UNBLOCK_RETRIES, is_stop);
    rig.shut_down();
    assert!(
        limits::holds_stop(&rig.actor, thread),
        "the stop was dropped"
    );
    assert_eq!(stops(&rig.log(thread)), []);
    rig.finish();
}
