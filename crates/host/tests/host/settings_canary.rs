//! The `SessionStart` settings canary (`actor/turn/session/canary.rs`): with the switch on
//! (`HostConfig::with_settings_canary`) and the guard program set, every Claude
//! `session/new`, `session/load` and `session/resume` carries the canary's entry in its flag
//! settings, and the nonce it writes must be in its file when the call answers, before any
//! prompt. `fake_agent`'s `session_canary` step plays the canary (and fails, exit 3, when the
//! call's settings hold no entry); a scenario that leaves it out plays settings Claude Code
//! dropped. A call whose canary did not write its nonce is sent once more with a fresh one; a
//! second miss refuses the launch with `agent.settings_not_loaded` and sends no prompt (a prompt
//! after a refusing scenario's last step is a mismatch). A resume, the call that takes a forked
//! session back too, carries the full settings and is checked the same way.

use std::path::PathBuf;

use clauseo_host::HostConfig;
use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, StreamId, ThreadId};

use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// The guard program the canary's entry names; nothing runs it (`fake_agent` writes the nonce).
// unix-paths-ok: a planted value, never run or opened.
const PROGRAM: &str = "/opt/clauseo/clauseo-desktop";

/// Starts run `run` with the guard program and the canary on.
fn start_checked(dir: &Dir, run: u16) -> Running {
    dir.start_with(run, |config: HostConfig| {
        config
            .with_pre_write_guard(PathBuf::from(PROGRAM))
            .with_settings_canary(true)
    })
}

/// Runs one prompt on `thread` to its end and returns the answer's text.
fn turn(host: &Running, thread: ThreadId, text: &str) -> String {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, text);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    events.turn_text(turn)
}

/// Runs one prompt on `thread`, which must fail before any prompt with
/// `agent.settings_not_loaded`.
fn refused(host: &Running, thread: ThreadId, text: &str) {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, text);
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
    assert_eq!(
        failed,
        ErrorCode::AgentSettingsNotLoaded,
        "{}",
        events.dump()
    );
    assert_eq!(events.turn_text(turn), "");
}

/// New with its canary goes on at once; load and resume whose first canary never ran go on
/// after exactly one retry each (each scenario expects the call twice, then the prompt).
#[test]
fn the_canary_is_checked_on_new_load_and_resume_with_one_retry() {
    let dir = Dir::new(
        "settings_canary",
        &[(CLAUDE, Scenario::Local("claude-canary-new"))],
    );
    let host = start_checked(&dir, 0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    assert_eq!(turn(&host, thread, "Summarise clause 4."), "First answer.");
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Local("claude-canary-load-retry"))]);
    let host = start_checked(&dir, 1);
    assert_eq!(turn(&host, thread, "And clause 5?"), "Loaded answer.");
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Local("claude-canary-resume-retry"))]);
    let host = start_checked(&dir, 2);
    assert_eq!(turn(&host, thread, "And clause 6?"), "Resumed answer.");
    host.shutdown();
}

/// A `session/new` whose canary never ran is sent once more, and the turn runs on the second
/// session.
#[test]
fn a_missing_canary_on_new_retries_once_and_goes_on() {
    let dir = Dir::new(
        "settings_canary_retry",
        &[(CLAUDE, Scenario::Local("claude-canary-new-retry"))],
    );
    let host = start_checked(&dir, 0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    assert_eq!(
        turn(&host, thread, "Summarise clause 4."),
        "Second try answer."
    );
    host.shutdown();
}

/// A canary that never runs, or writes another value than its nonce, twice: refused after
/// exactly one retry, with no prompt.
#[test]
fn a_canary_missing_or_wrong_twice_refuses_the_launch() {
    for (test, scenario) in [
        ("settings_canary_missing", "claude-canary-missing"),
        ("settings_canary_wrong", "claude-canary-wrong"),
    ] {
        let dir = Dir::new(test, &[(CLAUDE, Scenario::Local(scenario))]);
        let host = start_checked(&dir, 0);
        let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
        refused(&host, thread, "Summarise clause 4.");
        host.shutdown();
    }
}

/// A resume whose settings were dropped (no nonce, twice) is refused like a new one.
#[test]
fn a_resume_without_its_canary_is_refused_after_one_retry() {
    let dir = Dir::new(
        "settings_canary_resume",
        &[(CLAUDE, Scenario::Local("claude-canary-new"))],
    );
    let host = start_checked(&dir, 0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    assert_eq!(turn(&host, thread, "Summarise clause 4."), "First answer.");
    host.shutdown();

    dir.set_agents(&[(CLAUDE, Scenario::Local("claude-canary-resume-missing"))]);
    let host = start_checked(&dir, 1);
    refused(&host, thread, "And clause 5?");
    host.shutdown();
}

/// With the switch on but no guard program, no canary can go: the session is not checked, as
/// no guard goes either (a launch resting on the guard is refused elsewhere, `guard_missing`).
#[test]
fn without_the_guard_program_no_canary_goes_and_the_session_runs() {
    let dir = Dir::new(
        "settings_canary_no_guard",
        &[(CLAUDE, Scenario::Local("claude-mode-new"))],
    );
    let host = dir.start_with(0, |config: HostConfig| config.with_settings_canary(true));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    assert_eq!(turn(&host, thread, "Summarise clause 4."), "First answer.");
    host.shutdown();
}
