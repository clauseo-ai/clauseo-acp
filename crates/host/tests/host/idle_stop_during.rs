//! What comes while an idle stop is under way (R20b): the agent ignores its stdin's close, so
//! the stop's ladder holds it stopping until the host's clock reaches SIGTERM, 2 s on.
//!
//! - A prompt sent during the stop waits for the stop's end, then relaunches the agent: the
//!   thread reads `Starting` throughout, the resume cue (`resuming_since`) set from the
//!   detach, then `Running`; never `IdleStopped` or `Idle`.
//! - `stop_agents_for` during the stop makes it the hold's own: the agent detaches once,
//!   `Closed`, never `IdleStopped`, and a prompt then queues held for the switch.
//!
//! Unix only, as the ladder's SIGTERM step is (`actor/turn/tests/idle.rs`).

#![cfg(unix)]

use std::pin::pin;
use std::time::Duration;

use clauseo_host::StopFor;
use clauseo_protocol::{
    AgentId, DetachReason, DomainEvent, QueueHoldReason, StopReason, StreamId, ThreadStatus,
    Timestamp,
};
use clauseo_reducer::StreamState;
use futures::FutureExt as _;

use crate::idle_stop::{LIMIT, idle_stopped, start};
use crate::queue::hold_reason;
use crate::support::{FAKE, matter_with_thread, wait};

/// The ladder's wait after it closes the agent's stdin, before SIGTERM.
const STDIN_WAIT: Duration = Duration::from_secs(2);

/// Whether `event` is a detach.
fn detached(event: &DomainEvent) -> bool {
    matches!(event, DomainEvent::AgentSessionDetached(_))
}

#[test]
fn a_prompt_sent_during_the_stop_reads_starting_then_running_with_the_resume_cue() {
    let (dir, host) = start("idle_stop_prompt_during", "idle-stop-slow-exit-then-load");
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Remember the word heron.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    host.wait_armed(LIMIT);
    host.clock.advance(LIMIT);
    // The stop is under way: stdin closed, the agent still runs.
    host.wait_armed(STDIN_WAIT);
    let (second, _) = host.prompt(thread, "What was the word?");
    host.settle();
    assert_eq!(host.status(matter, thread), ThreadStatus::Starting);

    host.clock.advance(STDIN_WAIT);
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    assert!(
        events.turn_text(second).contains("heron"),
        "{}",
        events.dump()
    );
    assert_eq!(events.count(detached), 1, "{}", events.dump());
    assert_eq!(events.count(idle_stopped), 1, "{}", events.dump());
    events.assert_no_mismatch();

    // The header after each event, from the second prompt on, as the reducer folds the log.
    let mut state = StreamState::new(StreamId::Thread(thread));
    let mut seen: Vec<(&'static str, ThreadStatus, Option<Timestamp>)> = Vec::new();
    let mut from_second = false;
    let mut detach_ts = None;
    for envelope in &events.seen {
        let applied = clauseo_reducer::apply(&mut state, envelope).unwrap();
        assert!(applied.anomalies.is_empty(), "{:?}", applied.anomalies);
        let kind = match &envelope.event {
            DomainEvent::UserMessage(message) if message.turn == second => {
                from_second = true;
                "prompt"
            }
            DomainEvent::AgentSessionDetached(_) => {
                detach_ts = Some(envelope.ts);
                "detached"
            }
            DomainEvent::AgentSessionAttached(_) => "attached",
            DomainEvent::TurnStarted(started) if started.turn == second => "started",
            _ => "other",
        };
        if from_second && kind != "other" {
            let header = state.as_thread().unwrap().header().unwrap();
            seen.push((kind, header.status, header.resuming_since));
        }
    }
    let detach_ts = detach_ts.unwrap();
    assert_eq!(
        seen,
        vec![
            ("prompt", ThreadStatus::Starting, None),
            ("detached", ThreadStatus::Starting, Some(detach_ts)),
            ("attached", ThreadStatus::Starting, None),
            ("started", ThreadStatus::Running, None),
        ],
        "{}",
        events.dump()
    );
    // The relaunched agent ignores its stdin's close too: it is idle-stopped through the
    // ladder here, so the shutdown has no agent left to wait for on the stopped clock.
    host.wait_armed(LIMIT);
    host.clock.advance(LIMIT);
    host.wait_armed(STDIN_WAIT);
    host.clock.advance(STDIN_WAIT);
    events.until("the second idle stop", |events| {
        events.count(idle_stopped) == 2
    });
    host.shutdown();
}

#[test]
fn stop_agents_during_an_idle_stop_detaches_once_with_the_hold_s_reason() {
    let (dir, host) = start("idle_stop_hold_during", "idle-stop-slow-exit-then-load");
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Remember the word heron.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    host.wait_armed(LIMIT);
    host.clock.advance(LIMIT);
    host.wait_armed(STDIN_WAIT);

    // The login switches during the stop: the call goes out at its first poll, and answers
    // once the agent has ended.
    {
        let agent = AgentId::new(FAKE).unwrap();
        let mut stopping = pin!(host.handle.stop_agents_for(agent, StopFor::LoginSwitch));
        assert!(stopping.as_mut().now_or_never().is_none());
        host.settle();
        host.clock.advance(STDIN_WAIT);
        wait("stop_agents_for", stopping).unwrap();
    }
    events.until("the detach", |events| events.position(detached).is_some());

    // A prompt now queues held for the switch.
    host.prompt(thread, "What was the word?");
    host.settle();
    events.drain_ready();
    assert_eq!(
        hold_reason(&host, matter, thread),
        Some(QueueHoldReason::Switching)
    );
    assert_ne!(host.status(matter, thread), ThreadStatus::IdleStopped);
    let detaches: Vec<&DetachReason> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::AgentSessionDetached(detached) => Some(&detached.reason),
            _ => None,
        })
        .collect();
    assert_eq!(detaches, vec![&DetachReason::Closed], "{}", events.dump());
    assert_eq!(events.count(idle_stopped), 0, "{}", events.dump());
    host.shutdown();
}
