//! A Claude conversation stops when Claude Code's user settings file in Clauseo's Claude home
//! stops being the launch's (`actor/turn/prompt/claude_settings.rs`): a change between prompts
//! fails the next prompt before it is sent, a change while a prompt runs stops it within a few
//! seconds, both with `agent.settings_changed` and the agent stopped. The next message launches
//! it again, and that launch writes the file afresh. The user-writable Windows policy's mid-turn
//! check is tested with its own seam (`profile_loads/policy.rs`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, StreamId, ThreadId, TurnId};

use crate::observe::Events;
use crate::support::{Dir, Scenario, matter_with_thread};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// What the launch writes.
const WRITTEN: &str = r#"{"syncClaudeAiSkills": false}"#;

/// A planted file: the same key, and a hook switch-off.
const PLANTED: &[u8] = br#"{"syncClaudeAiSkills": false, "disableAllHooks": true}"#;

/// Clauseo's Claude home's user settings file.
fn settings_file(dir: &Dir) -> PathBuf {
    dir.data()
        .join("agents")
        .join(CLAUDE)
        .join("home")
        .join("settings.json")
}

/// The file's JSON.
fn settings(dir: &Dir) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(settings_file(dir)).unwrap()).unwrap()
}

/// `turn`'s `TurnFailed` code.
fn failed(events: &Events, turn: TurnId) -> ErrorCode {
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no TurnFailed: {}", events.dump()))
}

/// Runs one prompt on `thread` to its end; returns the events and how it stopped.
fn turn(
    host: &crate::support::Running,
    thread: ThreadId,
    text: &str,
) -> (Events, TurnId, StopReason) {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, text);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    (events, turn, ended.stop)
}

#[test]
fn a_settings_file_changed_between_prompts_stops_the_next_and_the_relaunch_rewrites_it() {
    let dir = Dir::new(
        "claude_settings_between",
        &[(CLAUDE, Scenario::Local("claude-settings-changed"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let (events, _, stop) = turn(&host, thread, "Summarise clause 4.");
    assert_eq!(stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        settings(&dir),
        serde_json::from_str::<serde_json::Value>(WRITTEN).unwrap()
    );

    std::fs::write(settings_file(&dir), PLANTED).unwrap();
    let (events, refused, stop) = turn(&host, thread, "And clause 5?");
    assert_ne!(stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(failed(&events, refused), ErrorCode::AgentSettingsChanged);
    assert_eq!(events.turn_text(refused), "", "nothing reached the agent");

    let (events, _, stop) = turn(&host, thread, "And clause 6?");
    assert_eq!(stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        settings(&dir),
        serde_json::from_str::<serde_json::Value>(WRITTEN).unwrap()
    );
    host.shutdown();
}

#[test]
fn a_settings_file_changed_while_a_prompt_runs_stops_the_turn() {
    let dir = Dir::new(
        "claude_settings_watch",
        &[(CLAUDE, Scenario::Local("claude-settings-watch"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, "Summarise clause 4.");
    events.until("the agent working", |e| {
        e.turn_text(turn).contains("Working.")
    });
    let changed = Instant::now();
    std::fs::write(settings_file(&dir), PLANTED).unwrap();
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_ne!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(failed(&events, turn), ErrorCode::AgentSettingsChanged);
    // One poll (2 s), with room for a slow test machine.
    assert!(
        changed.elapsed() < Duration::from_secs(10),
        "{:?}",
        changed.elapsed()
    );
    host.shutdown();
}
