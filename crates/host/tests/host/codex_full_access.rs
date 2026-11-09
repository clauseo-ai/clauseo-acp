//! An unjailed Codex (tier C) never reaches full access without the lawyer (R17 PR-D2c round 5).
//! A resume that reports it, and the agent's own move to it mid-session, are switched back to
//! the ask mode, `workspace-write` (`forced_mode::codex_start`, `forced_mode::agent_moved`); the
//! lawyer's own `SetMode` to it stands, even when the agent then reports it, but not when it
//! fails. A mode option moved there counts too, and a switch back that fails stops the agent
//! (round 6). A jailed Codex is unchanged: it always runs full access and its own moves are ignored (`codex_jailed.rs`).

use clauseo_protocol::{CommandOutcome, DomainEvent, NoticeKind, Seq, StopReason, StreamId};

use crate::codex::{CODEX, codex_dir, run_turn};
use crate::codex_jailed::{modes, set_mode};
use crate::observe::Events;
use crate::support::{Scenario, matter_with_thread};

/// Run 0 plays `codex-handshake`; run 1's resume answers `agent-full-access`, and the scenario
/// expects `session/set_mode workspace-write` before its prompt.
#[test]
fn an_unjailed_codex_resumed_in_full_access_is_switched_to_the_ask_mode() {
    let dir = codex_dir("codex_resumes_full_access", "codex-handshake");
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    let head = events.head();
    host.shutdown();

    dir.set_agents(&[(CODEX, Scenario::Local("codex-resumes-full-access"))]);
    let host = dir.start(1);
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_text(turn), "Asking again.", "{}", events.dump());
    assert_eq!(modes(&events), ["workspace-write"], "{}", events.dump());
    host.shutdown();
}

/// The agent's `current_mode_update` to `agent-full-access` is logged as it came, then the
/// host's `session/set_mode workspace-write` (the scenario expects it) is logged once answered.
#[test]
fn an_unjailed_codex_moving_itself_to_full_access_is_switched_back() {
    let dir = codex_dir("codex_moves_full_access", "codex-handshake");
    dir.set_agents(&[(CODEX, Scenario::Local("codex-moves-to-full-access"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(turn),
        "Back to asking.",
        "{}",
        events.dump()
    );
    events.until("the switch back logged", |e| modes(e).len() == 3);
    assert_eq!(
        modes(&events),
        ["workspace-write", "agent-full-access", "workspace-write"],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// The lawyer's `SetMode agent-full-access` reaches the agent, which reports the mode; no
/// `session/set_mode` goes back (the scenario's next expect is the second prompt).
#[test]
fn the_lawyers_pick_of_full_access_at_tier_c_stands() {
    let dir = codex_dir("codex_lawyer_full_access", "codex-handshake");
    dir.set_agents(&[(CODEX, Scenario::Local("codex-lawyer-full-access"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let first = run_turn(&host, &mut events, thread);
    assert_eq!(events.turn_text(first), "Asking.", "{}", events.dump());
    assert_eq!(
        host.ok(set_mode(thread, "agent-full-access")),
        CommandOutcome::Done
    );
    let second = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(second),
        "Full access as picked.",
        "{}",
        events.dump()
    );
    let logged = modes(&events);
    assert_eq!(logged.first().map(String::as_str), Some("workspace-write"));
    assert!(
        logged[1..].iter().all(|mode| mode == "agent-full-access"),
        "{}",
        events.dump()
    );
    assert!(logged.len() >= 2, "{}", events.dump());
    host.shutdown();
}

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

/// The host's switch back fails (the scenario answers it with an error): the turn is cancelled,
/// the agent stopped (its detach logged), one `ModeSetBackFailed` notice, and the mode it moved
/// to is the last logged. Run 1, after a restart, resumes in the ask mode
/// (`codex-resumes-unjailed`), so the next prompt runs asking.
#[test]
fn a_failed_switch_back_stops_the_agent_with_one_notice() {
    let dir = codex_dir("codex_set_back_fails", "codex-handshake");
    dir.set_agents(&[(CODEX, Scenario::Local("codex-set-back-fails"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    let ended = events.until_turn_ended(turn);
    assert_ne!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    events.until("the detach", |e| {
        e.events()
            .any(|event| matches!(event, DomainEvent::AgentSessionDetached(_)))
    });
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(
        notice_kinds(&events),
        [Some(NoticeKind::ModeSetBackFailed)],
        "{}",
        events.dump()
    );
    assert_eq!(
        modes(&events),
        ["workspace-write", "agent-full-access"],
        "{}",
        events.dump()
    );
    let head = events.head();
    host.shutdown();

    dir.set_agents(&[(CODEX, Scenario::Local("codex-resumes-unjailed"))]);
    let host = dir.start(1);
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(turn),
        "Asking as before.",
        "{}",
        events.dump()
    );
    assert_eq!(modes(&events), ["workspace-write"], "{}", events.dump());
    host.shutdown();
}

/// The agent reports `agent-full-access` while the lawyer's `SetMode` for it is in flight, and
/// the command then fails: the pick never held, so the host switches the session back (the
/// scenario expects `set_mode workspace-write` before the second prompt).
#[test]
fn a_report_of_a_failed_pick_is_switched_back() {
    let dir = codex_dir("codex_lawyer_pick_fails", "codex-handshake");
    dir.set_agents(&[(CODEX, Scenario::Local("codex-lawyer-pick-fails"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let first = run_turn(&host, &mut events, thread);
    assert_eq!(events.turn_text(first), "Asking.", "{}", events.dump());
    let error = host
        .dispatch(set_mode(thread, "agent-full-access"))
        .unwrap_err();
    assert!(error.code().as_str().starts_with("agent."), "{error}");
    events.until("the switch back logged", |e| {
        modes(e).last().map(String::as_str) == Some("workspace-write") && modes(e).len() == 3
    });
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

/// A `config_option_update` whose mode option is `agent-full-access` is switched back like a
/// `current_mode_update` (the scenario expects `set_mode workspace-write`).
#[test]
fn a_mode_option_moved_to_full_access_is_switched_back() {
    let dir = codex_dir("codex_option_full_access", "codex-handshake");
    dir.set_agents(&[(CODEX, Scenario::Local("codex-option-to-full-access"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(turn),
        "Back to asking.",
        "{}",
        events.dump()
    );
    events.until("the switch back logged", |e| modes(e).len() == 2);
    assert_eq!(
        modes(&events),
        ["workspace-write", "workspace-write"],
        "{}",
        events.dump()
    );
    host.shutdown();
}
