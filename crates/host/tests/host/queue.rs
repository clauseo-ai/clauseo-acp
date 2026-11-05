//! The thread's queue at the interface: a prompt sent while a turn runs waits and starts after
//! an `EndTurn`; the cap; every other stop (a cancel, a refusal that ended the turn, the usage
//! limit, the agent exiting) holds the queue until `ResumeQueue`; archive waits for the queue.
//! The helpers here serve `queue_moves.rs` and `steer.rs` too.

use std::time::Duration;

use clauseo_protocol::{
    CommandOutcome, Delivery, DeliveryRequest, DomainEvent, ErrorCode, HostCommand,
    MAX_QUEUED_TURNS, MatterId, MessageId, PromptBlock, QueuedPromptEdited, SteerResolved,
    StopReason, StreamId, ThreadId, TurnId, UserMessage, ViewEntry, ViewPatchBody,
};

use crate::codex::CODEX;
use crate::observe::{Events, ended_with, started};
use crate::permissions::{answer_from, until_first_request};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, send_prompt};

/// The durable batch: armed by a message's first un-durable delta, so seeing it armed means the
/// agent's first chunk reached the host and the turn is prompting.
pub(crate) const DURABLE: Duration = Duration::from_millis(250);
/// The cancel fallback.
pub(crate) const CANCEL_WAIT: Duration = Duration::from_secs(5);

/// Sends `content` with `delivery`; returns the turn and message `PromptAccepted` names.
pub(crate) fn send(
    host: &Running,
    thread: ThreadId,
    content: Vec<PromptBlock>,
    delivery: DeliveryRequest,
) -> (TurnId, MessageId) {
    let outcome = host.ok(HostCommand::SendPrompt {
        thread,
        content,
        delivery,
    });
    match outcome {
        CommandOutcome::PromptAccepted { turn, message } => (turn, message),
        other => panic!("SendPrompt answered {other:?}"),
    }
}

