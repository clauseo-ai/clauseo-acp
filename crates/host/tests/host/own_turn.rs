//! A turn the agent starts on its own (claude-agent-acp, once a command it put in the background
//! finishes): its tool call and text are never dropped. The host opens a turn for them with no
//! message of the lawyer's, shows and keeps it across a restart, gives it an Undo step, ends it
//! on the agent's cost-bearing usage update or after a quiet spell; a prompt queued meanwhile
//! waits behind it, and Send now cancels it (the prompt waiting out the late window); a
//! permission request with no turn opens one too and is asked, never cancelled.

use std::time::Duration;

use clauseo_protocol::{
    DeliveryRequest, DomainEvent, HostQuery, QueryResult, StopReason, StreamId, ThreadId, TurnId,
};

use crate::observe::Events;
use crate::permissions::until_first_request;
use crate::queue::{allow, send, settle, starts, until_queued};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, text};

/// How long an agent-started turn with nothing open waits for an update.
const QUIET: Duration = Duration::from_secs(60);
/// How long a cancelled agent-started turn's late window waits for the agent to go quiet.
const LATE_QUIET: Duration = Duration::from_secs(3);
/// The agent-started turn's text in every scenario.
const OWN_TEXT: &str = "The build finished: 3 warnings.";

/// The first turn that started other than `not`, once its `TurnStarted` is in.
fn until_other_start(events: &mut Events, not: TurnId) -> TurnId {
    let find = |e: &Events| {
        e.events().find_map(|event| match event {
            DomainEvent::TurnStarted(started) if started.turn != not => Some(started.turn),
            _ => None,
        })
    };
    events.until("an agent-started TurnStarted", |e| find(e).is_some());
    find(events).unwrap()
}

/// A host whose fake agent plays `scenario`, with a matter, a thread and the thread's events.
fn host_with(test: &str, scenario: &'static str) -> (Dir, Running, ThreadId, Events) {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let events = host.events(StreamId::Thread(thread));
    (dir, host, thread, events)
}

/// Whether `turn` has a step Undo can take: `PreviewUndo` answers for it.
fn has_undo_step(host: &Running, thread: ThreadId, turn: TurnId) -> bool {
    matches!(
        host.query(HostQuery::PreviewUndo { thread, turn }),
        Ok(QueryResult::UndoPreview(_))
    )
}

#[test]
fn an_agent_started_turn_is_shown_kept_and_undoable() {
    let (dir, host, thread, mut events) = host_with(
        "an_agent_started_turn_is_shown_kept_and_undoable",
        "own-turn-after-background",
    );
    let (first, _) = host.prompt(thread, "Run the build.");
    events.until("the Bash call", |e| {
        e.events().any(
            |event| matches!(event, DomainEvent::ToolCallStarted(t) if t.acp_id == "tc-bash-1"),
        )
    });
    let (turn, _) = send(
        &host,
        thread,
        text("Focus on clause 4"),
        DeliveryRequest::Steer,
    );
    assert_eq!(turn, first);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);

    let own = until_other_start(&mut events, first);
    assert_eq!(events.until_turn_ended(own).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    // No message of the lawyer's opened it.
    let users = events.count(|e| matches!(e, DomainEvent::UserMessage(m) if m.turn == own));
    assert_eq!(users, 0, "{}", events.dump());
    let tools = events.count(|e| {
        matches!(e, DomainEvent::ToolCallStarted(t) if t.turn == own && t.acp_id == "tc-read-2")
    });
    assert_eq!(tools, 1, "{}", events.dump());
    assert_eq!(events.turn_text(own), OWN_TEXT);
    let ended = events.turn_ended(own).unwrap();
    assert!(ended.checkpoint_after.is_some(), "{ended:?}");
    assert!(has_undo_step(&host, thread, own), "{}", events.dump());
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until_items(1);
    assert!(view.mentions(OWN_TEXT), "{}", view.dump());
    assert!(view.mentions("Read build.log"), "{}", view.dump());
    drop(view);
    drop(events);
    host.shutdown();

    // Kept: a restarted host shows the same turn from the log.
    let host = dir.start(1);
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until_items(1);
    assert!(view.mentions(OWN_TEXT), "{}", view.dump());
    assert!(view.mentions("Read build.log"), "{}", view.dump());
    assert!(has_undo_step(&host, thread, own));
    host.shutdown();
}

#[test]
fn a_prompt_queued_during_an_agent_started_turn_waits_for_its_end() {
    let (_dir, host, thread, mut events) = host_with(
        "a_prompt_queued_during_an_agent_started_turn_waits_for_its_end",
        "own-turn-queue",
    );
    let (first, _) = host.prompt(thread, "Run the build.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    let own = until_other_start(&mut events, first);

    let (next, _) = send(&host, thread, text("Next question"), DeliveryRequest::Queue);
    until_queued(&mut events, next);
    settle(&host, &mut events);
    assert_eq!(starts(&events, next), 0, "{}", events.dump());
    assert_eq!(events.last_queued(next).unwrap().behind_turn(), Some(own));

    // No usage update ends it: the quiet spell does.
    host.wait_armed(QUIET);
    host.clock.advance(QUIET);
    assert_eq!(events.until_turn_ended(own).stop, StopReason::EndTurn);
    assert_eq!(events.until_turn_ended(next).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(own), OWN_TEXT);
    assert_eq!(events.turn_text(next), "Next answer.");
    host.shutdown();
}

#[test]
fn send_now_cancels_an_agent_started_turn() {
    let (_dir, host, thread, mut events) = host_with(
        "send_now_cancels_an_agent_started_turn",
        "own-turn-send-now",
    );
    let (first, _) = host.prompt(thread, "Run the build.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    let own = until_other_start(&mut events, first);

    let (next, _) = send(
        &host,
        thread,
        text("Read clause 9"),
        DeliveryRequest::SendNow,
    );
    assert_eq!(events.until_turn_ended(own).stop, StopReason::Cancelled);
    // The agent sends no stop signal: the prompt sent now waits out the late window's quiet.
    host.wait_armed(LATE_QUIET);
    assert_eq!(starts(&events, next), 0, "{}", events.dump());
    host.clock.advance(LATE_QUIET);
    assert_eq!(events.until_turn_ended(next).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(events.turn_text(next), "Clause 9 says.");
    host.shutdown();
}

#[test]
fn a_permission_request_with_no_turn_opens_an_agent_started_turn_and_is_asked() {
    let (_dir, host, thread, mut events) = host_with(
        "a_permission_request_with_no_turn_opens_an_agent_started_turn_and_is_asked",
        "own-turn-permission",
    );
    let (first, _) = host.prompt(thread, "Run the build.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    let own = until_other_start(&mut events, first);
    until_first_request(&mut events);
    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(own).stop, StopReason::EndTurn);
    // `settle` checks the agent got its answer, not a cancel.
    settle(&host, &mut events);
    assert_eq!(events.turn_text(own), OWN_TEXT);
    host.shutdown();
}
