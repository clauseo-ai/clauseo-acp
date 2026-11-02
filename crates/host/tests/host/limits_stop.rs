//! A turn stopped at the limit, with `limits` in the host: the stop with no reset time is
//! logged after the read that gives it, start-up repair logs one a restart lost, a stop with a
//! reset time stays in the end commit, and a Codex prompt failing with a JSON-RPC error waits
//! for one read to tell a limit from an error. Every test reads through the scripted
//! [`Reads`], so no reader process starts.

use std::sync::Arc;
use std::time::Duration;

use clauseo_limits::{ReadFailure, ReadOutcome};
use clauseo_protocol::{
    AgentKind, DomainEvent, ErrorCode, LimitStopReason, StopReason, StreamId, ThreadId, Timestamp,
    TurnId, TurnStoppedAtLimit,
};

use crate::codex::{CODEX, turn_failed};
use crate::limits_host::{CLAUDE, Feed, Reads, at, usage};
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// The Codex fallback's wait for its read.
const READ_WAIT: Duration = Duration::from_secs(10);

/// Starts run `run` of `dir` reading through `reads`.
fn start(dir: &Dir, run: u16, reads: &Reads) -> Running {
    let reader = Arc::new(reads.clone());
    dir.start_with(run, move |config| config.with_limits_reader(reader))
}

/// Waits until `agent` has had `n` reads, and fails the test if it has had more.
fn until_reads(host: &Running, reads: &Reads, agent: AgentKind, n: usize) {
    reads.until_count(agent, n);
    host.settle();
    assert_eq!(reads.count(agent), n, "reads of {agent:?}");
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

/// The commit (by its batch) that delivered the first event `find` matches.
fn batch_of(events: &Events, find: impl Fn(&DomainEvent) -> bool) -> usize {
    let at = events.position(find).unwrap();
    events.batch[at]
}

/// A host whose `agent` plays `scenario`, its start-up read answered with a plan 10% used, and
/// one thread of it.
struct Rig {
    dir: Dir,
    host: Running,
    reads: Reads,
    thread: ThreadId,
    events: Events,
}

impl Rig {
    fn new(test: &str, id: &str, agent: AgentKind, scenario: Scenario) -> Self {
        let dir = Dir::new(test, &[(id, scenario)]);
        let reads = Reads::new();
        let host = start(&dir, 0, &reads);
        let mut feed = Feed::new(&host);
        feed.view("the current view");
        reads.answer(agent, usage(agent, 10.0, None));
        feed.changed("the start-up read");
        drop(feed);
        let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], id);
        let events = host.events(StreamId::Thread(thread));
        Self {
            dir,
            host,
            reads,
            thread,
            events,
        }
    }

    /// Runs one prompt to its end; the turn's events are drained after a settle.
    fn turn(&mut self) -> TurnId {
        let (turn, _) = self.host.prompt(self.thread, "Draft the clause.");
        self.events.until_turn_ended(turn);
        self.host.settle();
        self.events.drain_ready();
        self.events.assert_no_mismatch();
        turn
    }

    /// Waits for the turn's stop and drains what a settle brings after it.
    fn until_stop(&mut self) -> Vec<TurnStoppedAtLimit> {
        self.events
            .until("TurnStoppedAtLimit", |e| e.position(is_stop).is_some());
        self.host.settle();
        self.events.drain_ready();
        stops(&self.events)
    }
}

/// A turn stopped at the limit with no reset time: the end commit holds `TurnEnded { AtLimit }`
/// and no stop; one read starts; once it answers `outcome`, one stop follows in a commit of its
/// own, which this returns.
fn stop_after_read(
    test: &str,
    id: &str,
    agent: AgentKind,
    scenario: &'static str,
    outcome: ReadOutcome,
) -> TurnStoppedAtLimit {
    let mut rig = Rig::new(test, id, agent, Scenario::Bundled(scenario));
    let turn = rig.turn();
    assert_eq!(
        rig.events.turn_ended(turn).unwrap().stop,
        StopReason::AtLimit
    );
    assert_eq!(stops(&rig.events), [], "{}", rig.events.dump());
    until_reads(&rig.host, &rig.reads, agent, 2);

    rig.reads.answer(agent, outcome);
    let stops = rig.until_stop();
    let [stop] = stops.as_slice() else {
        panic!("one stop: {}", rig.events.dump());
    };
    assert_eq!(stop.turn, turn);
    let ended = batch_of(&rig.events, |e| matches!(e, DomainEvent::TurnEnded(_)));
    assert_ne!(
        ended,
        batch_of(&rig.events, is_stop),
        "the stop rode in the end commit"
    );
    assert_eq!(rig.reads.count(agent), 2, "one read for the stop");
    rig.host.shutdown();
    stop.clone()
}

