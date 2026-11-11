//! A Claude agent's steer accepted and not yet shown taken in when its turn ends `end_turn` is
//! checked on for 10 s on the host clock: Claude Code may write the line late, or run the
//! queued steer as a turn of its own once the turn has ended. Shown within that time, it is
//! `Injected` with the turn it was sent into; never shown, it is `NotSent` at the check 10 s
//! after the end, and never sent again on its own. The fake agent plays Claude (`claude-acp`) and
//! asks a permission after the steer, so the turn ends when the test allows it.

use std::path::PathBuf;
use std::time::Duration;

use clauseo_protocol::{
    DeliveryRequest, DomainEvent, MessageId, SteerOutcome, StopReason, ThreadId, TurnId,
};

use crate::observe::Events;
use crate::permissions::until_first_request;
use crate::queue::{DURABLE, allow, send, settle};
use crate::steer_confirm::{EVERY, STEER, claude, outcomes, write_taken_in};
use crate::support::{Dir, Running, text};

/// A Claude thread playing `scenario` whose steer was accepted while its first turn ran, that
/// turn then ended `end_turn` and its check at the end answered: its dir, host, thread, events,
/// matter folder, the turn and the steer's message.
fn ended_with_a_steer(
    test: &str,
    scenario: &'static str,
) -> (Dir, Running, ThreadId, Events, PathBuf, TurnId, MessageId) {
    let (dir, host, thread, mut events, folder) = claude(test, scenario);
    let (first, _) = host.prompt(thread, "Summarise the clause.");
    host.wait_armed(DURABLE);
    let (turn, message) = send(&host, thread, text(STEER), DeliveryRequest::Steer);
    assert_eq!(turn, first, "a joining steer answers with the running turn");
    until_first_request(&mut events);
    // Accepted: the check while the turn runs is armed. The end replaces it.
    host.wait_armed(EVERY);
    allow(&host, &mut events, thread);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    (dir, host, thread, events, folder, first, message)
}

/// Once the next check is armed (the one before answered), moves the host clock on to it.
fn tick(host: &Running) {
    host.wait_armed(EVERY);
    host.clock.advance(EVERY);
}

#[test]
fn a_steer_shown_5_s_after_the_turn_ends_is_injected() {
    let (dir, host, _thread, mut events, folder, first, message) = ended_with_a_steer(
        "a_steer_shown_5_s_after_the_turn_ends_is_injected",
        "claude-steer-unconfirmed",
    );
    // Checks at the end, 2 s and 4 s after it saw nothing.
    tick(&host);
    tick(&host);
    host.wait_armed(EVERY);
    settle(&host, &mut events);
    assert!(outcomes(&events, message).is_empty(), "{}", events.dump());
    // 5 s after the end Claude Code writes the line; the check at 6 s finds it.
    host.clock.advance(Duration::from_secs(1));
    write_taken_in(&dir, &folder);
    host.clock.advance(Duration::from_secs(1));
    events.until("SteerResolved { Injected }", |e| {
        !outcomes(e, message).is_empty()
    });
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
fn a_steer_the_transcript_never_shows_is_not_sent_10_s_after_the_turn_ends() {
    let (_dir, host, _thread, mut events, _folder, first, message) = ended_with_a_steer(
        "a_steer_the_transcript_never_shows_is_not_sent_10_s_after_the_turn_ends",
        "claude-steer-unconfirmed",
    );
    // Checks at 2, 4, 6 and 8 s after the end see nothing, and none of them is the last.
    for _ in 0..4 {
        tick(&host);
    }
    host.wait_armed(EVERY);
    settle(&host, &mut events);
    assert!(outcomes(&events, message).is_empty(), "{}", events.dump());
    // The check 10 s after the end is the last.
    host.clock.advance(EVERY);
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
fn a_steer_claude_runs_as_a_turn_of_its_own_is_injected() {
    let (dir, host, _thread, mut events, folder, first, message) = ended_with_a_steer(
        "a_steer_claude_runs_as_a_turn_of_its_own_is_injected",
        "claude-steer-own-turn",
    );
    // Claude Code runs the queued steer as a turn of its own, with no `session/prompt`.
    let own = |e: &Events| {
        e.events().find_map(|event| match event {
            DomainEvent::TurnStarted(started) if started.turn != first => Some(started.turn),
            _ => None,
        })
    };
    events.until("the agent-started turn", |e| own(e).is_some());
    let own = own(&events).unwrap();
    assert_eq!(events.until_turn_ended(own).stop, StopReason::EndTurn);
    settle(&host, &mut events);
    assert!(outcomes(&events, message).is_empty(), "{}", events.dump());
    // Its transcript shows the steer it took, and the next check finds it.
    write_taken_in(&dir, &folder);
    tick(&host);
    events.until("SteerResolved { Injected }", |e| {
        !outcomes(e, message).is_empty()
    });
    settle(&host, &mut events);
    assert_eq!(
        outcomes(&events, message),
        [SteerOutcome::Injected { turn: first }],
        "{}",
        events.dump()
    );
    host.shutdown();
}
