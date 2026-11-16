//! A requeued steer keeps its head place across a host restart, as it has live: a steer the
//! agent answers `promptRequired` while its turn runs goes to the queue's head, ahead of the
//! prompts queued before it. After a restart it still runs first, then the prompts in order,
//! and the view orders the queue the same way before and after the restart.

use clauseo_protocol::{
    CommandOutcome, DeliveryRequest, DomainEvent, HostCommand, MatterId, SteerOutcome, StopReason,
    StreamId, ThreadId, TurnId,
};

use crate::observe::started;
use crate::queue::{DURABLE, held, send, settle, starts, steer_resolved, until_queued, view_queue};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, text};

/// The steered text the scenario expects.
const STEER: &str = "Focus on clause 4";

/// `turn`'s `TurnStarted` position in `events`.
fn started_at(events: &crate::observe::Events, turn: TurnId) -> usize {
    events
        .position(|event| started(event, turn))
        .unwrap_or_else(|| panic!("{turn} never started: {}", events.dump()))
}

/// The first run: prompt 1 runs, "Two" and "Three" queue, and a steer sent into prompt 1 is
/// requeued at the head; the host then stops with prompt 1 open. Returns the matter, the thread
/// and the turns of the steer, "Two" and "Three".
fn requeued_then_stopped(dir: &Dir) -> (MatterId, ThreadId, [TurnId; 3]) {
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Look at the matter.");
    host.wait_armed(DURABLE);
    // The two prompts queue before the steer, so the requeued steer is the one that jumps them.
    let (two, _) = send(&host, thread, text("Two"), DeliveryRequest::Queue);
    until_queued(&mut events, two);
    let (three, _) = send(&host, thread, text("Three"), DeliveryRequest::Queue);
    until_queued(&mut events, three);
    let (turn, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(turn, first, "a joining steer answers with the running turn");
    events.until("SteerResolved", |e| steer_resolved(e, message).is_some());
    let (_, resolved) = steer_resolved(&events, message).unwrap();
    let SteerOutcome::Requeued { turn: steer } = resolved.outcome else {
        panic!("not requeued: {}", events.dump());
    };
    settle(&host, &mut events);
    assert_eq!(starts(&events, steer), 0, "{}", events.dump());
    // Live, the requeued steer is at the head; the view shows the host's order.
    assert_eq!(
        view_queue(&host, thread),
        [steer, two, three],
        "{}",
        events.dump()
    );
    drop(events);
    host.abort();
    (matter, thread, [steer, two, three])
}

#[test]
fn a_requeued_steer_runs_first_after_a_host_restart() {
    let dir = Dir::new(
        "a_requeued_steer_runs_first_after_a_host_restart",
        &[(FAKE, Scenario::Local("steer-requeued-restart"))],
    );
    let (matter, thread, [steer, two, three]) = requeued_then_stopped(&dir);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "not held after the restart");
    assert_eq!(
        view_queue(&host, thread),
        [steer, two, three],
        "{}",
        events.dump()
    );
    // Send next: the steer goes first, then the two queued behind it, in order (the scenario's
    // prompts check the texts' order too).
    assert_eq!(
        host.ok(HostCommand::ResumeQueue { thread }),
        CommandOutcome::Done
    );
    for turn in [steer, two, three] {
        assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    }
    settle(&host, &mut events);
    assert!(started_at(&events, steer) < started_at(&events, two));
    assert!(
        started_at(&events, two) < started_at(&events, three),
        "{}",
        events.dump()
    );
    assert!(!held(&host, matter, thread));
    events.assert_no_mismatch();
    host.shutdown();
}

/// Whether `event` names `turn` as its turn: its queueing, start, end or answer.
fn names(event: &DomainEvent, turn: TurnId) -> bool {
    match event {
        DomainEvent::TurnQueued(queued) => queued.turn() == turn,
        DomainEvent::TurnStarted(started) => started.turn == turn,
        DomainEvent::TurnEnded(ended) => ended.turn == turn,
        DomainEvent::TextAppended(appended) => appended.turn == turn,
        _ => false,
    }
}

/// A second restart with the requeued steer still held: the turn it was sent into has ended
/// (the first restart's repair), no turn runs, and prompts wait behind the steer. Start-up's
/// repair of agent-started turns must not take the steer's wait for one: it comes back held
/// and not ended, runs once on Resume, and nothing names its turn after its end.
#[test]
fn a_requeued_steer_still_held_survives_a_second_restart() {
    let dir = Dir::new(
        "a_requeued_steer_still_held_survives_a_second_restart",
        &[(FAKE, Scenario::Local("steer-requeued-restart"))],
    );
    let (matter, thread, [steer, two, three]) = requeued_then_stopped(&dir);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    settle(&host, &mut events);
    assert!(
        held(&host, matter, thread),
        "not held after the first restart"
    );
    drop(events);
    host.abort();

    let host = dir.start(2);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    settle(&host, &mut events);
    assert!(events.turn_ended(steer).is_none(), "{}", events.dump());
    assert!(
        held(&host, matter, thread),
        "not held after the second restart"
    );
    assert_eq!(
        view_queue(&host, thread),
        [steer, two, three],
        "{}",
        events.dump()
    );
    assert_eq!(
        host.ok(HostCommand::ResumeQueue { thread }),
        CommandOutcome::Done
    );
    for turn in [steer, two, three] {
        assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    }
    settle(&host, &mut events);
    assert_eq!(starts(&events, steer), 1, "{}", events.dump());
    let ends = events.count(|e| matches!(e, DomainEvent::TurnEnded(ended) if ended.turn == steer));
    assert_eq!(ends, 1, "{}", events.dump());
    let end = events
        .position(|e| matches!(e, DomainEvent::TurnEnded(ended) if ended.turn == steer))
        .unwrap();
    let last = events
        .events()
        .enumerate()
        .filter(|(_, e)| names(e, steer))
        .last();
    assert_eq!(last.map(|(at, _)| at), Some(end), "{}", events.dump());
    assert!(started_at(&events, steer) < end, "{}", events.dump());
    assert!(started_at(&events, steer) < started_at(&events, two));
    assert!(!held(&host, matter, thread));
    events.assert_no_mismatch();
    host.shutdown();
}
