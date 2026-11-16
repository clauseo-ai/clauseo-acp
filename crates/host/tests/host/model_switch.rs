//! A model switch that would cost a Claude session its `auto` mode (audit row 4 of
//! b4l-sbxrefuse, `actor/turn/model_switch.rs`): claude-agent-acp 0.89.1 moves the session to
//! `acceptEdits` before it answers, so the host sets the old model again, then `auto`, logs a
//! notice naming both models, and keeps no pick in the matter's prefs. When the switch back
//! fails, the session stays in `acceptEdits` and the thread is told auto is unavailable.

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, ConfigOption, ConfigOptionKind, ConfigValue, DomainEvent,
    HostCommand, NoticeKind, StopReason, StreamId, ThreadId,
};

use crate::matters::summary;
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

fn id(text: &str) -> AgentChoiceId {
    AgentChoiceId::new(text).unwrap()
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

/// Every notice's kind, in order.
fn notices(events: &Events) -> Vec<NoticeKind> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => notice.kind.clone(),
            _ => None,
        })
        .collect()
}

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

/// The current value of `option` in the last logged options.
fn current(events: &Events, option: &str) -> Option<String> {
    let last: Vec<ConfigOption> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ConfigOptionsReplaced(replaced) => Some(replaced.options.clone()),
            _ => None,
        })
        .last()?;
    last.iter()
        .find(|offered| offered.id == option)
        .and_then(|offered| match &offered.kind {
            ConfigOptionKind::Select { current_value, .. } => Some(current_value.to_string()),
            _ => None,
        })
}

/// Plays `scenario` to the lawyer's pick of Haiku after a first turn; gives the host, the
/// thread's events once the pick is done, and the matter's prefs.
fn pick_haiku(test: &str, scenario: &'static str) -> (Running, ThreadId, Events, bool) {
    let dir = Dir::new(test, &[(CLAUDE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = turn(&host, thread, "Summarise clause 4.");
    let pick = HostCommand::SetConfigOption {
        thread,
        option_id: id("model"),
        value: ConfigValue::Select(id("haiku")),
    };
    assert_eq!(host.ok(pick), CommandOutcome::Done);
    events.until("the notice", |e| !notices(e).is_empty());
    let prefs = summary(&host, matter).agent_prefs.is_some();
    (host, thread, events, prefs)
}

#[test]
fn a_model_switch_that_costs_auto_is_put_back_with_a_notice() {
    let (host, thread, events, prefs) =
        pick_haiku("model_switch_refused", "claude-model-switch-refused");
    assert_eq!(
        notices(&events),
        [NoticeKind::ModelSwitchRefused {
            picked: "Haiku".to_owned(),
            kept: "Opus".to_owned(),
        }],
        "{}",
        events.dump()
    );
    // The start logs `auto`; then the move and the switch back.
    assert_eq!(
        modes(&events),
        ["auto", "acceptEdits", "auto"],
        "{}",
        events.dump()
    );
    assert_eq!(current(&events, "model").as_deref(), Some("opus"));
    assert_eq!(current(&events, "mode").as_deref(), Some("auto"));
    assert!(!prefs, "a refused pick is not kept in the matter's prefs");
    // The session goes on, on Opus in auto.
    turn(&host, thread, "And clause 5?");
    host.shutdown();
}

#[test]
fn a_switch_back_that_fails_leaves_accept_edits_and_says_auto_is_unavailable() {
    let (host, thread, events, prefs) =
        pick_haiku("model_switch_stuck", "claude-model-switch-stuck");
    assert_eq!(
        notices(&events),
        [NoticeKind::AutoModeUnavailable],
        "{}",
        events.dump()
    );
    assert_eq!(modes(&events), ["auto", "acceptEdits"], "{}", events.dump());
    assert_eq!(current(&events, "model").as_deref(), Some("haiku"));
    assert!(
        !prefs,
        "a pick that left auto is not kept in the matter's prefs"
    );
    turn(&host, thread, "And clause 5?");
    host.shutdown();
}
