//! A requeued steer keeps its head place across a host restart, as it has live: a steer the
//! agent answers `promptRequired` while its turn runs goes to the queue's head, ahead of the
//! prompts queued before it. After a restart it still runs first, then the prompts in order,
//! and the view orders the queue the same way before and after the restart.

use clauseo_protocol::{
    CommandOutcome, DeliveryRequest, HostCommand, SteerOutcome, StopReason, StreamId, TurnId,
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

#[test]
fn a_requeued_steer_runs_first_after_a_host_restart() {
    let dir = Dir::new(
        "a_requeued_steer_runs_first_after_a_host_restart",
        &[(FAKE, Scenario::Local("steer-requeued-restart"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
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
