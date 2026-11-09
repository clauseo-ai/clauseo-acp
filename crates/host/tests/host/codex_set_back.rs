//! An unjailed Codex's switch back, R17 PR-D2c round 7 (`forced_mode/unjailed.rs`).
//!
//! - One the agent never answers counts as failed after `SET_BACK_WAIT` on the host clock.
//! - A failed one kills the agent at once: the scenarios hold on stdin close, so on a clock
//!   nothing moves, only a kill with no graces ends them.
//! - A lawyer's mode `SetConfigOption` that fails, or that is answered with another value,
//!   leaves no pick, so the agent's later move to full access is switched back.
//! - A move reported while a switch back is in flight gets one more switch back, after which a
//!   third move stops the agent; the last mode logged is the agent's.
//! - An agent that moves slowly, each move after the last switch back landed, is stopped on its
//!   third move (`MOVES_BEFORE_STOP`), counted for its session, not its turn.
//! - After a stop, prompts queued or sent now before it wait for the lawyer's Send next; with
//!   none, the lawyer's next message starts a new agent in the ask mode.

use std::time::Duration;

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, ConfigValue, DeliveryRequest, DomainEvent, HostCommand,
    NoticeKind, QueueHoldReason, StopReason, StreamId, ThreadId, ThreadStatus, TurnId,
};

