//! A Claude agent's steer counts as delivered only once Claude's transcript shows it taken in:
//! claude-agent-acp answers `injected` once the steer is in its own queue, and drops it unread
//! on a cancel. The fake agent plays Claude (`claude-acp`, with its home in the data folder) and
//! the test writes the transcript line Claude Code would. Shown taken in while the turn runs,
//! the steer is logged `Injected`; never shown, it is `NotSent` once the turn ends, whether it
//! ended `end_turn` or was stopped, and it is never sent again on its own; Send now on it cancels
//! the turn and sends it as the next one.

use std::path::Path;
use std::time::Duration;

use clauseo_protocol::{
    CommandOutcome, DeliveryRequest, DomainEvent, HostCommand, MessageId, SteerOutcome, StopReason,
    StreamId, ThreadId,
};

use crate::observe::Events;
use crate::permissions::until_first_request;
use crate::queue::{DURABLE, allow, send, settle, starts};
use crate::support::{Dir, Running, Scenario, matter_with_thread, text};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";
/// The steered text the scenarios expect.
const STEER: &str = "Focus on clause 4";
/// How often the host reads the transcript while the steer's turn runs.
const EVERY: Duration = Duration::from_secs(2);

/// The outcomes of `message`'s `SteerResolved`s, in order.
fn outcomes(events: &Events, message: MessageId) -> Vec<SteerOutcome> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::SteerResolved(resolved) if resolved.message == message => {
                Some(resolved.outcome.clone())
            }
            _ => None,
        })
        .collect()
}

/// A Claude thread playing `scenario`, its events, and the matter folder.
fn claude(
    test: &str,
    scenario: &'static str,
) -> (Dir, Running, ThreadId, Events, std::path::PathBuf) {
    let dir = Dir::new(test, &[(CLAUDE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let events = host.events(StreamId::Thread(thread));
    (dir, host, thread, events, folder)
}

/// Writes the transcript Claude Code keeps for the scenarios' session `sess-1`, with a `user`
/// line holding the steer. Stamped far ahead, so it is never earlier than the steer's send on
/// the test clock.
fn write_taken_in(dir: &Dir, folder: &Path) {
    let home = dir.data().join("agents").join(CLAUDE).join("home");
    let path = clauseo_host::claude_transcript(&home, folder, "sess-1");
    let lines = [
        serde_json::json!({"type": "queue-operation", "operation": "enqueue",
            "timestamp": "2999-01-01T00:00:00.000Z", "content": STEER}),
        serde_json::json!({"type": "user", "timestamp": "2999-01-01T00:00:00.000Z",
            "message": {"role": "user", "content": [{"type": "text", "text": STEER}]}}),
    ];
    let mut text = String::new();
    for line in &lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    crate::write::write(&path, text.as_bytes());
}

#[test]
fn a_steer_the_transcript_shows_taken_in_is_injected() {
    let (dir, host, thread, mut events, folder) = claude(
        "a_steer_the_transcript_shows_taken_in_is_injected",
        "claude-steer-confirmed",
    );
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);
    let (turn, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(turn, first, "a joining steer answers with the running turn");
    // Accepted after the answer `injected`: nothing is logged until the transcript shows it.
    until_first_request(&mut events);
    settle(&host, &mut events);
    assert!(outcomes(&events, message).is_empty(), "{}", events.dump());

    write_taken_in(&dir, &folder);
    host.wait_armed(EVERY);
    host.clock.advance(EVERY);
    events.until("SteerResolved { Injected }", |e| {
        !outcomes(e, message).is_empty()
    });
    assert!(events.turn_ended(first).is_none(), "{}", events.dump());

    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    // `settle` checks the agent saw no second prompt.
    settle(&host, &mut events);
    assert_eq!(
        outcomes(&events, message),
        [SteerOutcome::Injected { turn: first }],
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn a_steer_the_transcript_never_shows_is_not_sent_once_the_turn_ends() {
    let (_dir, host, thread, mut events, _) = claude(
        "a_steer_the_transcript_never_shows_is_not_sent_once_the_turn_ends",
        "claude-steer-unconfirmed",
    );
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);
    let (_, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    events.until("SteerResolved { NotSent }", |e| {
        !outcomes(e, message).is_empty()
    });
    // `settle` checks the agent saw no second prompt.
    settle(&host, &mut events);
    assert_eq!(
        outcomes(&events, message),
        [SteerOutcome::NotSent { turn: first }],
        "{}",
        events.dump()
    );
    let turns = events.count(|e| matches!(e, DomainEvent::TurnStarted(_)));
    assert_eq!(turns, 1, "{}", events.dump());
    host.shutdown();
}

#[test]
fn an_accepted_steer_whose_turn_is_stopped_is_not_sent() {
    let (_dir, host, thread, mut events, _) = claude(
        "an_accepted_steer_whose_turn_is_stopped_is_not_sent",
        "claude-steer-cancelled",
    );
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);
    let (_, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    // The text after the answer `injected` is durable once its batch's timer fires.
    host.wait_armed(DURABLE);
    host.clock.advance(DURABLE);
    events.until("the answer's second part", |e| {
        e.turn_text(first).ends_with("caps liability.")
    });

    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    events.until("SteerResolved { NotSent }", |e| {
        !outcomes(e, message).is_empty()
    });
    // `settle` checks the agent saw no second prompt.
    settle(&host, &mut events);
    assert_eq!(
        outcomes(&events, message),
        [SteerOutcome::NotSent { turn: first }],
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn an_accepted_steer_sent_now_cancels_the_turn_and_goes_next() {
    let (_dir, host, thread, mut events, _) = claude(
        "an_accepted_steer_sent_now_cancels_the_turn_and_goes_next",
        "claude-steer-send-now",
    );
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);
    let (_, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    // The text after the answer `injected` is durable once its batch's timer fires.
    host.wait_armed(DURABLE);
    host.clock.advance(DURABLE);
    events.until("the answer's second part", |e| {
        e.turn_text(first).ends_with("caps liability.")
    });

    // The row's Send now names the turn the steer was sent into.
    assert_eq!(
        host.ok(HostCommand::SendQueuedNow {
            thread,
            turn: first
        }),
        CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    events.until("SteerResolved { Requeued }", |e| {
        !outcomes(e, message).is_empty()
    });
    let [SteerOutcome::Requeued { turn: next }] = outcomes(&events, message)[..] else {
        panic!("not requeued: {}", events.dump());
    };
    assert_eq!(events.until_turn_ended(next).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert_eq!(starts(&events, next), 1, "{}", events.dump());
    assert_eq!(
        outcomes(&events, message),
        [SteerOutcome::Requeued { turn: next }],
        "{}",
        events.dump()
    );
    assert_eq!(events.turn_text(next), "Focusing on clause 4.");
    host.shutdown();
}
