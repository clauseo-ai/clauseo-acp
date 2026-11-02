//! A pending stop (a turn stopped at the limit with no reset time, waiting for its agent's
//! next read) is never lost while the host runs: it keeps the thread's stream loaded after the
//! agent exits, a Claude one is logged after 30 s when no read answers, a later turn running
//! when the read answers changes nothing but the order, a read that began before the stop does
//! not decide it, and a normal quit logs it with its own reason. Reads go through the scripted
//! [`Reads`].

use std::sync::Arc;
use std::time::Duration;

use clauseo_limits::ReadOutcome;
use clauseo_protocol::{
    AgentKind, DomainEvent, LimitStopReason, RefreshWhy, StopReason, StreamId, ThreadId, TurnId,
    TurnStoppedAtLimit,
};
use clauseo_reducer::{Anomaly, StreamState};

use crate::limits_host::{CLAUDE, Feed, Reads, at, refresh, usage};
use crate::observe::Events;
use crate::permissions::until_first_request;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// How long a Claude pending stop waits for a read.
const STOP_WAIT: Duration = Duration::from_secs(30);

/// A Claude host playing `scenario`, its start-up read answered with a plan 10% used, one
/// thread of it, and that thread's first turn run to its end, stopped at the limit with no
/// reset time and no stop logged yet; the read the stop asked for is in flight.
struct Stopped {
    dir: Dir,
    host: Running,
    reads: Reads,
    thread: ThreadId,
    turn: TurnId,
    events: Events,
}

fn stopped(test: &str, scenario: Scenario) -> Stopped {
    stopped_after(test, scenario, usage(AgentKind::Claude, 10.0, None))
}

/// [`stopped`], its start-up read answered with `start_up`.
fn stopped_after(test: &str, scenario: Scenario, start_up: ReadOutcome) -> Stopped {
    let dir = Dir::new(test, &[(CLAUDE, scenario)]);
    let reads = Reads::new();
    let host = start(&dir, 0, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(AgentKind::Claude, start_up);
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
        dir,
        host,
        reads,
        thread,
        turn,
        events,
    }
}

/// Starts run `run` of `dir` reading through `reads`.
fn start(dir: &Dir, run: u16, reads: &Reads) -> Running {
    let reader = Arc::new(reads.clone());
    dir.start_with(run, move |config| config.with_limits_reader(reader))
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

/// The stop a read finding no limit gives (or the 30 s bound, with the start-up read's plan).
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
fn a_claude_stop_whose_read_never_answers_is_logged_after_thirty_seconds() {
    let mut rig = stopped(
        "a_claude_stop_whose_read_never_answers_is_logged_after_thirty_seconds",
        Scenario::Bundled("session-failure"),
    );
    rig.host.wait_armed(STOP_WAIT);
    rig.host.clock.advance(Duration::from_millis(29_999));
    rig.host.settle();
    rig.events.drain_ready();
    assert_eq!(stops(&rig.events), [], "logged before 30 s");

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

#[test]
fn a_read_that_began_before_the_stop_does_not_decide_it() {
    let dir = Dir::new(
        "a_read_that_began_before_the_stop_does_not_decide_it",
        &[(CLAUDE, Scenario::Bundled("session-failure"))],
    );
    let reads = Reads::new();
    let host = start(&dir, 0, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 10.0, None));
    feed.changed("the start-up read");
    drop(feed);
    // A read starts before the turn and still runs when the turn stops at the limit.
    refresh(&host, RefreshWhy::TryAgain);
    reads.until_count(AgentKind::Claude, 2);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Draft the clause.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::AtLimit);
    host.settle();
    events.drain_ready();
    assert_eq!(reads.count(AgentKind::Claude), 2, "one read at a time");

    // The earlier read answers, under the limit: it decides nothing, and a read starts again.
    reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 40.0, Some(at(7200))),
    );
    reads.until_count(AgentKind::Claude, 3);
    host.settle();
    events.drain_ready();
    assert_eq!(stops(&events), [], "{}", events.dump());

    reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 100.0, Some(at(7200))),
    );
    assert_eq!(
        until_stop(&host, &mut events),
        [TurnStoppedAtLimit {
            turn,
            window: Some("five_hour".to_owned()),
            resets_at: Some(at(7200)),
            reason: LimitStopReason::PlanWindow,
        }]
    );
    assert_eq!(reads.count(AgentKind::Claude), 3);
    host.shutdown();
}

#[test]
fn a_quit_while_a_stop_waits_logs_it_with_its_own_reason() {
    // At the limit from the start-up read, so `at_limit` gives the window at the quit.
    let rig = stopped_after(
        "a_quit_while_a_stop_waits_logs_it_with_its_own_reason",
        Scenario::Local("session-failure-temporary"),
        usage(AgentKind::Claude, 100.0, Some(at(7200))),
    );
    let Stopped {
        dir,
        host,
        thread,
        turn,
        events,
        ..
    } = rig;
    drop(events);
    // The read the stop asked for is still in flight.
    host.shutdown();

    let host = start(&dir, 1, &Reads::new());
    host.settle();
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    host.settle();
    events.drain_ready();
    assert_eq!(
        stops(&events),
        [TurnStoppedAtLimit {
            turn,
            window: Some("five_hour".to_owned()),
            resets_at: Some(at(7200)),
            reason: LimitStopReason::Temporary,
        }],
        "{}",
        events.dump()
    );
    host.shutdown();
}
