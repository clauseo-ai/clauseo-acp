//! The expired sign-in (R20 brief; decision record 0048, P4b): a turn failing with
//! `agent.auth_required`, or with an error the sign-in-again test accepts (Codex's
//! `refresh_token_reused`), comes back held at the head of its queue, held as a refused
//! permission's `NotAllowed` holds it (#408), and `restart_agents` for that agent sends it once.
//! `stop_agents` puts a running turn back the same way and stops the agent. Across a host
//! restart the turn put back keeps its head place, ahead of the turns queued before it, and
//! every view orders the queue the same way. Removing the turn put back clears the sign-in
//! hold: the queue stays held the usual way, until the lawyer's "Send next".

use clauseo_protocol::{
    AgentId, CommandOutcome, ContentBlock, DeliveryRequest, DomainEvent, ErrorCode, HostCommand,
    MatterId, StopReason, StreamId, ThreadId, ThreadStatus, TurnId,
};

use crate::codex::{CODEX, codex_dir};
use crate::observe::Events;
use crate::permissions::{answer_from, until_first_request};
use crate::queue::{allow, held, send, settle, starts, until_queued, view_queue};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, text, wait};

/// The prompt every test sends first.
const ASKED: &str = "Look at the matter.";

/// `turn`'s `TurnFailed` code, when received.
fn failure(events: &Events, turn: TurnId) -> Option<ErrorCode> {
    events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
        _ => None,
    })
}

/// Waits for the turn queued behind `ended` (the one put back) and gives it with its text.
fn put_back(events: &mut Events, ended: TurnId) -> (TurnId, String) {
    let behind = |event: &DomainEvent| matches!(event, DomainEvent::TurnQueued(queued) if queued.behind_turn() == Some(ended));
    events.until("the turn put back", |events| {
        events.position(behind).is_some()
    });
    let turn = events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnQueued(queued) if queued.behind_turn() == Some(ended) => {
                Some(queued.turn())
            }
            _ => None,
        })
        .unwrap();
    let text = events
        .events()
        .find_map(|event| match event {
            DomainEvent::UserMessage(message) if message.turn == turn => {
                match message.content.as_slice() {
                    [ContentBlock::Text { text }] => Some(text.clone()),
                    _ => None,
                }
            }
            _ => None,
        })
        .unwrap_or_default();
    (turn, text)
}

/// How a thread's held queue shows in `ListThreads`: held, and the thread's status.
fn shown(host: &Running, matter: MatterId, thread: ThreadId) -> (bool, ThreadStatus) {
    (held(host, matter, thread), host.status(matter, thread))
}

/// The fake agent's id.
fn fake() -> AgentId {
    AgentId::new(FAKE).unwrap()
}

/// A first prompt that fails for the sign-in comes back held; `restart_agents` sends it once,
/// to a relaunched agent that loads the session, and a second call sends nothing.
fn held_then_resumed_once(dir: &Dir, host: &Running) {
    let (matter, thread, _) = matter_with_thread(dir, host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, ASKED);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    assert_eq!(
        failure(&events, first),
        Some(ErrorCode::AgentAuthRequired),
        "{}",
        events.dump()
    );
    let (again, text) = put_back(&mut events, first);
    assert_eq!(text, ASKED);
    host.settle();
    assert!(held(host, matter, thread), "{}", events.dump());
    assert_eq!(starts(&events, again), 0);

    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    assert_eq!(events.until_turn_ended(again).stop, StopReason::EndTurn);
    host.settle();
    assert!(!held(host, matter, thread));

    // A second restart resumes nothing: the agent stops and nothing starts.
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    host.settle();
    events.drain_ready();
    assert_eq!(starts(&events, again), 1, "{}", events.dump());
    let started = events.count(|event| matches!(event, DomainEvent::TurnStarted(_)));
    assert_eq!(started, 2, "{}", events.dump());
    events.assert_no_mismatch();
}

#[test]
fn a_turn_failing_auth_required_is_held_and_restart_agents_sends_it_once() {
    let dir = Dir::new(
        "sign_in_hold_auth",
        &[(FAKE, Scenario::Bundled("auth-required-then-load"))],
    );
    let host = dir.start(0);
    held_then_resumed_once(&dir, &host);
    host.shutdown();
}

