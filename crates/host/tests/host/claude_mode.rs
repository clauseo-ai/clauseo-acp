//! A Claude agent (`claude-acp`) outside forced ask runs in Clauseo's mode, `auto`, whatever
//! mode it started in: claude-agent-acp's own `default` on `session/new` and `session/resume`, a
//! resumed transcript's `bypassPermissions` on `session/load`. The host sends `session/set_mode
//! auto` before the first prompt, with `allowDangerouslySkipPermissions: false` in `_meta` (the
//! scenarios check both), and `SetMode` still works after the start. When auto is not available
//! (not offered, the switch refused, or the agent moving off it at once) the session runs in
//! `acceptEdits`, with one notice. An answer with no modes, one offering neither mode, and a
//! refused fallback each fail the launch before any prompt. A matter whose folder Claude's
//! sandbox cannot name runs in `acceptEdits` with no notice. Forced ask starts in `default`
//! instead (`forced_ask.rs`).

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, DomainEvent, ErrorCode, HostCommand, NoticeKind, StopReason,
    StreamId, ThreadId,
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

/// How many `AutoModeUnavailable` notices the thread has.
fn auto_notices(events: &Events) -> usize {
    events
        .events()
        .filter(|event| {
            matches!(event, DomainEvent::Notice(notice)
                if notice.kind == Some(NoticeKind::AutoModeUnavailable))
        })
        .count()
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
fn a_claude_session_switches_to_auto_before_its_first_prompt_on_load_and_on_resume() {
    let dir = Dir::new(
        "claude_mode",
        &[(CLAUDE, Scenario::Local("claude-mode-new"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = turn(&host, thread, "Summarise clause 4.");
    assert_eq!(modes(&events), ["auto"], "{}", events.dump());

    // The mode switch the app's command uses still reaches the agent and is logged.
    assert_eq!(
        host.ok(HostCommand::SetMode {
            thread,
            mode_id: AgentChoiceId::new("plan").unwrap(),
        }),
        CommandOutcome::Done
    );
    events.until("ModeChanged plan", |e| modes(e).len() == 2);
    assert_eq!(modes(&events), ["auto", "plan"]);
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Local("claude-mode-load"))]);
    let host = dir.start(1);
    let events = turn(&host, thread, "And clause 5?");
    assert_eq!(
        modes(&events),
        ["auto", "plan", "auto"],
        "{}",
        events.dump()
    );
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Local("claude-mode-resume"))]);
    let host = dir.start(2);
    let events = turn(&host, thread, "And clause 6?");
    assert_eq!(
        modes(&events),
        ["auto", "plan", "auto", "auto"],
        "{}",
        events.dump()
    );
    assert_eq!(auto_notices(&events), 0, "{}", events.dump());
    host.shutdown();
}

