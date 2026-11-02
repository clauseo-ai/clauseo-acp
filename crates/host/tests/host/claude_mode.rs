//! A Claude agent (`claude-acp`) runs in Clauseo's mode, `default`, whatever mode it started in:
//! a user's `permissions.defaultMode` (`acceptEdits` here) on `session/new`, a resumed
//! transcript's `bypassPermissions` on `session/load`. The host sends `session/set_mode default`
//! before the first prompt, with `allowDangerouslySkipPermissions: false` in `_meta` (the
//! scenarios check both), and `SetMode` still works after the start. A failed switch sends no
//! prompt.

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, DomainEvent, HostCommand, StopReason, StreamId, ThreadId,
};

use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// Every `ModeChanged`'s mode, in order.
fn modes(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ModeChanged(changed) => Some(changed.mode_id.to_string()),
            _ => None,
        })
        .collect()
}

/// Runs one prompt on `thread` to its end and returns the thread's events so far.
fn turn(host: &Running, thread: ThreadId, text: &str) -> Events {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, text);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    events
}

#[test]
fn a_claude_session_switches_to_default_before_its_first_prompt_and_on_load() {
    let dir = Dir::new(
        "claude_mode",
        &[(CLAUDE, Scenario::Local("claude-mode-new"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = turn(&host, thread, "Summarise clause 4.");
    assert_eq!(modes(&events), ["default"], "{}", events.dump());

    // The mode switch the app's command uses still reaches the agent and is logged.
    assert_eq!(
        host.ok(HostCommand::SetMode {
            thread,
            mode_id: AgentChoiceId::new("plan").unwrap(),
        }),
        CommandOutcome::Done
    );
    events.until("ModeChanged plan", |e| modes(e).len() == 2);
    assert_eq!(modes(&events), ["default", "plan"]);
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Local("claude-mode-load"))]);
    let host = dir.start(1);
    let events = turn(&host, thread, "And clause 5?");
    assert_eq!(
        modes(&events),
        ["default", "plan", "default"],
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn a_failed_switch_to_default_sends_no_prompt() {
    let dir = Dir::new(
        "claude_mode_refused",
        &[(CLAUDE, Scenario::Local("claude-mode-refused"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Summarise clause 4.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    assert_eq!(events.turn_text(turn), "");
    assert!(modes(&events).is_empty(), "{}", events.dump());
    host.shutdown();
}
