//! The agent's own name from `initialize`'s `agentInfo.title`: logged once, before the session
//! starts, so the thread header carries it when `session/new` fails with `auth_required`; no
//! title, no `AgentNamed`.

use clauseo_protocol::{
    DomainEvent, ErrorCode, StopReason, StreamHeader, StreamId, ThreadId, ViewPatchBody,
};

use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// `thread`'s header name, as a view subscription delivers its header: `None` when the header
/// holds no name.
fn header_name(host: &crate::support::Running, thread: ThreadId) -> Option<String> {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until("the thread's view reset", |v| v.resets() > 0);
    let window = view
        .patches()
        .find_map(|patch| match &patch.body {
            ViewPatchBody::Reset { view_window } => Some(view_window.clone()),
            _ => None,
        })
        .expect("a reset");
    let Some(StreamHeader::Thread(header)) = window.header else {
        panic!("a reset with no thread header: {:?}", view.dump());
    };
    header.agent_name.map(|name| name.to_string())
}

/// Whether `event` is `AgentNamed` with `name`.
fn named(name: &'static str) -> impl Fn(&DomainEvent) -> bool {
    move |event| matches!(event, DomainEvent::AgentNamed(named) if named.agent_name == name)
}

#[test]
fn titled_agent_logs_its_name_before_session_new_fails() {
    let dir = Dir::new(
        "agent_name_titled",
        &[(FAKE, Scenario::Bundled("auth-required-titled"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the lease.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    let failed = events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
        _ => None,
    });
    assert_eq!(
        failed,
        Some(ErrorCode::AgentAuthRequired),
        "{}",
        events.dump()
    );
    // The name is in the log before anything the failed launch left behind.
    let name_at = events
        .position(named("Fake Agent"))
        .unwrap_or_else(|| panic!("{}", events.dump()));
    let failure_at = events
        .position(|event| matches!(event, DomainEvent::TurnFailed(_)))
        .unwrap_or_else(|| panic!("{}", events.dump()));
    assert!(name_at < failure_at, "{}", events.dump());
    host.settle();
    assert_eq!(
        header_name(&host, thread).as_deref(),
        Some("Fake Agent"),
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn same_title_on_a_second_start_logs_nothing_new() {
    let dir = Dir::new(
        "agent_name_restart",
        &[(FAKE, Scenario::Bundled("auth-required-titled"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    for prompt in ["Read the lease.", "Read it again."] {
        let (turn, _) = host.prompt(thread, prompt);
        let ended = events.until_turn_ended(turn);
        assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    }
    host.settle();
    events.drain_ready();
    assert_eq!(events.count(named("Fake Agent")), 1, "{}", events.dump());
    host.shutdown();
}

#[test]
fn an_agent_without_a_title_logs_no_name() {
    let dir = Dir::new(
        "agent_name_untitled",
        &[(FAKE, Scenario::Bundled("auth-required"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the lease.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    host.settle();
    events.drain_ready();
    assert_eq!(
        events.count(|event| matches!(event, DomainEvent::AgentNamed(_))),
        0,
        "{}",
        events.dump()
    );
    assert_eq!(header_name(&host, thread), None, "{}", events.dump());
    host.shutdown();
}