/// Plays `scenario`, whose agent cannot run `auto`, through two turns: the session runs in
/// `acceptEdits` with exactly one notice, and the modes logged are `expected`.
fn falls_back(test: &str, scenario: &'static str, expected: &[&str]) {
    let dir = Dir::new(test, &[(CLAUDE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    turn(&host, thread, "Summarise clause 4.");
    let mut events = turn(&host, thread, "And clause 5?");
    events.until("modes", |e| modes(e).len() == expected.len());
    assert_eq!(modes(&events), expected, "{}", events.dump());
    assert_eq!(
        modes(&events).last().map(String::as_str),
        Some("acceptEdits")
    );
    assert_eq!(auto_notices(&events), 1, "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_session_offering_no_auto_runs_in_accept_edits_with_one_notice() {
    falls_back(
        "claude_mode_no_auto",
        "claude-mode-no-auto",
        &["acceptEdits", "default", "acceptEdits"],
    );
}

#[test]
fn a_refused_switch_to_auto_falls_back_to_accept_edits_with_one_notice() {
    falls_back(
        "claude_mode_auto_refused",
        "claude-mode-auto-refused",
        &["acceptEdits"],
    );
}

#[test]
fn an_agent_that_moves_off_auto_at_once_runs_in_accept_edits_with_one_notice() {
    falls_back(
        "claude_mode_auto_moved",
        "claude-mode-auto-moved",
        &["auto", "acceptEdits"],
    );
}

/// Plays `scenario` to a turn that must fail before any prompt with `code`, with no mode
/// logged.
fn fails_before_prompt(test: &str, scenario: &'static str, code: ErrorCode) {
    let dir = Dir::new(test, &[(CLAUDE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Summarise clause 4.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    let failed = events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no TurnFailed: {}", events.dump()));
    assert_eq!(failed, code, "{}", events.dump());
    assert_eq!(events.turn_text(turn), "");
    assert!(modes(&events).is_empty(), "{}", events.dump());
    host.shutdown();
}

#[test]
fn a_failed_switch_to_auto_and_to_accept_edits_sends_no_prompt() {
    fails_before_prompt(
        "claude_mode_refused",
        "claude-mode-refused",
        ErrorCode::AgentProtocolError,
    );
}

#[test]
fn a_session_with_no_modes_fails_before_any_prompt() {
    fails_before_prompt(
        "claude_mode_none",
        "claude-mode-none",
        ErrorCode::AgentProtocolError,
    );
}

#[test]
fn a_session_offering_neither_auto_nor_accept_edits_fails_before_any_prompt() {
    fails_before_prompt(
        "claude_mode_no_accept_edits",
        "claude-mode-no-accept-edits",
        ErrorCode::AgentProtocolError,
    );
}

/// Outside forced ask, a session the agent moves to `bypassPermissions` or `dontAsk` is switched
/// back to `auto`, each logged as it came and then as switched back; one it moves to
/// `acceptEdits` stays there.
#[test]
fn a_session_that_moves_to_bypass_or_dont_ask_goes_back_to_auto() {
    let dir = Dir::new(
        "claude_mode_reset",
        &[(CLAUDE, Scenario::Local("claude-mode-reset"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = turn(&host, thread, "Summarise clause 4.");
    events.until("ModeChanged auto back", |e| modes(e).len() == 3);
    assert_eq!(
        modes(&events),
        ["auto", "bypassPermissions", "auto"],
        "{}",
        events.dump()
    );
    let mut events = turn(&host, thread, "And clause 5?");
    events.until("ModeChanged auto back", |e| modes(e).len() == 5);
    assert_eq!(
        modes(&events)[3..],
        ["dontAsk", "auto"],
        "{}",
        events.dump()
    );
    let mut events = turn(&host, thread, "And clause 6?");
    events.until("ModeChanged acceptEdits", |e| modes(e).len() == 6);
    assert_eq!(modes(&events)[5..], ["acceptEdits"], "{}", events.dump());
    assert_eq!(auto_notices(&events), 0, "{}", events.dump());
    host.shutdown();
}

/// A matter folder whose name holds a `*` cannot be named in Claude's sandbox, so the session
/// gets none and runs in `acceptEdits` though `auto` is offered, with no notice (the account
/// has auto); a move to `bypassPermissions` goes back to `acceptEdits`, not `auto`.
#[cfg(unix)]
#[test]
fn a_matter_the_sandbox_cannot_name_runs_in_accept_edits() {
    let dir = Dir::new(
        "claude_mode_no_sandbox",
        &[(CLAUDE, Scenario::Local("claude-mode-no-sandbox"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "A*", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = turn(&host, thread, "Summarise clause 4.");
    events.until("ModeChanged acceptEdits back", |e| modes(e).len() == 3);
    assert_eq!(
        modes(&events),
        ["acceptEdits", "bypassPermissions", "acceptEdits"],
        "{}",
        events.dump()
    );
    assert_eq!(auto_notices(&events), 0, "{}", events.dump());
    host.shutdown();
}