#[test]
fn a_turn_failing_refresh_token_reused_is_held_as_auth_required() {
    let dir = Dir::new(
        "sign_in_hold_refresh",
        &[(FAKE, Scenario::Bundled("refresh-reused-then-load"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_sign_in_again(|text| text.contains("refresh_token_reused"))
    });
    held_then_resumed_once(&dir, &host);
    host.shutdown();
}

#[test]
fn stop_agents_puts_a_running_turn_back_held_and_restart_sends_it() {
    let dir = Dir::new(
        "sign_in_hold_stop",
        &[(FAKE, Scenario::Bundled("stop-mid-turn-then-load"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, ASKED);
    events.until("the turn starts", |events| starts(events, first) == 1);
    wait("stop_agents", host.handle.stop_agents(fake())).unwrap();
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    let (again, text) = put_back(&mut events, first);
    assert_eq!(text, ASKED);
    host.settle();
    assert!(held(&host, matter, thread), "{}", events.dump());
    assert_eq!(starts(&events, again), 0);

    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    assert_eq!(events.until_turn_ended(again).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn a_held_sign_in_shows_as_a_refused_permissions_hold_does() {
    // #408: a refused permission ends the turn NotAllowed and holds the queued prompt.
    let dir = codex_dir("sign_in_hold_like_not_allowed", "codex-deny-ends-turn");
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Write ok to a file outside the matter.");
    until_first_request(&mut events);
    host.prompt(thread, ASKED);
    answer_from(&host, &mut events, thread, 0, &["cancel"]);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::NotAllowed);
    host.settle();
    let refused = shown(&host, matter, thread);
    host.shutdown();

    let dir = Dir::new(
        "sign_in_hold_like_auth",
        &[(FAKE, Scenario::Bundled("auth-required-then-load"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, ASKED);
    events.until_turn_ended(first);
    put_back(&mut events, first);
    host.settle();
    assert_eq!(shown(&host, matter, thread), refused);
    assert!(refused.0);
    host.shutdown();
}

#[test]
fn a_turn_held_for_the_sign_in_runs_first_after_a_host_restart() {
    let dir = Dir::new(
        "sign_in_hold_restart",
        &[(FAKE, Scenario::Local("sign-in-hold-restart"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    // Two prompts queue behind the first while it waits on a permission, so they are in the
    // log before the turn put back for the sign-in.
    let (first, _) = host.prompt(thread, ASKED);
    until_first_request(&mut events);
    let (two, _) = send(&host, thread, text("Two"), DeliveryRequest::Queue);
    until_queued(&mut events, two);
    let (three, _) = send(&host, thread, text("Three"), DeliveryRequest::Queue);
    until_queued(&mut events, three);
    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    assert_eq!(failure(&events, first), Some(ErrorCode::AgentAuthRequired));
    // "Two" waits behind the first turn too: the one put back is the `TurnQueued` at the head.
    let head = |event: &DomainEvent| matches!(event, DomainEvent::TurnQueued(queued) if queued.is_at_head());
    events.until("the turn put back", |events| {
        events.position(head).is_some()
    });
    let again = events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnQueued(queued) if queued.is_at_head() => Some(queued.turn()),
            _ => None,
        })
        .unwrap();
    assert!(![first, two, three].contains(&again));
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "{}", events.dump());
    // The view shows the queue in the host's order: the turn put back first.
    assert_eq!(view_queue(&host, thread), [again, two, three]);
    drop(events);
    host.abort();

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "not held after the restart");
    assert_eq!(view_queue(&host, thread), [again, two, three]);
    // Signed in: the held turn goes first, then the two queued behind it, in order (the fake
    // agent's prompts check the texts' order too).
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    for turn in [again, two, three] {
        assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    }
    settle(&host, &mut events);
    let started = |turn: TurnId| {
        events
            .position(
                |event| matches!(event, DomainEvent::TurnStarted(started) if started.turn == turn),
            )
            .unwrap_or_else(|| panic!("{turn} never started: {}", events.dump()))
    };
    assert!(started(again) < started(two), "{}", events.dump());
    assert!(started(two) < started(three), "{}", events.dump());
    assert!(!held(&host, matter, thread));
    host.shutdown();
}

#[test]
fn removing_the_held_turn_clears_the_sign_in_hold_and_the_queue_waits_for_send_next() {
    let dir = Dir::new(
        "sign_in_hold_removed",
        &[(FAKE, Scenario::Local("sign-in-hold-removed"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, ASKED);
    until_first_request(&mut events);
    let (two, _) = send(&host, thread, text("Two"), DeliveryRequest::Queue);
    until_queued(&mut events, two);
    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    assert_eq!(failure(&events, first), Some(ErrorCode::AgentAuthRequired));
    // "Two" waits behind the first turn too: the one put back is the `TurnQueued` at the head.
    let head = |event: &DomainEvent| matches!(event, DomainEvent::TurnQueued(queued) if queued.is_at_head());
    events.until("the turn put back", |events| {
        events.position(head).is_some()
    });
    let again = events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnQueued(queued) if queued.is_at_head() => Some(queued.turn()),
            _ => None,
        })
        .unwrap();
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "{}", events.dump());

    // The lawyer removes the turn held for the sign-in.
    let remove = HostCommand::RemoveQueuedPrompt {
        thread,
        turn: again,
    };
    assert_eq!(host.ok(remove), CommandOutcome::Done);
    settle(&host, &mut events);
    assert!(
        held(&host, matter, thread),
        "the queue stays held: {}",
        events.dump()
    );

    // Signed in: nothing moves, as the queue waits for the lawyer, not for the sign-in.
    wait("restart_agents", host.handle.restart_agents(fake())).unwrap();
    settle(&host, &mut events);
    assert_eq!(starts(&events, two), 0, "{}", events.dump());
    assert!(held(&host, matter, thread), "{}", events.dump());

    // "Send next" sends it.
    assert_eq!(
        host.ok(HostCommand::ResumeQueue { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(two).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(starts(&events, again), 0, "{}", events.dump());
    assert!(!held(&host, matter, thread));
    events.assert_no_mismatch();
    host.shutdown();
}