/// The index in `events.seen` of `message`'s `UserMessage`, and the event.
pub(crate) fn user_message(events: &Events, message: MessageId) -> (usize, UserMessage) {
    events
        .seen
        .iter()
        .enumerate()
        .find_map(|(at, envelope)| match &envelope.event {
            DomainEvent::UserMessage(user) if user.message == message => Some((at, user.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no UserMessage {message}: {}", events.dump()))
}

/// The index of `turn`'s first `TurnQueued`.
pub(crate) fn queued_at(events: &Events, turn: TurnId) -> usize {
    events
        .position(|event| matches!(event, DomainEvent::TurnQueued(q) if q.turn() == turn))
        .unwrap_or_else(|| panic!("no TurnQueued of {turn}: {}", events.dump()))
}

/// `message`'s `SteerResolved` and its index, when received.
pub(crate) fn steer_resolved(
    events: &Events,
    message: MessageId,
) -> Option<(usize, SteerResolved)> {
    events
        .seen
        .iter()
        .enumerate()
        .find_map(|(at, envelope)| match &envelope.event {
            DomainEvent::SteerResolved(resolved) if resolved.message == message => {
                Some((at, resolved.clone()))
            }
            _ => None,
        })
}

/// `turn`'s last `QueuedPromptEdited`, when received.
pub(crate) fn edited(events: &Events, turn: TurnId) -> Option<QueuedPromptEdited> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::QueuedPromptEdited(edit) if edit.turn == turn => Some(edit.clone()),
            _ => None,
        })
        .last()
}

/// Whether `ListThreads` shows `thread`'s queue held.
pub(crate) fn held(host: &Running, matter: MatterId, thread: ThreadId) -> bool {
    host.threads(matter)
        .into_iter()
        .find(|summary| summary.thread == thread)
        .unwrap_or_else(|| panic!("thread {thread} not listed"))
        .queue_held
}

/// How many `TurnStarted` of `turn` were received.
pub(crate) fn starts(events: &Events, turn: TurnId) -> usize {
    events.count(|event| started(event, turn))
}

/// Answers the thread's first permission request with `allow-once`.
pub(crate) fn allow(host: &Running, events: &mut Events, thread: ThreadId) {
    answer_from(host, events, thread, 0, &["allow-once"]);
}

/// Takes everything the host published so far and checks the agent saw no mismatch.
pub(crate) fn settle(host: &Running, events: &mut Events) {
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
}

/// Waits until `turn`'s `TurnQueued` is in.
pub(crate) fn until_queued(events: &mut Events, turn: TurnId) {
    events.until("TurnQueued", |e| e.last_queued(turn).is_some());
}

/// The turns of the queued prompts in a fresh view of `thread` (its `Reset`), in the order the
/// view orders them (`clauseo_reducer::queued_order`, as the app's queue strip does).
pub(crate) fn view_queue(host: &Running, thread: ThreadId) -> Vec<TurnId> {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until("the view's reset", |view| view.patches().next().is_some());
    let Some(ViewPatchBody::Reset { view_window }) = view.patches().next().map(|patch| &patch.body)
    else {
        panic!("the view did not start with a reset: {}", view.dump());
    };
    let entries = &view_window.entries;
    clauseo_reducer::queued_order(entries)
        .into_iter()
        .map(|message| {
            entries
                .iter()
                .find_map(|entry| match entry {
                    ViewEntry::UserMessage(prompt) if prompt.message == message => {
                        Some(prompt.turn)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no prompt {message}: {}", view.dump()))
        })
        .collect()
}

#[test]
fn queued_prompt_starts_after_end_turn() {
    let dir = Dir::new(
        "queued_prompt_starts_after_end_turn",
        &[(FAKE, Scenario::Bundled("queue-two-prompts"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Check clause 4.");
    until_first_request(&mut events);

    let (second, message) = send(
        &host,
        thread,
        crate::support::text("Second question"),
        DeliveryRequest::Queue,
    );
    assert_ne!(second, first);
    until_queued(&mut events, second);
    let queued = events.last_queued(second).unwrap();
    assert_eq!(
        (queued.behind(), queued.behind_turn()),
        (thread, Some(first))
    );
    let (user_at, user) = user_message(&events, message);
    assert_eq!((user.turn, user.delivery), (second, Delivery::Queue));
    let queued_at = queued_at(&events, second);
    assert_eq!(queued_at, user_at + 1, "{}", events.dump());
    assert_eq!(
        events.batch[user_at],
        events.batch[queued_at],
        "UserMessage and TurnQueued in one commit: {}",
        events.dump()
    );
    assert_eq!(starts(&events, second), 0);

    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(second), "Second answer.");
    let first_end = events
        .position(|e| ended_with(e, first, StopReason::EndTurn))
        .unwrap();
    let second_start = events.position(|e| started(e, second)).unwrap();
    assert!(first_end < second_start, "{}", events.dump());
    host.shutdown();
}

#[test]
fn queue_full_at_the_limit() {
    let dir = Dir::new(
        "queue_full_at_the_limit",
        &[(FAKE, Scenario::Bundled("cancel-unanswered"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DURABLE);

    let queued: Vec<TurnId> = (1..=MAX_QUEUED_TURNS)
        .map(|n| host.prompt(thread, &format!("Question {n}")).0)
        .collect();
    let last = *queued.last().unwrap();
    until_queued(&mut events, last);
    settle(&host, &mut events);
    let head = events.head();
    let error = host.refused(
        send_prompt(thread, "One question too many"),
        ErrorCode::ThreadQueueFull,
    );
    assert!(error.retryable());
    settle(&host, &mut events);
    assert_eq!(
        events.head(),
        head,
        "the refused prompt logged: {}",
        events.dump()
    );

    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    host.wait_armed(CANCEL_WAIT);
    host.clock.advance(CANCEL_WAIT);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "the queue is not held");
    for turn in &queued {
        assert_eq!(starts(&events, *turn), 0, "{}", events.dump());
        assert!(events.turn_ended(*turn).is_none(), "{}", events.dump());
    }
    host.shutdown();
}

/// How the first turn of [`held_until_resume_queue`] ends.
#[derive(Debug, Clone, Copy)]
enum FirstEnd {
    /// The lawyer presses Stop: `Cancelled`.
    Stop,
    /// The lawyer refuses Codex's permission request and Codex ends the turn: `NotAllowed`.
    Deny,
}

/// The first turn ends by `end` with `stop` while a second prompt waits: the queue holds
/// until `ResumeQueue`, which starts the second (answered `text`); on an empty queue
/// `ResumeQueue` logs nothing.
fn held_until_resume_queue(
    test: &str,
    agent: &str,
    scenario: &'static str,
    end: FirstEnd,
    stop: StopReason,
    text: &str,
) {
    let dir = Dir::new(test, &[(agent, Scenario::Bundled(scenario))]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], agent);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Find the cap.");
    match end {
        FirstEnd::Stop => host.wait_armed(DURABLE),
        FirstEnd::Deny => until_first_request(&mut events),
    }
    let (second, _) = host.prompt(thread, "Use the 2019 version instead");
    until_queued(&mut events, second);
    assert!(!held(&host, matter, thread), "held before any stop");

    match end {
        FirstEnd::Stop => assert_eq!(
            host.ok(HostCommand::Cancel { thread }),
            CommandOutcome::Done
        ),
        FirstEnd::Deny => answer_from(&host, &mut events, thread, 0, &["cancel"]),
    }
    assert_eq!(
        events.until_turn_ended(first).stop,
        stop,
        "{}",
        events.dump()
    );
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "the queue is not held");
    assert_eq!(starts(&events, second), 0, "{}", events.dump());

    assert_eq!(
        host.ok(HostCommand::ResumeQueue { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(second), text);
    assert!(!held(&host, matter, thread), "still held after the resume");

    // On an empty queue it answers Done and logs nothing (its receipt is not an event).
    let head = events.head();
    assert_eq!(
        host.ok(HostCommand::ResumeQueue { thread }),
        CommandOutcome::Done
    );
    settle(&host, &mut events);
    assert_eq!(events.head(), head, "{}", events.dump());
    host.shutdown();
}

#[test]
fn held_after_cancel_until_resume_queue() {
    held_until_resume_queue(
        "held_after_cancel_until_resume_queue",
        FAKE,
        "send-now",
        FirstEnd::Stop,
        StopReason::Cancelled,
        "Using the 2019 version. ",
    );
}

/// A turn Codex ended on the lawyer's refusal holds the queue as a Stop does (private #361).
#[test]
fn held_after_not_allowed_until_resume_queue() {
    held_until_resume_queue(
        "held_after_not_allowed_until_resume_queue",
        CODEX,
        "codex-deny-ends-turn",
        FirstEnd::Deny,
        StopReason::NotAllowed,
        "Working inside the matter instead. ",
    );
}

#[test]
fn held_after_a_limit_stop() {
    let dir = Dir::new(
        "held_after_a_limit_stop",
        &[(FAKE, Scenario::Local("queue-limit-stop"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Draft the clause.");
    until_first_request(&mut events);
    let (second, _) = host.prompt(thread, "Second question");
    until_queued(&mut events, second);

    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::AtLimit);
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "the queue is not held");
    assert_eq!(starts(&events, second), 0, "{}", events.dump());

    assert_eq!(
        host.ok(HostCommand::ResumeQueue { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(second), "Second answer.");
    host.shutdown();
}

#[test]
fn held_after_agent_exit_then_resume() {
    let dir = Dir::new(
        "held_after_agent_exit_then_resume",
        &[(FAKE, Scenario::Local("queue-exit-then-load"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Check clause 4.");
    until_first_request(&mut events);
    let (second, _) = host.prompt(thread, "Second question");
    until_queued(&mut events, second);

    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    settle(&host, &mut events);
    let exited = events.count(|event| {
        matches!(event, DomainEvent::TurnFailed(f)
            if f.turn == first && f.code == ErrorCode::AgentExited)
    });
    assert_eq!(exited, 1, "{}", events.dump());
    assert!(held(&host, matter, thread), "the queue is not held");
    assert_eq!(starts(&events, second), 0, "{}", events.dump());

    assert_eq!(
        host.ok(HostCommand::ResumeQueue { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(second), "Second answer.");
    host.shutdown();
}

#[test]
fn archive_refused_while_turns_wait() {
    let dir = Dir::new(
        "archive_refused_while_turns_wait",
        &[(FAKE, Scenario::Bundled("send-now"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DURABLE);
    let (second, _) = host.prompt(thread, "Use the 2019 version instead");
    until_queued(&mut events, second);
    host.ok(HostCommand::Cancel { thread });
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    settle(&host, &mut events);
    assert!(held(&host, matter, thread));

    // No turn runs, but one waits: an archived thread could never send it.
    host.refused(
        HostCommand::ArchiveThread { thread },
        ErrorCode::ThreadTurnRunning,
    );

    host.ok(HostCommand::ResumeQueue { thread });
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(
        host.ok(HostCommand::ArchiveThread { thread }),
        CommandOutcome::Done
    );
    host.shutdown();
}