use crate::codex::{CODEX, codex_dir, run_turn};
use crate::codex_jailed::modes;
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread, text};

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
fn stopped_once(host: &Running, events: &mut Events, turn: TurnId) {
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

/// Every `AgentSessionAttached`: one per launch of the thread's agent.
fn attached(events: &Events) -> usize {
    events
        .events()
        .filter(|event| matches!(event, DomainEvent::AgentSessionAttached(_)))
        .count()
}

/// The agent moves to full access once a turn, each move only after the last switch back has
/// landed (the next prompt goes once it is logged): the first two are switched back, the third
/// (`MOVES_BEFORE_STOP`) stops it, killed at once, with one notice; no prompt runs after it.
#[test]
fn a_third_move_stops_the_agent_however_slowly_it_moves() {
    let (_dir, host, thread, mut events) =
        start("codex_keeps_moving_slowly", "codex-keeps-moving-slowly");
    for n in 1..=2 {
        let turn = run_turn(&host, &mut events, thread);
        events.until("the switch back logged", |e| modes(e).len() == 1 + 2 * n);
        assert_eq!(
            events.turn_text(turn),
            format!("Answer {n}."),
            "{}",
            events.dump()
        );
        assert_eq!(notice_kinds(&events), [], "{}", events.dump());
    }
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    stopped_once(&host, &mut events, turn);
    assert_eq!(events.turn_text(turn), "", "{}", events.dump());
    assert_eq!(
        modes(&events),
        [
            "workspace-write",
            "agent-full-access",
            "workspace-write",
            "agent-full-access",
            "workspace-write",
            "agent-full-access"
        ],
        "{}",
        events.dump()
    );
    assert_eq!(attached(&events), 1, "{}", events.dump());
    host.shutdown();
}

/// The agent is stopped once `SET_BACK_WAIT` passes with its switch back unanswered.
fn stop_unanswered(host: &Running, events: &mut Events, turn: TurnId) {
    for _ in 0..5 {
        host.clock.advance(SET_BACK_WAIT);
        host.settle();
        events.drain_ready();
        if !notice_kinds(events).is_empty() {
            break;
        }
    }
    stopped_once(host, events, turn);
}

/// Whether `turn` started or ended.
fn ran(events: &Events, turn: TurnId) -> bool {
    events.events().any(|event| match event {
        DomainEvent::TurnStarted(started) => started.turn == turn,
        DomainEvent::TurnEnded(ended) => ended.turn == turn,
        _ => false,
    })
}

/// `thread` runs nothing and its queue shows held for the lawyer (`paused`, "Send next").
fn held_for_the_lawyer(host: &Running, thread: ThreadId) {
    let summary = host
        .matters()
        .into_iter()
        .flat_map(|matter| host.threads(matter.matter))
        .find(|summary| summary.thread == thread)
        .expect("the thread is listed");
    assert_eq!(summary.status, ThreadStatus::Idle, "{summary:?}");
    assert!(summary.queue_held, "{summary:?}");
    assert_eq!(
        summary.queue_held_reason,
        Some(QueueHoldReason::Paused),
        "{summary:?}"
    );
}

/// A prompt queued while the agent runs, then the stop (`stop_unasked`): the queued prompt stays
/// queued, held for the lawyer (no `TurnStarted`, no `TurnEnded`), and no agent launches by
/// itself, however long the host waits. The lawyer's next plain message un-holds the queue
/// (`After::Appended`): one new launch runs the queued prompt first, in the ask mode, the new
/// message behind it.
#[test]
fn a_prompt_queued_before_the_stop_waits_for_the_lawyer() {
    let (_dir, host, thread, mut events) = start(
        "codex_set_back_queued",
        "codex-set-back-unanswered-then-again",
    );
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    events.until("the move", |e| modes(e).len() == 2);
    let (queued, _) = host.prompt(thread, "And then this.");
    events.until("the queued turn", |e| {
        e.events()
            .any(|event| matches!(event, DomainEvent::TurnQueued(_)))
    });
    stop_unanswered(&host, &mut events, turn);
    for _ in 0..3 {
        host.clock.advance(SET_BACK_WAIT);
        host.settle();
        events.drain_ready();
    }
    held_for_the_lawyer(&host, thread);
    assert!(!ran(&events, queued), "{}", events.dump());
    assert_eq!(attached(&events), 1, "{}", events.dump());
    let (next, _) = host.prompt(thread, "Start again.");
    let ended = events.until_turn_ended(queued);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        events.turn_text(queued),
        "Asking again.",
        "{}",
        events.dump()
    );
    // Up to the queued turn's end: one new launch, in the ask mode, and the new message not
    // started (the scenario's agent exits after one turn, so it may take another).
    let upto: Vec<&DomainEvent> = events
        .events()
        .take_while(|event| !matches!(event, DomainEvent::TurnEnded(e) if e.turn == queued))
        .collect();
    let launches = upto
        .iter()
        .filter(|event| matches!(event, DomainEvent::AgentSessionAttached(_)))
        .count();
    assert_eq!(launches, 2, "{}", events.dump());
    let started = |turn: TurnId| {
        upto.iter()
            .any(|event| matches!(event, DomainEvent::TurnStarted(s) if s.turn == turn))
    };
    assert!(started(queued) && !started(next), "{}", events.dump());
    assert_eq!(
        modes(&events)[..3],
        ["workspace-write", "agent-full-access", "workspace-write"],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// A prompt the lawyer sent now while the agent ran, waiting at the head for the cancelled turn
/// to end, then the stop: it waits for the lawyer too, though a head sent now starts whatever
/// the stop of the turn before it; Send next launches the agent for it.
#[test]
fn a_prompt_sent_now_before_the_stop_waits_for_the_lawyer() {
    let (_dir, host, thread, mut events) = start(
        "codex_set_back_sent_now",
        "codex-set-back-unanswered-then-again",
    );
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    events.until("the move", |e| modes(e).len() == 2);
    // Short of the switch back's wait, then the send now, whose cancel's own wait
    // (`CANCEL_WAIT`, 5 s) is still running when the switch back's ends.
    host.clock.advance(Duration::from_secs(6));
    host.settle();
    let sent = host.ok(HostCommand::SendPrompt {
        thread,
        content: text("And then this."),
        delivery: DeliveryRequest::SendNow,
    });
    let CommandOutcome::PromptAccepted { turn: queued, .. } = sent else {
        panic!("SendPrompt answered {sent:?}");
    };
    events.until("the turn sent now", |e| {
        e.events()
            .any(|event| matches!(event, DomainEvent::TurnQueued(_)))
    });
    host.clock.advance(Duration::from_secs(4));
    host.settle();
    stopped_once(&host, &mut events, turn);
    host.settle();
    events.drain_ready();
    held_for_the_lawyer(&host, thread);
    assert!(!ran(&events, queued), "{}", events.dump());
    assert_eq!(attached(&events), 1, "{}", events.dump());
    host.ok(HostCommand::ResumeQueue { thread });
    events.until_turn_ended(queued);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(attached(&events), 2, "{}", events.dump());
    assert_eq!(
        events.turn_text(queued),
        "Asking again.",
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// The stop with nothing queued: the lawyer's next plain message starts one new agent, whose
/// turn runs in the ask mode, as TN-11's words say.
#[test]
fn the_next_message_after_the_stop_starts_a_new_agent() {
    let (_dir, host, thread, mut events) = start(
        "codex_set_back_next_message",
        "codex-set-back-unanswered-then-again",
    );
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    events.until("the move", |e| modes(e).len() == 2);
    stop_unanswered(&host, &mut events, turn);
    assert_eq!(attached(&events), 1, "{}", events.dump());
    let (next, _) = host.prompt(thread, "Start again.");
    let ended = events.until_turn_ended(next);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(attached(&events), 2, "{}", events.dump());
    assert_eq!(events.turn_text(next), "Asking again.", "{}", events.dump());
    assert_eq!(
        modes(&events),
        ["workspace-write", "agent-full-access", "workspace-write"],
        "{}",
        events.dump()
    );
    host.shutdown();
}
