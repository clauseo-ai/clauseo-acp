//! Limit signals: a rejected rate limit and an AIR session failure end the turn at the limit;
//! a warning logs nothing.

use clauseo_protocol::{
    DomainEvent, LimitStopReason, StopReason, StreamId, Timestamp, TurnStoppedAtLimit,
};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};
use crate::turn::change_label;

/// Starts a host whose `fake-agent` plays `scenario`, runs one prompt to its end and returns
/// the thread's events (drained after a settle) with the turn.
fn one_turn(
    test: &str,
    scenario: &'static str,
) -> (Dir, Running, Events, clauseo_protocol::TurnId) {
    let dir = Dir::new(test, &[(FAKE, Scenario::Bundled(scenario))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Draft the clause.");
    events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    (dir, host, events, turn)
}

fn is_stop(event: &DomainEvent) -> bool {
    matches!(event, DomainEvent::TurnStoppedAtLimit(_))
}

fn stops(events: &Events) -> Vec<TurnStoppedAtLimit> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::TurnStoppedAtLimit(stop) => Some(stop.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn limit_rejected_ends_turn_at_limit() {
    let (_dir, host, mut events, turn) =
        one_turn("limit_rejected_ends_turn_at_limit", "limit-rejected");
    events.until("TurnStoppedAtLimit", |e| e.position(is_stop).is_some());
    let ended = events.turn_ended(turn).unwrap();
    assert_eq!(ended.stop, StopReason::AtLimit);
    assert!(ended.checkpoint_after.is_some());

    let changed = events.files_changed(turn).expect("FilesChanged");
    let files: Vec<(&str, &str)> = changed
        .files
        .iter()
        .map(|f| (f.path().as_str(), change_label(f)))
        .collect();
    assert_eq!(files, [("notes/draft.txt", "added")]);

    assert_eq!(
        stops(&events),
        [TurnStoppedAtLimit {
            turn,
            window: Some("five_hour".to_owned()),
            resets_at: Some(Timestamp::new(1_792_310_400_000).unwrap()),
            reason: LimitStopReason::PlanWindow,
        }]
    );
    // FilesChanged, TurnEnded and TurnStoppedAtLimit: consecutive seqs, delivered together as
    // the one commit that holds them.
    let at = |find: &dyn Fn(&DomainEvent) -> bool| events.position(find).unwrap();
    let files_at = at(&|e| matches!(e, DomainEvent::FilesChanged(_)));
    let ended_at = at(&|e| matches!(e, DomainEvent::TurnEnded(_)));
    let stop_at = at(&is_stop);
    assert_eq!(
        (ended_at, stop_at),
        (files_at + 1, files_at + 2),
        "{}",
        events.dump()
    );
    assert_eq!(
        events.batch[files_at], events.batch[stop_at],
        "not one commit"
    );
    host.shutdown();
}

#[test]
fn session_failure_ends_turn_at_limit() {
    let (_dir, host, mut events, turn) =
        one_turn("session_failure_ends_turn_at_limit", "session-failure");
    events.until("TurnStoppedAtLimit", |e| e.position(is_stop).is_some());
    host.settle();
    events.drain_ready();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::AtLimit);
    assert_eq!(
        stops(&events),
        [TurnStoppedAtLimit {
            turn,
            window: None,
            resets_at: None,
            reason: LimitStopReason::PlanWindow,
        }],
        "{}",
        events.dump()
    );
    let ended_at = events
        .position(|e| matches!(e, DomainEvent::TurnEnded(_)))
        .unwrap();
    assert!(events.position(is_stop).unwrap() > ended_at);
    host.shutdown();
}

#[test]
fn limit_warning_logs_no_limit_event() {
    let (_dir, host, events, turn) = one_turn("limit_warning_logs_no_limit_event", "limit-warning");
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.count(is_stop), 0, "{}", events.dump());
    host.shutdown();
}
