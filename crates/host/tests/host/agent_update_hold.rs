//! An agent's own update (`StopFor::AgentInstall`): desktop holds the agent while its install is
//! redone after an app update, since its entry still names the install that went. A message
//! sent to it meanwhile is accepted and waits, its queue showing `update`, and nothing of that
//! agent launches; a conversation on another agent runs on; `restart_agents`, once the new
//! install's entry is written, sends the waiting message once.

use clauseo_host::StopFor;
use clauseo_protocol::{AgentId, DomainEvent, QueueHoldReason, StopReason, StreamId, UnsentCause};

use crate::queue::{held, hold_reason, settle, starts};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread, trace_to_stderr, wait};

/// The other agent, whose install is ready.
const OTHER: &str = "other-agent";
/// The message sent during the update.
const ASKED: &str = "Look at the matter.";

#[test]
fn an_agent_s_update_holds_only_that_agent_shows_update_and_restart_sends_the_message_once() {
    trace_to_stderr();
    let dir = Dir::new(
        "agent_update_hold",
        &[
            (FAKE, Scenario::Bundled("prompt-stream")),
            (OTHER, Scenario::Bundled("prompt-stream")),
        ],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let other = host.create_thread(matter, OTHER);
    let agent = AgentId::new(FAKE).unwrap();
    host.wait_dumping(
        "stop_agents_for",
        host.handle
            .stop_agents_for(agent.clone(), StopFor::AgentInstall),
    )
    .unwrap();

    // A message during the download: accepted and queued, waiting with `update`.
    let mut events = host.events(StreamId::Thread(thread));
    let (waiting, _) = host.prompt(thread, ASKED);
    events.until("the message queued", |events| {
        events
            .position(|event| matches!(event, DomainEvent::TurnQueued(q) if q.turn() == waiting))
            .is_some()
    });
    settle(&host, &mut events);
    assert!(held(&host, matter, thread), "{}", events.dump());
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Update),
        "{}",
        events.dump()
    );
    assert_eq!(starts(&events, waiting), 0, "{}", events.dump());

    // The other agent's conversation runs on meanwhile.
    let mut others = host.events(StreamId::Thread(other));
    let (theirs, _) = host.prompt(other, ASKED);
    assert_eq!(others.until_turn_ended(theirs).stop, StopReason::EndTurn);
    settle(&host, &mut others);
    assert_eq!(hold_reason(&host, matter, other), None);
    settle(&host, &mut events);
    assert_eq!(
        starts(&events, waiting),
        0,
        "still waiting: {}",
        events.dump()
    );

    // Ready, the new entry written: the message runs, once.
    wait("restart_agents", host.handle.restart_agents(agent.clone())).unwrap();
    assert_eq!(events.until_turn_ended(waiting).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(hold_reason(&host, matter, thread), None);
    wait("restart_agents", host.handle.restart_agents(agent)).unwrap();
    settle(&host, &mut events);
    events.drain_ready();
    assert_eq!(starts(&events, waiting), 1, "{}", events.dump());
    events.assert_no_mismatch();
    host.shutdown();
}

/// A message whose launch the hold meets (sent in the moment before desktop's hold, its agent not
/// answering yet): its turn ends `Cancelled` with #1314's cause `update`, not `sign_in`.
#[test]
fn a_launch_the_update_s_hold_stops_ends_with_the_update_cause() {
    trace_to_stderr();
    let dir = Dir::new("agent_update_cause", &[(FAKE, Scenario::Bundled("idle"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, ASKED);
    host.settle();
    let agent = AgentId::new(FAKE).unwrap();
    host.wait_dumping(
        "stop_agents_for",
        host.handle.stop_agents_for(agent, StopFor::AgentInstall),
    )
    .unwrap();
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::Cancelled, "{}", events.dump());
    assert_eq!(ended.cause, Some(UnsentCause::Update), "{}", events.dump());
    host.shutdown();
}
