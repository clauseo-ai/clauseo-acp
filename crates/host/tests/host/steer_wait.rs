//! A message typed while Claude answers reaches it once, and only what reached it shows as
//! taken in: Send now with a permission pending cancels the turn (the permission answered
//! cancelled) and sends the message next; a steer while a permission is open is never sent as a
//! steer and waits as the next turn, which starts once the turn ends `end_turn` (the permission
//! allowed or rejected) and is not sent when the lawyer stops the turn; a steer with no answer
//! while the turn runs on goes next once its wait runs out; an unanswered steer sent now cancels
//! the turn and goes next; and on Codex, Send now cancels and sends. A Claude agent's steer
//! taken in only as far as its adapter's queue is in `steer_confirm.rs`.

use std::time::Duration;

use clauseo_protocol::{
    CommandOutcome, Delivery, DeliveryRequest, DomainEvent, HostCommand, MessageId,
    PermissionChoice, SteerOutcome, SteerResolved, StopReason, StreamId,
};

use crate::codex::CODEX;
use crate::observe::{Events, ended_with, started};
use crate::permissions::{answer_from, until_first_request};
use crate::queue::{DURABLE, allow, held, send, settle, starts, until_queued, user_message};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, text};

/// How long the host waits for a steer's answer.
const STEER_WAIT: Duration = Duration::from_secs(30);
/// The steered text the steer scenarios expect.
const STEER: &str = "Focus on clause 4";

/// Each `SteerResolved` of `message`, in order.
fn resolutions(events: &Events, message: MessageId) -> Vec<SteerResolved> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::SteerResolved(resolved) if resolved.message == message => {
                Some(resolved.clone())
            }
            _ => None,
        })
        .collect()
}

/// The outcomes of `message`'s `SteerResolved`s, in order.
fn outcomes(events: &Events, message: MessageId) -> Vec<SteerOutcome> {
    resolutions(events, message)
        .into_iter()
        .map(|r| r.outcome)
        .collect()
}

/// The turn `message` was requeued to, once its `SteerResolved { Requeued }` is in.
fn until_requeued(events: &mut Events, message: MessageId) -> clauseo_protocol::TurnId {
    events.until("SteerResolved { Requeued }", |e| {
        resolutions(e, message)
            .iter()
            .any(|r| matches!(r.outcome, SteerOutcome::Requeued { .. }))
    });
    resolutions(events, message)
        .into_iter()
        .find_map(|r| match r.outcome {
            SteerOutcome::Requeued { turn } => Some(turn),
            _ => None,
        })
        .unwrap_or_else(|| panic!("not requeued: {}", events.dump()))
}

/// A test's folder with its fake agent playing `scenario`, and its host started.
fn running(test: &str, scenario: &'static str) -> (Dir, Running) {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    (dir, host)
}

#[test]
fn send_now_with_a_permission_pending_cancels_and_sends_the_message_next() {
    let (dir, host) = running(
        "send_now_with_a_permission_pending_cancels_and_sends_the_message_next",
        "send-now-permission",
    );
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    until_first_request(&mut events);
    let request = events.permission_requests()[0];

    let (second, message) = send(
        &host,
        thread,
        text("Read clause 9"),
        DeliveryRequest::SendNow,
    );
    assert_ne!(second, first, "a message sent now is a new turn");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);

    let resolved = events.position(|e| {
        matches!(e, DomainEvent::PermissionResolved(r)
            if r.request == request && r.choice == PermissionChoice::Cancelled)
    });
    let first_end = events
        .position(|e| ended_with(e, first, StopReason::Cancelled))
        .unwrap();
    let second_start = events.position(|e| started(e, second)).unwrap();
    assert!(
        resolved.is_some_and(|at| at < first_end) && first_end < second_start,
        "{}",
        events.dump()
    );
    let (_, user) = user_message(&events, message);
    assert_eq!((user.turn, user.delivery), (second, Delivery::SendNow));
    assert_eq!(events.turn_text(second), "Focusing on clause 9.");
    host.shutdown();
}

#[test]
fn a_steer_while_a_permission_is_open_waits_as_the_next_turn() {
    let (dir, host) = running(
        "a_steer_while_a_permission_is_open_waits_as_the_next_turn",
        "steer-during-permission",
    );
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    until_first_request(&mut events);

    let (next, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_ne!(next, first, "a steer that cannot join now is the next turn");
    settle(&host, &mut events);
    let (_, user) = user_message(&events, message);
    assert_eq!((user.turn, user.delivery), (next, Delivery::Steer));
    assert_eq!(
        resolutions(&events, message)
            .into_iter()
            .map(|r| r.outcome)
            .collect::<Vec<_>>(),
        [SteerOutcome::Requeued { turn: next }],
        "{}",
        events.dump()
    );
    let waits = events.last_queued(next).unwrap();
    assert!(waits.is_at_head(), "{}", events.dump());
    assert_eq!(waits.behind_turn(), Some(first));
    assert_eq!(starts(&events, next), 0, "{}", events.dump());

    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(next).stop, StopReason::EndTurn);
    // `settle` checks the agent saw no `_session/steering`.
    settle(&host, &mut events);
    assert_eq!(events.turn_text(first), "The clause caps liability.");
    assert_eq!(events.turn_text(next), "Focusing on clause 4.");
    host.shutdown();
}

