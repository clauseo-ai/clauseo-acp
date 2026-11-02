//! Steer at the interface: a steer the agent takes into the running turn, one that comes after
//! the turn's end and goes next, the send-now fallback for an agent without a real steer, and a
//! steer on an idle thread.

use clauseo_protocol::{
    Delivery, DeliveryRequest, DomainEvent, SteerOutcome, StopReason, StreamId,
};

use crate::observe::{ended_with, started};
use crate::queue::{DURABLE, send, settle, steer_resolved, user_message};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, text};

/// The steered text every steer scenario expects.
const STEER: &str = "Focus on clause 4";

/// The thread header's `steer_joins_turn` in a fresh view of `thread`.
fn steer_joins_turn(host: &Running, thread: clauseo_protocol::ThreadId) -> bool {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until_items(1);
    let json = view.items[0].json();
    let field = "\"steer_joins_turn\":";
    let at = json
        .find(field)
        .unwrap_or_else(|| panic!("no steer_joins_turn in {json}"));
    json[at + field.len()..].starts_with("true")
}

#[test]
fn steer_joins_the_running_turn() {
    let dir = Dir::new(
        "steer_joins_the_running_turn",
        &[(FAKE, Scenario::Bundled("steer-injected"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);

    let (turn, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(turn, first, "a joining steer answers with the running turn");
    events.until("SteerResolved", |e| steer_resolved(e, message).is_some());
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    settle(&host, &mut events);

    let (user_at, user) = user_message(&events, message);
    assert_eq!((user.turn, user.delivery), (first, Delivery::Steer));
    let (resolved_at, resolved) = steer_resolved(&events, message).unwrap();
    assert!(user_at < resolved_at, "{}", events.dump());
    assert_eq!(resolved.outcome, SteerOutcome::Injected { turn: first });
    let ends = events.count(|e| matches!(e, DomainEvent::TurnEnded(t) if t.turn == first));
    assert_eq!(ends, 1, "{}", events.dump());
    assert_eq!(
        events.turn_text(first),
        "The clause caps liability at the fees paid."
    );
    assert!(
        steer_joins_turn(&host, thread),
        "claude-agent-acp's steer joins"
    );
    host.shutdown();
}

#[test]
fn steer_after_the_end_goes_next() {
    let dir = Dir::new(
        "steer_after_the_end_goes_next",
        &[(FAKE, Scenario::Bundled("steer-prompt-required"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);

    let (turn, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(turn, first);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    events.until("SteerResolved", |e| steer_resolved(e, message).is_some());
    let (resolved_at, resolved) = steer_resolved(&events, message).unwrap();
    let SteerOutcome::Requeued { turn: second } = resolved.outcome else {
        panic!("not requeued: {}", events.dump());
    };
    assert_ne!(second, first);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);

    let queued = events.last_queued(second).unwrap();
    assert_eq!(
        (queued.behind(), queued.behind_turn()),
        (thread, Some(first))
    );
    let queued_at = crate::queue::queued_at(&events, second);
    assert_eq!(
        events.batch[resolved_at],
        events.batch[queued_at],
        "{}",
        events.dump()
    );
    let first_end = events
        .position(|e| ended_with(e, first, StopReason::EndTurn))
        .unwrap();
    let second_start = events.position(|e| started(e, second)).unwrap();
    assert!(first_end < second_start, "{}", events.dump());
    assert_eq!(events.turn_text(second), "Focusing on clause 4.");
    host.shutdown();
}

#[test]
fn steer_falls_back_to_send_now() {
    let dir = Dir::new(
        "steer_falls_back_to_send_now",
        &[(FAKE, Scenario::Bundled("steer-fallback"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);

    let (second, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_ne!(second, first, "a steer that cannot join is a new turn");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);

    let (user_at, user) = user_message(&events, message);
    assert_eq!((user.turn, user.delivery), (second, Delivery::Steer));
    let (resolved_at, resolved) = steer_resolved(&events, message)
        .unwrap_or_else(|| panic!("no SteerResolved: {}", events.dump()));
    assert_eq!(resolved.outcome, SteerOutcome::SentNow { turn: second });
    assert_eq!(
        events.batch[user_at],
        events.batch[resolved_at],
        "{}",
        events.dump()
    );
    let queued = events.last_queued(second).unwrap();
    assert_eq!(queued.behind_turn(), Some(first));
    assert_eq!(events.turn_text(first), "The clause ");
    assert_eq!(events.turn_text(second), "Focusing on clause 4.");
    assert!(
        !steer_joins_turn(&host, thread),
        "codex-acp's steer does not join"
    );
    host.shutdown();
}

#[test]
fn steer_with_no_turn_sends_now() {
    let dir = Dir::new(
        "steer_with_no_turn_sends_now",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));

    let (turn, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    let (user_at, user) = user_message(&events, message);
    assert_eq!((user.turn, user.delivery), (turn, Delivery::Steer));
    let (resolved_at, resolved) = steer_resolved(&events, message)
        .unwrap_or_else(|| panic!("no SteerResolved: {}", events.dump()));
    assert_eq!(resolved.outcome, SteerOutcome::SentNow { turn });
    assert_eq!(
        events.batch[user_at],
        events.batch[resolved_at],
        "{}",
        events.dump()
    );
    assert!(
        events.last_queued(turn).is_none(),
        "an idle thread queues nothing: {}",
        events.dump()
    );
    host.shutdown();
}
