//! The queue across a restart and a steer that is never answered: a held queue whose last prompt
//! is a later turn that ended (sent now, then cancelled) comes back held and can be edited and
//! removed; a steer saved but not answered before the host stops comes back not sent, with no
//! queued turn; a steer the agent never answers, its turn ended, is not sent once its wait, which
//! starts at its send, runs out (`steer_wait.rs` has it going next while the turn runs on).

use std::time::Duration;

use clauseo_protocol::{
    CommandOutcome, DeliveryRequest, DomainEvent, HostCommand, SteerOutcome, StopReason, StreamId,
};

use crate::queue::{DURABLE, edited, held, send, settle, starts, steer_resolved, until_queued};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, text};

/// How long the host waits for a steer's answer.
const STEER_WAIT: Duration = Duration::from_secs(30);
/// The steered text the steer scenarios expect.
const STEER: &str = "Focus on clause 4";

#[test]
fn a_held_queue_behind_a_later_ended_turn_survives_a_restart() {
    let dir = Dir::new(
        "a_held_queue_behind_a_later_ended_turn_survives_a_restart",
        &[(FAKE, Scenario::Local("queue-held-after-send-now"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "One");
    host.wait_armed(DURABLE);
    let (queued, _) = send(&host, thread, text("Two"), DeliveryRequest::Queue);
    until_queued(&mut events, queued);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "not held after the cancel");
    // A prompt sent now runs ahead of the held one, then is cancelled too: the last prompt of
    // the log is now a turn that ended.
    let (now, _) = send(&host, thread, text("Three"), DeliveryRequest::SendNow);
    host.wait_armed(DURABLE);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(now).stop, StopReason::Cancelled);
    settle(&host, &mut events);
    assert!(
        held(&host, matter, thread),
        "not held after the second cancel"
    );
    assert_eq!(starts(&events, queued), 0, "{}", events.dump());
    drop(events);
    host.abort();

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    settle(&host, &mut events);
    assert!(
        held(&host, matter, thread),
        "the queue did not come back held"
    );
    // The host knows the turn again: it can be edited and removed.
    let edit = HostCommand::EditQueuedPrompt {
        thread,
        turn: queued,
        text: "Two, shorter".to_owned(),
    };
    assert_eq!(host.ok(edit), CommandOutcome::Done);
    events.until("QueuedPromptEdited", |e| edited(e, queued).is_some());
    let remove = HostCommand::RemoveQueuedPrompt {
        thread,
        turn: queued,
    };
    assert_eq!(host.ok(remove), CommandOutcome::Done);
    assert_eq!(events.until_turn_ended(queued).stop, StopReason::Cancelled);
    settle(&host, &mut events);
    assert_eq!(starts(&events, queued), 0, "{}", events.dump());
    assert!(!held(&host, matter, thread), "an emptied queue is not held");
    host.shutdown();
}

#[test]
fn a_steer_never_answered_comes_back_not_sent_after_a_restart() {
    let dir = Dir::new(
        "a_steer_never_answered_comes_back_not_sent_after_a_restart",
        &[(FAKE, Scenario::Local("steer-unanswered"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);
    let (turn, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(turn, first, "a joining steer answers with the running turn");
    settle(&host, &mut events);
    assert!(
        steer_resolved(&events, message).is_none(),
        "{}",
        events.dump()
    );
    drop(events);
    host.abort();

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    settle(&host, &mut events);
    let (_, resolved) = steer_resolved(&events, message)
        .unwrap_or_else(|| panic!("the steer was left unsettled: {}", events.dump()));
    assert_eq!(
        resolved.outcome,
        SteerOutcome::NotSent { turn: first },
        "{}",
        events.dump()
    );
    // Nothing waits: the steer is not a queued turn, and the queue is not held for it.
    assert!(!held(&host, matter, thread), "{}", events.dump());
    let turns = events.count(|e| matches!(e, DomainEvent::TurnStarted(_)));
    assert_eq!(turns, 1, "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_steer_with_no_answer_after_its_turn_ended_is_not_sent_when_its_wait_runs_out() {
    let dir = Dir::new(
        "a_steer_with_no_answer_after_its_turn_ended_is_not_sent_when_its_wait_runs_out",
        &[(FAKE, Scenario::Local("steer-no-answer"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);
    let (_, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    // The queue waits for the steer's answer: nothing starts yet. The wait was armed at the
    // send; the test clock has not moved since.
    assert!(
        steer_resolved(&events, message).is_none(),
        "{}",
        events.dump()
    );
    let turns = events.count(|e| matches!(e, DomainEvent::TurnStarted(_)));
    assert_eq!(turns, 1, "{}", events.dump());

    host.wait_armed(STEER_WAIT);
    host.clock.advance(STEER_WAIT);
    events.until("SteerResolved", |e| steer_resolved(e, message).is_some());
    let (_, resolved) = steer_resolved(&events, message).unwrap();
    assert_eq!(resolved.outcome, SteerOutcome::NotSent { turn: first });
    // `settle` checks the agent saw no second prompt.
    settle(&host, &mut events);
    let turns = events.count(|e| matches!(e, DomainEvent::TurnStarted(_)));
    assert_eq!(turns, 1, "{}", events.dump());
    host.shutdown();
}
