//! An unjailed Codex's switch back, R17 PR-D2c round 7 (`forced_mode/unjailed.rs`).
//!
//! - One the agent never answers counts as failed after `SET_BACK_WAIT` on the host clock.
//! - A failed one kills the agent at once: the scenarios hold on stdin close, so on a clock
//!   nothing moves, only a kill with no graces ends them.
//! - A lawyer's mode `SetConfigOption` that fails, or that is answered with another value,
//!   leaves no pick, so the agent's later move to full access is switched back.
//! - A move reported while a switch back is in flight gets one more switch back, after which a
//!   third move stops the agent; the last mode logged is the agent's.

use std::time::Duration;

use clauseo_protocol::{
    AgentChoiceId, ConfigValue, DomainEvent, HostCommand, NoticeKind, StopReason, StreamId,
    ThreadId,
};

use crate::codex::{CODEX, codex_dir, run_turn};
use crate::codex_jailed::modes;
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// `SET_BACK_WAIT` (`forced_mode/unjailed.rs`).
const SET_BACK_WAIT: Duration = Duration::from_secs(10);

/// Every host notice's kind, in order.
fn notice_kinds(events: &Events) -> Vec<Option<NoticeKind>> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.kind.clone()),
            _ => None,
        })
        .collect()
}

fn detached(events: &Events) -> bool {
    events
        .events()
        .any(|event| matches!(event, DomainEvent::AgentSessionDetached(_)))
}

fn set_config_mode(thread: ThreadId, mode: &str) -> HostCommand {
    HostCommand::SetConfigOption {
        thread,
        option_id: AgentChoiceId::new("mode").unwrap(),
        value: ConfigValue::Select(AgentChoiceId::new(mode).unwrap()),
    }
}

/// A host on `scenario` with one Codex thread, its events subscribed.
fn start(test: &str, scenario: &'static str) -> (Dir, Running, ThreadId, Events) {
    let dir = codex_dir(test, "codex-handshake");
    dir.set_agents(&[(CODEX, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let events = host.events(StreamId::Thread(thread));
    (dir, host, thread, events)
}

/// The turn ended other than `EndTurn`, the agent detached with no clock moved past what
/// the caller moved, and one `ModeSetBackFailed` notice was logged.
fn stopped_once(host: &Running, events: &mut Events, turn: clauseo_protocol::TurnId) {
    let ended = events.until_turn_ended(turn);
    assert_ne!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    events.until("the detach", detached);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(
        notice_kinds(events),
        [Some(NoticeKind::ModeSetBackFailed)],
        "{}",
        events.dump()
    );
}

/// The agent never answers the switch back: once `SET_BACK_WAIT` passes on the host clock, it
/// is stopped as a failed one is. The clock moves by that wait at a time, from the move on,
/// until the notice comes (the switch back's timer may be armed just after a move).
#[test]
fn an_unanswered_switch_back_stops_the_agent_after_its_wait() {
    let (_dir, host, thread, mut events) =
        start("codex_set_back_unanswered", "codex-set-back-unanswered");
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    events.until("the move", |e| modes(e).len() == 2);
    for _ in 0..5 {
        host.clock.advance(SET_BACK_WAIT);
        host.settle();
        events.drain_ready();
        if !notice_kinds(&events).is_empty() {
            break;
        }
    }
    stopped_once(&host, &mut events, turn);
    assert_eq!(
        modes(&events),
        ["workspace-write", "agent-full-access"],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// The lawyer's mode `SetConfigOption` fails; the agent then moves to full access, which is
/// switched back (the scenario expects `set_mode workspace-write` before the second prompt).
#[test]
fn a_failed_mode_option_command_leaves_no_pick() {
    let (_dir, host, thread, mut events) =
        start("codex_config_pick_fails", "codex-config-pick-fails");
    run_turn(&host, &mut events, thread);
    let error = host
        .dispatch(set_config_mode(thread, "agent-full-access"))
        .unwrap_err();
    assert!(error.code().as_str().starts_with("agent."), "{error}");
    events.until("the switch back logged", |e| modes(e).len() == 3);
    let second = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(second),
        "Asking again.",
        "{}",
        events.dump()
    );
    assert_eq!(
        modes(&events),
        ["workspace-write", "agent-full-access", "workspace-write"],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// The lawyer's mode `SetConfigOption` succeeds with the mode option still at
/// `workspace-write`: no pick holds, so the agent's later move to full access is switched back.
#[test]
fn a_mode_option_answered_with_another_value_leaves_no_pick() {
    let (_dir, host, thread, mut events) =
        start("codex_config_pick_other", "codex-config-pick-other");
    run_turn(&host, &mut events, thread);
    host.ok(set_config_mode(thread, "agent-full-access"));
    events.until("the switch back logged", |e| modes(e).len() == 3);
    let second = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(second),
        "Asking again.",
        "{}",
        events.dump()
    );
    assert_eq!(
        modes(&events),
        ["workspace-write", "agent-full-access", "workspace-write"],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// A second move arrives while the first switch back is in flight: once it succeeds the host
/// switches back once more (the scenario expects it), and the last mode logged is the ask mode,
/// the agent's own.
#[test]
fn a_move_during_a_switch_back_gets_one_more() {
    let (_dir, host, thread, mut events) =
        start("codex_moves_during_set_back", "codex-moves-during-set-back");
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_text(turn), "Asking now.", "{}", events.dump());
    events.until("both switches back logged", |e| modes(e).len() == 5);
    assert_eq!(
        modes(&events),
        [
            "workspace-write",
            "agent-full-access",
            "agent-full-access",
            "workspace-write",
            "workspace-write"
        ],
        "{}",
        events.dump()
    );
    assert_eq!(notice_kinds(&events), [], "{}", events.dump());
    host.shutdown();
}

/// The agent moves again during the second switch back too: the host stops it, killed at once,
/// with one notice; the last mode logged is the agent's full access.
#[test]
fn a_move_during_the_second_switch_back_stops_the_agent() {
    let (_dir, host, thread, mut events) = start("codex_keeps_moving", "codex-keeps-moving");
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    stopped_once(&host, &mut events, turn);
    assert_eq!(
        modes(&events).last().map(String::as_str),
        Some("agent-full-access"),
        "{}",
        events.dump()
    );
    host.shutdown();
}
