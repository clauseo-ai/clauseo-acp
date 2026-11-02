//! A pending stop (a turn stopped at the limit with no reset time, waiting for its agent's
//! next read) is never lost while the host runs: it keeps the thread's stream loaded after the
//! agent exits, it is logged after 10 s when no read answers, and a later turn running when
//! the read answers changes nothing but the order. Reads go through the scripted [`Reads`].

use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{
    AgentKind, DomainEvent, LimitStopReason, StopReason, StreamId, ThreadId, TurnId,
    TurnStoppedAtLimit,
};
use clauseo_reducer::{Anomaly, StreamState};

use crate::limits_host::{CLAUDE, Feed, Reads, at, usage};
use crate::observe::Events;
use crate::permissions::until_first_request;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// How long a pending stop waits for a read.
const STOP_WAIT: Duration = Duration::from_secs(10);

/// A Claude host playing `scenario`, its start-up read answered with a plan 10% used, one
/// thread of it, and that thread's first turn run to its end, stopped at the limit with no
/// reset time and no stop logged yet; the read the stop asked for is in flight.
struct Stopped {
    _dir: Dir,
    host: Running,
    reads: Reads,
    thread: ThreadId,
    turn: TurnId,
    events: Events,
}

fn stopped(test: &str, scenario: Scenario) -> Stopped {
    let dir = Dir::new(test, &[(CLAUDE, scenario)]);
    let reads = Reads::new();
    let reader = Arc::new(reads.clone());
    let host = dir.start_with(0, move |config| config.with_limits_reader(reader));
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 10.0, None));
    feed.changed("the start-up read");
    drop(feed);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Draft the clause.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::AtLimit);
    reads.until_count(AgentKind::Claude, 2);
    host.settle();
    events.drain_ready();
    assert_eq!(stops(&events), [], "{}", events.dump());
    Stopped {
        _dir: dir,
        host,
        reads,
        thread,
        turn,
        events,
    }
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

/// Reads until a stop is in `events`, then what a settle brings.
fn until_stop(host: &Running, events: &mut Events) -> Vec<TurnStoppedAtLimit> {
    events.until("TurnStoppedAtLimit", |e| e.position(is_stop).is_some());
    host.settle();
    events.drain_ready();
    stops(events)
}

/// The stop a read finding no limit gives (or the 10 s bound, with the start-up read's plan).
fn empty_stop(turn: TurnId) -> TurnStoppedAtLimit {
    TurnStoppedAtLimit {
        turn,
        window: None,
        resets_at: None,
        reason: LimitStopReason::PlanWindow,
    }
}

#[test]
fn a_stop_outlives_its_agent_and_every_viewer() {
    let mut rig = stopped(
        "a_stop_outlives_its_agent_and_every_viewer",
        Scenario::Local("session-failure-then-exit"),
    );
    // The agent exits at the next prompt, which ends that turn too.
    let (later, _) = rig.host.prompt(rig.thread, "Edit the clause.");
    rig.events.until_turn_ended(later);
    rig.events.until("the agent's exit", |e| {
        e.position(|event| matches!(event, DomainEvent::AgentSessionDetached(_)))
            .is_some()
    });
    // Nothing views the thread and its agent is gone: only the pending stop keeps it loaded.
    let Stopped {
        host,
        reads,
        thread,
        turn,
        events,
        ..
    } = rig;
    drop(events);
    host.settle();

    reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 40.0, Some(at(7200))),
    );
    host.settle();
    let mut events = host.events(StreamId::Thread(thread));
    assert_eq!(until_stop(&host, &mut events), [empty_stop(turn)]);
    host.shutdown();
}

#[test]
fn a_stop_whose_read_never_answers_is_logged_after_ten_seconds() {
    let mut rig = stopped(
        "a_stop_whose_read_never_answers_is_logged_after_ten_seconds",
        Scenario::Bundled("session-failure"),
    );
    rig.host.wait_armed(STOP_WAIT);
    rig.host.clock.advance(Duration::from_millis(9_999));
    rig.host.settle();
    rig.events.drain_ready();
    assert_eq!(stops(&rig.events), [], "logged before 10 s");

    rig.host.clock.advance(Duration::from_millis(1));
    let logged = until_stop(&rig.host, &mut rig.events);
    assert_eq!(
        logged,
        [empty_stop(rig.turn)],
        "the start-up read's plan is under the limit"
    );

    // The read answering at last logs nothing more.
    rig.reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 100.0, Some(at(7200))),
    );
    rig.host.settle();
    rig.events.drain_ready();
    assert_eq!(rig.events.count(is_stop), 1, "{}", rig.events.dump());
    rig.host.shutdown();
}

#[test]
fn a_stop_logged_while_a_later_turn_runs_is_no_duplicate() {
    let mut rig = stopped(
        "a_stop_logged_while_a_later_turn_runs_is_no_duplicate",
        Scenario::Local("session-failure-then-ask"),
    );
    let (later, _) = rig.host.prompt(rig.thread, "Edit the clause.");
    until_first_request(&mut rig.events);

    let resets_at = at(7200);
    rig.reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 100.0, Some(resets_at)),
    );
    let logged = until_stop(&rig.host, &mut rig.events);
    assert_eq!(
        logged,
        [TurnStoppedAtLimit {
            turn: rig.turn,
            window: Some("five_hour".to_owned()),
            resets_at: Some(resets_at),
            reason: LimitStopReason::PlanWindow,
        }]
    );
    assert!(
        rig.events.turn_ended(later).is_none(),
        "the later turn still runs: {}",
        rig.events.dump()
    );

    crate::permissions::answer_from(&rig.host, &mut rig.events, rig.thread, 0, &["allow-once"]);
    let ended = rig.events.until_turn_ended(later);
    assert_eq!(ended.stop, StopReason::EndTurn);
    rig.host.settle();
    rig.events.drain_ready();
    rig.events.assert_no_mismatch();

    let mut state = StreamState::new(StreamId::Thread(rig.thread));
    let mut anomalies = Vec::new();
    for envelope in &rig.events.seen {
        anomalies.extend(
            clauseo_reducer::apply(&mut state, envelope)
                .unwrap()
                .anomalies,
        );
    }
    assert!(
        !anomalies
            .iter()
            .any(|anomaly| matches!(anomaly, Anomaly::Duplicate { .. })),
        "{anomalies:?}"
    );
    assert_eq!(rig.events.count(is_stop), 1, "{}", rig.events.dump());
    rig.host.shutdown();
}