/// The plan read when an agent is at its limit: the five-hour window full, resetting in 2 h.
fn full(agent: AgentKind) -> ReadOutcome {
    usage(agent, 100.0, Some(at(7200)))
}

/// The stop a full five-hour window gives.
fn full_window(stop: &TurnStoppedAtLimit) -> (Option<&str>, Option<Timestamp>, LimitStopReason) {
    (stop.window.as_deref(), stop.resets_at, stop.reason)
}

#[test]
fn claude_stop_with_no_reset_takes_the_window_from_the_read() {
    let stop = stop_after_read(
        "claude_stop_with_no_reset_takes_the_window_from_the_read",
        CLAUDE,
        AgentKind::Claude,
        "session-failure",
        full(AgentKind::Claude),
    );
    assert_eq!(
        full_window(&stop),
        (
            Some("five_hour"),
            Some(at(7200)),
            LimitStopReason::PlanWindow
        )
    );
}

#[test]
fn codex_air_stop_takes_the_window_from_the_read() {
    let stop = stop_after_read(
        "codex_air_stop_takes_the_window_from_the_read",
        CODEX,
        AgentKind::Codex,
        "codex-limit-air",
        full(AgentKind::Codex),
    );
    assert_eq!(
        full_window(&stop),
        (
            Some("five_hour"),
            Some(at(7200)),
            LimitStopReason::PlanWindow
        )
    );
}

#[test]
fn stop_with_no_reset_is_empty_when_the_read_finds_no_limit() {
    let stop = stop_after_read(
        "stop_with_no_reset_is_empty_when_the_read_finds_no_limit",
        CLAUDE,
        AgentKind::Claude,
        "session-failure",
        usage(AgentKind::Claude, 40.0, Some(at(7200))),
    );
    assert_eq!(
        full_window(&stop),
        (None, None, LimitStopReason::PlanWindow)
    );
}

#[test]
fn stop_with_no_reset_is_empty_after_a_read_that_timed_out() {
    let stop = stop_after_read(
        "stop_with_no_reset_is_empty_after_a_read_that_timed_out",
        CODEX,
        AgentKind::Codex,
        "codex-limit-air",
        ReadOutcome::Failed(ReadFailure::TimedOut),
    );
    assert_eq!(
        full_window(&stop),
        (None, None, LimitStopReason::PlanWindow)
    );
}

#[test]
fn stop_with_no_reset_is_empty_after_a_read_that_failed() {
    let failed = ReadFailure::Exited {
        code: Some(1),
        stderr_tail: String::new(),
    };
    let stop = stop_after_read(
        "stop_with_no_reset_is_empty_after_a_read_that_failed",
        CLAUDE,
        AgentKind::Claude,
        "session-failure",
        ReadOutcome::Failed(failed),
    );
    assert_eq!(
        full_window(&stop),
        (None, None, LimitStopReason::PlanWindow)
    );
}