#[test]
fn a_steer_with_no_answer_while_the_turn_runs_goes_next_when_its_wait_runs_out() {
    let (dir, host) = running(
        "a_steer_with_no_answer_while_the_turn_runs_goes_next_when_its_wait_runs_out",
        "steer-no-answer-running",
    );
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);
    let (turn, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(turn, first, "a joining steer answers with the running turn");
    // The agent asks a permission after it got the steer; the turn runs on past the wait.
    until_first_request(&mut events);
    host.wait_armed(STEER_WAIT);
    host.clock.advance(STEER_WAIT);
    let next = until_requeued(&mut events, message);
    assert_ne!(next, first);
    settle(&host, &mut events);
    assert!(events.turn_ended(first).is_none(), "{}", events.dump());
    assert_eq!(starts(&events, next), 0, "{}", events.dump());

    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(next).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(next), "Focusing on clause 4.");
    host.shutdown();
}

#[test]
fn a_steer_held_behind_a_permission_goes_next_when_the_permission_is_rejected() {
    let (dir, host) = running(
        "a_steer_held_behind_a_permission_goes_next_when_the_permission_is_rejected",
        "steer-during-permission",
    );
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    until_first_request(&mut events);
    let (next, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    until_queued(&mut events, next);

    // Rejected, the agent carries on and ends `end_turn`: the held steer goes as the next turn.
    answer_from(&host, &mut events, thread, 0, &["reject-once"]);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(next).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(
        outcomes(&events, message),
        [SteerOutcome::Requeued { turn: next }],
        "{}",
        events.dump()
    );
    assert_eq!(events.turn_text(next), "Focusing on clause 4.");
    host.shutdown();
}

#[test]
fn a_steer_held_behind_a_permission_is_not_sent_when_the_turn_is_stopped() {
    let (dir, host) = running(
        "a_steer_held_behind_a_permission_is_not_sent_when_the_turn_is_stopped",
        "steer-held-stop",
    );
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    until_first_request(&mut events);
    let (next, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    until_queued(&mut events, next);

    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    assert_eq!(events.until_turn_ended(next).stop, StopReason::Cancelled);
    events.until("SteerResolved { NotSent }", |e| {
        outcomes(e, message).len() == 2
    });
    // `settle` checks the agent saw no second prompt.
    settle(&host, &mut events);
    assert_eq!(
        outcomes(&events, message),
        [
            SteerOutcome::Requeued { turn: next },
            SteerOutcome::NotSent { turn: first }
        ],
        "{}",
        events.dump()
    );
    assert_eq!(starts(&events, next), 0, "{}", events.dump());
    assert!(!held(&host, matter, thread), "an emptied queue is not held");
    host.shutdown();
}

#[test]
fn an_unanswered_steer_sent_now_cancels_the_turn_and_goes_next() {
    let (dir, host) = running(
        "an_unanswered_steer_sent_now_cancels_the_turn_and_goes_next",
        "steer-send-now",
    );
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);
    let (turn, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(turn, first, "a joining steer answers with the running turn");
    host.wait_armed(STEER_WAIT);

    // The row's Send now names the turn the steer was sent into.
    assert_eq!(
        host.ok(HostCommand::SendQueuedNow {
            thread,
            turn: first
        }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    let next = until_requeued(&mut events, message);
    assert_eq!(events.until_turn_ended(next).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(resolutions(&events, message).len(), 1, "{}", events.dump());
    assert_eq!(events.turn_text(next), "Focusing on clause 4.");
    host.shutdown();
}

/// Codex (`codex-acp`): Send now cancels the running turn and sends the message as the next
/// turn, with no steer.
#[test]
fn codex_send_now_cancels_then_prompts() {
    let dir = Dir::new(
        "codex_send_now_cancels_then_prompts",
        &[(CODEX, Scenario::Bundled("steer-fallback"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);

    let (second, message) = send(&host, thread, text(STEER), DeliveryRequest::SendNow);
    assert_ne!(second, first);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    let (_, user) = user_message(&events, message);
    assert_eq!((user.turn, user.delivery), (second, Delivery::SendNow));
    assert!(
        resolutions(&events, message).is_empty(),
        "{}",
        events.dump()
    );
    let first_end = events
        .position(|e| ended_with(e, first, StopReason::Cancelled))
        .unwrap();
    let second_start = events.position(|e| started(e, second)).unwrap();
    assert!(first_end < second_start, "{}", events.dump());
    assert_eq!(events.turn_text(second), "Focusing on clause 4.");
    host.shutdown();
}
