//! Cancel: pending permissions and unfinished tool calls are closed first, late updates are
//! dropped, an agent that never answers is stopped after 5 s, and shutdown cancels too.

use std::time::Duration;

use clauseo_protocol::{
    CommandOutcome, DetachReason, DomainEvent, ErrorCode, HostCommand, PermissionChoice,
    StopReason, StreamId, ToolCallStatus,
};

use crate::observe::ended_with;
use crate::permissions::until_first_request;
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, wait};

/// The display batch: armed by a message's first delta, so seeing it armed means the agent's
/// first chunk reached the host.
const DISPLAY: Duration = Duration::from_millis(16);
/// The cancel fallback.
const CANCEL_WAIT: Duration = Duration::from_secs(5);

fn is_closed(event: &DomainEvent) -> bool {
    matches!(event, DomainEvent::AgentSessionDetached(d) if matches!(d.reason, DetachReason::Closed))
}

#[test]
fn cancel_resolves_permissions_and_tool_calls() {
    let dir = Dir::new(
        "cancel_resolves_permissions_and_tool_calls",
        &[(FAKE, Scenario::Bundled("permission-cancelled"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the brief.");
    until_first_request(&mut events);
    let request = events.permission_requests()[0];
    let tool_call = events
        .events()
        .find_map(|event| match event {
            DomainEvent::ToolCallStarted(started) if started.acp_id == "t-edit-1" => {
                Some(started.tool_call)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no ToolCallStarted t-edit-1: {}", events.dump()));
    let requested_at = events
        .position(|e| matches!(e, DomainEvent::PermissionRequested(_)))
        .unwrap();

    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::Cancelled);

    let resolved = events.position(|event| {
        matches!(event, DomainEvent::PermissionResolved(r)
            if r.request == request && r.choice == PermissionChoice::Cancelled)
    });
    let marked = events.position(|event| {
        matches!(event, DomainEvent::ToolCallUpdated(u)
            if u.tool_call == tool_call && u.patch.status == Some(ToolCallStatus::Cancelled))
    });
    let (Some(resolved), Some(marked)) = (resolved, marked) else {
        panic!(
            "want PermissionResolved(Cancelled) and Cancelled: {}",
            events.dump()
        );
    };
    let mut first = [resolved, marked];
    first.sort_unstable();
    assert_eq!(
        first,
        [requested_at + 1, requested_at + 2],
        "the cancel's events come first: {}",
        events.dump()
    );
    let late = events.count(|event| {
        matches!(event, DomainEvent::ToolCallUpdated(u)
            if u.tool_call == tool_call && u.patch.status == Some(ToolCallStatus::Failed))
    });
    assert_eq!(late, 0, "the late update was logged: {}", events.dump());
    let ends = events.count(|e| matches!(e, DomainEvent::TurnEnded(t) if t.turn == turn));
    assert_eq!(ends, 1);
    host.shutdown();
}

#[test]
fn cancel_drops_late_updates() {
    let dir = Dir::new(
        "cancel_drops_late_updates",
        &[(FAKE, Scenario::Bundled("cancel-late-updates"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DISPLAY);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(first);
    assert_eq!(ended.stop, StopReason::Cancelled);

    let (second, _) = host.prompt(thread, "Try again.");
    let ended = events.until_turn_ended(second);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);

    let all_text: String = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::TextAppended(appended) => Some(appended.text.as_str()),
            _ => None,
        })
        .collect();
    for late in ["caps liability", "at the fees", "Still writing."] {
        assert!(
            !all_text.contains(late),
            "late chunk {late:?} logged: {}",
            events.dump()
        );
    }
    assert_eq!(events.turn_text(second), "Second answer.");
    let completed = events.count(|event| {
        matches!(event, DomainEvent::ToolCallUpdated(u)
            if u.patch.status == Some(ToolCallStatus::Completed))
    });
    assert_eq!(
        completed,
        0,
        "the late tool_call_update was logged: {}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn cancel_unanswered_ends_after_5_s() {
    let dir = Dir::new(
        "cancel_unanswered_ends_after_5_s",
        &[(FAKE, Scenario::Bundled("cancel-unanswered"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the cap.");
    host.wait_armed(DISPLAY);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    host.wait_armed(CANCEL_WAIT);
    host.settle();
    events.drain_ready();
    assert!(
        events.turn_ended(turn).is_none(),
        "ended before 5 s: {}",
        events.dump()
    );

    host.clock.advance(CANCEL_WAIT);
    let ended = events.until_turn_ended(turn);
    events.until("AgentSessionDetached { Closed }", |e| {
        e.position(is_closed).is_some()
    });
    assert_eq!(ended.stop, StopReason::Cancelled);
    assert!(ended.checkpoint_after.is_some(), "no checkpoint_after");
    let closed = events.position(is_closed).unwrap();
    let end = events
        .position(|e| ended_with(e, turn, StopReason::Cancelled))
        .unwrap();
    assert!(
        closed < end,
        "the agent is stopped before the turn ends: {}",
        events.dump()
    );
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn shutdown_ends_turns_and_stops_agents() {
    let dir = Dir::new(
        "shutdown_ends_turns_and_stops_agents",
        &[(FAKE, Scenario::Bundled("permission-cancelled"))],
    );
    let mut host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the brief.");
    until_first_request(&mut events);
    let request = events.permission_requests()[0];

    let join = host.take_join();
    wait("shutdown", host.handle.shutdown()).unwrap_or_else(|e| panic!("shutdown: {e}"));
    events.until_end();
    events.assert_no_mismatch();
    let resolved = events.count(|event| {
        matches!(event, DomainEvent::PermissionResolved(r)
            if r.request == request && r.choice == PermissionChoice::Cancelled)
    });
    assert_eq!(resolved, 1, "{}", events.dump());
    assert!(
        events
            .turn_ended(turn)
            .is_some_and(|e| e.stop == StopReason::Cancelled)
    );
    assert_eq!(events.count(is_closed), 1, "{}", events.dump());
    join.join().unwrap_or_else(|e| panic!("join: {e}"));

    let later = host.dispatch(HostCommand::Cancel { thread }).unwrap_err();
    assert_eq!(later.code(), ErrorCode::Internal);
    assert!(!later.retryable());
    let later = host
        .query(clauseo_protocol::HostQuery::ListMatters)
        .unwrap_err();
    assert_eq!(later.code(), ErrorCode::Internal);
}