#[test]
fn a_restart_before_the_read_logs_the_stop_once_at_start_up() {
    let mut rig = Rig::new(
        "a_restart_before_the_read_logs_the_stop_once_at_start_up",
        CLAUDE,
        AgentKind::Claude,
        Scenario::Bundled("session-failure"),
    );
    let turn = rig.turn();
    until_reads(&rig.host, &rig.reads, AgentKind::Claude, 2);
    let Rig {
        dir, host, thread, ..
    } = rig;
    // The read is still in flight when the host stops as a crash would.
    host.abort();

    let host = start(&dir, 1, &Reads::new());
    let mut events = host.events(StreamId::Thread(thread));
    events.until("TurnStoppedAtLimit", |e| e.position(is_stop).is_some());
    host.settle();
    events.drain_ready();
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
    host.shutdown();

    let host = start(&dir, 2, &Reads::new());
    host.settle();
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    host.settle();
    events.drain_ready();
    assert_eq!(events.count(is_stop), 1, "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_crash_while_two_stops_wait_logs_one_for_each_at_start_up() {
    let mut rig = Rig::new(
        "a_crash_while_two_stops_wait_logs_one_for_each_at_start_up",
        CLAUDE,
        AgentKind::Claude,
        Scenario::Local("session-failure-twice"),
    );
    let first = rig.turn();
    until_reads(&rig.host, &rig.reads, AgentKind::Claude, 2);
    // The second turn starts and stops while the first one's stop still waits.
    let second = rig.turn();
    assert_eq!(
        rig.events.turn_ended(second).unwrap().stop,
        StopReason::AtLimit
    );
    assert_eq!(stops(&rig.events), [], "{}", rig.events.dump());
    let Rig {
        dir, host, thread, ..
    } = rig;
    host.abort();

    let empty = |turn| TurnStoppedAtLimit {
        turn,
        window: None,
        resets_at: None,
        reason: LimitStopReason::PlanWindow,
    };
    let host = start(&dir, 1, &Reads::new());
    let mut events = host.events(StreamId::Thread(thread));
    events.until("two TurnStoppedAtLimit", |e| e.count(is_stop) == 2);
    host.settle();
    events.drain_ready();
    assert_eq!(
        stops(&events),
        [empty(first), empty(second)],
        "{}",
        events.dump()
    );
    host.shutdown();

    let host = start(&dir, 2, &Reads::new());
    host.settle();
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    host.settle();
    events.drain_ready();
    assert_eq!(
        stops(&events),
        [empty(first), empty(second)],
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn stop_with_a_reset_stays_in_the_end_commit_then_reads() {
    let mut rig = Rig::new(
        "stop_with_a_reset_stays_in_the_end_commit_then_reads",
        CLAUDE,
        AgentKind::Claude,
        Scenario::Bundled("limit-rejected"),
    );
    let turn = rig.turn();
    let stops = rig.until_stop();
    assert_eq!(
        stops,
        [TurnStoppedAtLimit {
            turn,
            window: Some("five_hour".to_owned()),
            resets_at: Some(Timestamp::new(1_792_310_400_000).unwrap()),
            reason: LimitStopReason::PlanWindow,
        }]
    );
    let ended = batch_of(&rig.events, |e| matches!(e, DomainEvent::TurnEnded(_)));
    assert_eq!(ended, batch_of(&rig.events, is_stop), "not one commit");
    until_reads(&rig.host, &rig.reads, AgentKind::Claude, 2);

    rig.reads.answer(AgentKind::Claude, full(AgentKind::Claude));
    rig.host.settle();
    rig.events.drain_ready();
    assert_eq!(rig.events.count(is_stop), 1, "{}", rig.events.dump());
    rig.host.shutdown();
}

/// A Codex prompt failing with -32603 (`limit-codex-error` behind codex-acp's handshake): one
/// read starts and the turn has not ended; returns the rig and the turn.
fn codex_error(test: &str) -> (Rig, TurnId) {
    codex_error_in(test, Scenario::Local("codex-limit-error"))
}

/// As [`codex_error`], with the agent playing `scenario`.
fn codex_error_in(test: &str, scenario: Scenario) -> (Rig, TurnId) {
    let mut rig = Rig::new(test, CODEX, AgentKind::Codex, scenario);
    let (turn, _) = rig.host.prompt(rig.thread, "Draft the clause.");
    until_reads(&rig.host, &rig.reads, AgentKind::Codex, 2);
    rig.host.settle();
    rig.events.drain_ready();
    assert!(
        rig.events.turn_ended(turn).is_none(),
        "the turn ended before its read: {}",
        rig.events.dump()
    );
    (rig, turn)
}

#[test]
fn codex_error_at_the_limit_ends_the_turn_at_the_limit() {
    let (mut rig, turn) = codex_error("codex_error_at_the_limit_ends_the_turn_at_the_limit");
    rig.reads.answer(AgentKind::Codex, full(AgentKind::Codex));
    let ended = rig.events.until_turn_ended(turn);
    rig.host.settle();
    rig.events.drain_ready();
    rig.events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::AtLimit);
    assert_eq!(
        turn_failed(&rig.events, turn),
        None,
        "{}",
        rig.events.dump()
    );
    assert_eq!(
        stops(&rig.events),
        [TurnStoppedAtLimit {
            turn,
            window: Some("five_hour".to_owned()),
            resets_at: Some(at(7200)),
            reason: LimitStopReason::PlanWindow,
        }]
    );
    let ended = batch_of(&rig.events, |e| matches!(e, DomainEvent::TurnEnded(_)));
    assert_eq!(ended, batch_of(&rig.events, is_stop), "not one commit");
    assert_eq!(
        rig.reads.count(AgentKind::Codex),
        2,
        "no read after the end"
    );
    rig.host.shutdown();
}

/// The turn failed with `agent.protocol_error`, as before the fallback, with no stop.
fn assert_protocol_error(rig: &mut Rig, turn: TurnId) {
    let ended = rig.events.until_turn_ended(turn);
    rig.host.settle();
    rig.events.drain_ready();
    assert_eq!(ended.stop, StopReason::Failed);
    let failed = turn_failed(&rig.events, turn).unwrap();
    assert_eq!(failed.code, ErrorCode::AgentProtocolError);
    assert_eq!(rig.events.count(is_stop), 0, "{}", rig.events.dump());
    assert_eq!(
        rig.reads.count(AgentKind::Codex),
        2,
        "no read after the end"
    );
}

#[test]
fn codex_error_under_the_limit_fails_as_before() {
    let (mut rig, turn) = codex_error("codex_error_under_the_limit_fails_as_before");
    rig.reads
        .answer(AgentKind::Codex, usage(AgentKind::Codex, 40.0, None));
    assert_protocol_error(&mut rig, turn);
    rig.host.shutdown();
}

#[test]
fn codex_error_whose_read_never_answers_ends_after_ten_seconds() {
    let (mut rig, turn) =
        codex_error("codex_error_whose_read_never_answers_ends_after_ten_seconds");
    rig.host.wait_armed(READ_WAIT);
    rig.host.clock.advance(Duration::from_millis(9_999));
    rig.host.settle();
    rig.events.drain_ready();
    assert!(rig.events.turn_ended(turn).is_none(), "ended before 10 s");
    rig.host.clock.advance(Duration::from_millis(1));
    assert_protocol_error(&mut rig, turn);
    rig.host.shutdown();
}

#[test]
fn codex_error_held_for_its_read_ends_once_through_cancel_and_shutdown() {
    let (mut rig, turn) =
        codex_error("codex_error_held_for_its_read_ends_once_through_cancel_and_shutdown");
    let outcome = rig
        .host
        .ok(clauseo_protocol::HostCommand::Cancel { thread: rig.thread });
    assert_eq!(outcome, clauseo_protocol::CommandOutcome::Done);
    rig.host.settle();
    rig.events.drain_ready();
    assert!(
        rig.events.turn_ended(turn).is_none(),
        "a cancel ended the held turn"
    );
    // Shutdown decides at once: the start-up read found no limit, so the turn fails as before.
    let Rig {
        dir, host, thread, ..
    } = rig;
    host.shutdown();

    let host = start(&dir, 1, &Reads::new());
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    host.settle();
    events.drain_ready();
    let ended = |e: &DomainEvent| matches!(e, DomainEvent::TurnEnded(ended) if ended.turn == turn);
    assert_eq!(events.count(ended), 1, "{}", events.dump());
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::Failed);
    let failed = turn_failed(&events, turn).unwrap();
    assert_eq!(
        failed.code,
        ErrorCode::AgentProtocolError,
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn codex_error_whose_agent_exits_during_the_read_ends_once_with_the_detach_last() {
    let (mut rig, turn) = codex_error_in(
        "codex_error_whose_agent_exits_during_the_read_ends_once_with_the_detach_last",
        Scenario::Local("codex-limit-error-exit"),
    );
    // The exit usually lands while the end waits for the read (its detach then joins the end
    // commit); nothing here can hold it back, so either order must end the turn once, with
    // the detach after the stop.
    rig.reads.answer(AgentKind::Codex, full(AgentKind::Codex));
    let ended = rig.events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::AtLimit);
    rig.events.until("the detach", |e| {
        e.position(|e| matches!(e, DomainEvent::AgentSessionDetached(_)))
            .is_some()
    });
    rig.host.settle();
    rig.events.drain_ready();
    let kinds = rig.events.kinds();
    let tail: Vec<&str> = kinds.iter().rev().take(3).rev().copied().collect();
    assert_eq!(
        tail,
        [
            "turn_ended",
            "turn_stopped_at_limit",
            "agent_session_detached"
        ],
        "{}",
        rig.events.dump()
    );
    let ends = |e: &DomainEvent| matches!(e, DomainEvent::TurnEnded(ended) if ended.turn == turn);
    assert_eq!(rig.events.count(ends), 1);
    rig.host.shutdown();
}
