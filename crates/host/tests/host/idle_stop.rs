//! The idle stop at the host's interface (R20b, decision record 0052): an agent with nothing
//! pending for the idle limit (set short through `HostConfig::with_idle_limit`) is stopped, its
//! thread shows `IdleStopped`, and the next question relaunches it and resumes the session.
//! Every other test runs with the idle stop off ([`off`]).

use std::time::Duration;

use clauseo_host::HostConfig;
use clauseo_protocol::{DetachReason, DomainEvent, NoticeKind, StopReason, StreamId, ThreadStatus};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// The idle limit these tests set, the by-hand check's.
pub(crate) const LIMIT: Duration = Duration::from_secs(60);

/// The host config with the idle stop off: no idle limit and no live-agents cap, for every test
/// that is not about it (`support.rs`).
pub(crate) fn off(config: HostConfig) -> HostConfig {
    config
        .with_idle_limit(Duration::MAX)
        .with_live_agents(usize::MAX)
}

/// A host whose fake agent plays `scenario`, with the idle stop on at [`LIMIT`].
pub(crate) fn start(test: &str, scenario: &'static str) -> (Dir, Running) {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local(scenario))]);
    let host = dir.start_with(0, |config| {
        config.with_idle_limit(LIMIT).with_live_agents(2)
    });
    (dir, host)
}

/// Whether `event` is the idle stop's detach.
pub(crate) fn idle_stopped(event: &DomainEvent) -> bool {
    matches!(event, DomainEvent::AgentSessionDetached(detached)
        if detached.reason == DetachReason::IdleStopped)
}

/// Whether `event` is an entry of the conversation an agent update makes.
fn update_entry(event: &DomainEvent) -> bool {
    matches!(
        event,
        DomainEvent::TextAppended(_)
            | DomainEvent::ContentAppended(_)
            | DomainEvent::ToolCallStarted(_)
            | DomainEvent::ToolCallUpdated(_)
    )
}

/// Asks the first question, sees its turn end, lets the limit pass and sees the agent stopped
/// idle, the thread `IdleStopped`; then asks the second, which must end `EndTurn`. Returns the
/// events.
fn stop_then_ask_again(dir: &Dir, host: &Running) -> Events {
    let (matter, thread, _) = matter_with_thread(dir, host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Remember the word heron.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    host.wait_armed(LIMIT);
    host.clock.advance(LIMIT);
    events.until("the idle stop", |events| {
        events.position(idle_stopped).is_some()
    });
    host.settle();
    assert_eq!(host.status(matter, thread), ThreadStatus::IdleStopped);
    let entries = events.count(update_entry);

    let (second, _) = host.prompt(thread, "What was the word?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    assert!(
        events.turn_text(second).contains("heron"),
        "{}",
        events.dump()
    );
    // Nothing the resume replayed was logged: the first turn's entries are as they were, and
    // every entry added since is the second turn's answer.
    let detached = events.position(idle_stopped).unwrap();
    let started = events
        .position(|event| crate::observe::started(event, second))
        .unwrap();
    let between = events
        .events()
        .skip(detached)
        .take(started - detached)
        .filter(|event| update_entry(event))
        .count();
    assert_eq!(between, 0, "{}", events.dump());
    assert!(events.count(update_entry) > entries);
    host.settle();
    assert_eq!(host.status(matter, thread), ThreadStatus::Idle);
    events.assert_no_mismatch();
    events
}

#[test]
fn an_idle_agent_stops_and_its_next_question_loads_the_session() {
    let (dir, host) = start("idle_stop_load", "idle-stop-then-load");
    let events = stop_then_ask_again(&dir, &host);
    let notices = events.count(|event| {
        matches!(event, DomainEvent::Notice(notice)
            if notice.kind == Some(NoticeKind::ContextNotRestored))
    });
    assert_eq!(notices, 0, "{}", events.dump());
    host.shutdown();
}

#[test]
fn an_agent_without_load_session_resumes_with_session_resume() {
    let (dir, host) = start("idle_stop_resume", "idle-stop-then-resume");
    let events = stop_then_ask_again(&dir, &host);
    let notices = events.count(|event| matches!(event, DomainEvent::Notice(_)));
    assert_eq!(notices, 0, "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_failed_load_after_an_idle_stop_starts_a_new_session_with_the_notice() {
    let (dir, host) = start("idle_stop_load_fails", "idle-stop-then-load-fails");
    let events = stop_then_ask_again(&dir, &host);
    let notices = events.count(|event| {
        matches!(event, DomainEvent::Notice(notice)
            if notice.kind == Some(NoticeKind::ContextNotRestored))
    });
    assert_eq!(notices, 1, "{}", events.dump());
    host.shutdown();
}
