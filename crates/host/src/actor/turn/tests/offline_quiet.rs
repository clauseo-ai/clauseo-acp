//! The quiet arm (`offline/quiet.rs`, D-050): a live turn of a Claude or Codex agent that sends
//! no model output for 30 s, with no tool call open and no ask, is probed; two missed probes in
//! a row end it `Failed { agent.offline }` with the answer so far kept; a probe that gets
//! through waits again; output, a tool call or an ask keeps it from ever probing; an agent's
//! note after a missed probe ends it at the next miss; the window's online leaves it. The probe
//! is a scripted answer: no test here touches the network.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use clauseo_protocol::{
    AgentChoiceId, AgentKind, CommandOutcome, ErrorCode, HostCommand, StopReason, ThreadId, TurnId,
};
use serde_json::{Value, json};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline::{answer, ended};
use super::rig::Rig;
use crate::actor::input::Input;
use crate::actor::turn::offline::probe::ReachProbe;
use crate::actor::turn::offline::quiet::QUIET_WAIT;
use crate::actor::turn::offline::{Arm, Watch};
use crate::actor::turn::{TurnInput, TurnPhase};
use crate::agent::AgentInput;

pub(super) const PROMPT: &str =
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#;

pub(super) const CANCELLED: &str = r#"{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}"#;

pub(super) const END_TURN: &str = r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#;

/// A permission request the lawyer answers Allow once.
pub(super) const PERMISSION: &str = r#"{"request":{"id":"perm-1","method":"session/request_permission","params":{"sessionId":"sess-1","toolCall":{"toolCallId":"t-1","title":"Run ls","kind":"execute","status":"pending"},"options":[{"optionId":"allow-once","name":"Allow once","kind":"allow_once"}]}}}
{"expect_response":{"id":"perm-1","result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}}"#;

/// A script line sending `update`.
pub(super) fn update(update: &Value) -> String {
    json!({"update": {"update": update}}).to_string()
}

/// An answer chunk.
pub(super) fn chunk(text: &str) -> String {
    update(
        &json!({"sessionUpdate": "agent_message_chunk", "messageId": "m-1",
        "content": {"type": "text", "text": text}}),
    )
}

/// A thought chunk.
pub(super) fn thought(text: &str) -> String {
    update(&json!({"sessionUpdate": "agent_thought_chunk",
        "content": {"type": "text", "text": text}}))
}

/// A tool call still running.
pub(super) fn tool() -> String {
    update(
        &json!({"sessionUpdate": "tool_call", "toolCallId": "t-9", "title": "Run the check",
        "kind": "execute", "status": "in_progress"}),
    )
}

/// codex-acp's `_meta` for a `connection` failure (`recordSessionFailure`).
fn failure(severity: &str, title: &str) -> Value {
    let actions = if severity == "error" {
        json!(["retry", "new_session"])
    } else {
        json!([])
    };
    json!({"jetbrains": {"air": {"version": 1, "sessionFailure": {
        "id": "turn-1:error", "revision": 1, "category": "connection", "severity": severity,
        "title": title, "actions": actions,
    }}}})
}

/// A Codex retry: a `connection` failure of severity `warning`.
fn retry(title: &str) -> String {
    update(&json!({"sessionUpdate": "session_info_update", "_meta": failure("warning", title)}))
}

/// A notice (Codex's warnings).
fn notice(text: &str) -> String {
    update(&json!({"sessionUpdate": "notice", "severity": "warning", "title": text}))
}

pub(super) fn script(lines: &[String]) -> String {
    let mut script = lines.join("\n");
    script.push('\n');
    script
}

/// A rig whose agent plays `lines` as one of `kind` (`None`: of no kind) for the probe and
/// the quiet arm, with the quiet watch on and `probe` counted; the thread, and the count.
pub(super) fn start(
    test: &str,
    kind: Option<AgentKind>,
    lines: &[String],
    probe: ReachProbe,
) -> (Rig, ThreadId, Arc<AtomicUsize>) {
    let mut rig = Rig::start(test, &script(lines));
    let (probe, count) = probe.counted();
    rig.actor.turn_path.probe = probe;
    rig.actor.turn_path.quiet_watch = true;
    rig.actor.turn_path.kind_for_tests = kind;
    let thread = rig.thread();
    (rig, thread, count)
}

/// Sends the prompt and runs until the turn is live, holding back the agent's events.
pub(super) fn prompting_held(rig: &mut Rig, thread: ThreadId) -> TurnId {
    let turn = send_prompt(rig, 10, thread);
    rig.run_holding("the prompt", event, |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.phase == TurnPhase::Prompting)
    });
    turn
}

/// Whether `input` is an event from the agent's connection.
fn event(_: &crate::actor::Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Agent {
            input: AgentInput::Event(_),
            ..
        }
    )
}

/// Whether `input` comes from the agent: an event, or a call's answer (the prompt's end).
pub(super) fn from_agent(actor: &crate::actor::Actor, input: &Input) -> bool {
    event(actor, input) || matches!(input, Input::Turn(TurnInput::Agent { .. }))
}

/// Runs until `n` inputs from the agent in all (events, the prompt's answer) are held.
pub(super) fn hold(rig: &mut Rig, n: usize) {
    let held = std::cell::Cell::new(rig.held.len());
    let count = |actor: &crate::actor::Actor, input: &Input| {
        let from = from_agent(actor, input);
        if from {
            held.set(held.get() + 1);
        }
        from
    };
    rig.run_holding("the agent's inputs", count, |_| held.get() >= n);
}

/// Handles the first held input and what follows it.
pub(super) fn release_one(rig: &mut Rig) {
    let input = rig.held.remove(0);
    rig.actor.on_input(input);
    rig.drain();
}

/// The watch on `thread`'s turn.
pub(super) fn watch(rig: &Rig, thread: ThreadId) -> Option<&Watch> {
    rig.actor.threads[&thread]
        .turn
        .current
        .as_ref()?
        .offline
        .as_ref()
}

/// Whether `thread`'s turn is live with no failure.
pub(super) fn going(rig: &Rig, thread: ThreadId) -> bool {
    let current = rig.actor.threads[&thread].turn.current.as_ref();
    current.is_some_and(|current| current.live() && current.failure.is_none())
}

/// Lets `how_long` pass: due timers fire, a probe they start answers, and what follows runs.
/// Agent inputs stay held.
pub(super) fn wait(rig: &mut Rig, thread: ThreadId, how_long: Duration) {
    rig.clock.advance(how_long);
    rig.drain();
    rig.run_holding("the probe's answer", from_agent, |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        !current
            .and_then(|current| current.offline.as_ref())
            .is_some_and(Watch::probing)
    });
    rig.drain();
}

/// Lets `steps` × `step` pass, one step at a time.
pub(super) fn wait_steps(rig: &mut Rig, thread: ThreadId, steps: u32, step: Duration) {
    for _ in 0..steps {
        wait(rig, thread, step);
    }
}

/// The probes so far.
pub(super) fn probes(count: &AtomicUsize) -> usize {
    count.load(Ordering::SeqCst)
}

/// Releases everything held and runs `thread`'s turn to its end.
pub(super) fn finish_turn(rig: &mut Rig, thread: ThreadId) {
    rig.release_held();
    until_ended(rig, thread);
}

#[test]
fn a_silent_claude_turn_ends_offline_after_two_missed_probes() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        CANCELLED.to_owned(),
    ];
    let (mut rig, thread, count) = start(
        "quiet-claude",
        Some(AgentKind::Claude),
        &lines,
        ReachProbe::fixed(false),
    );
    let turn = prompting_held(&mut rig, thread);
    // The answer's first words come at once.
    hold(&mut rig, 1);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    assert_eq!(probes(&count), 1);
    assert!(going(&rig, thread), "one miss waits again");
    assert_eq!(watch(&rig, thread).map(Watch::by), Some(Arm::Quiet));
    wait(&mut rig, thread, QUIET_WAIT);
    assert_eq!(probes(&count), 2);
    finish_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

#[test]
fn a_long_think_whose_probe_gets_through_waits_until_output() {
    let rest = " the notice period is thirty days.";
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        chunk(rest),
        END_TURN.to_owned(),
    ];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, count) = start("quiet-think", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 3);
    release_one(&mut rig);
    // 4.5 minutes of silence: one probe every 30 s, each through, and the turn goes on.
    wait_steps(&mut rig, thread, 9, QUIET_WAIT);
    assert_eq!(probes(&count), 9);
    assert!(going(&rig, thread));
    assert!(watch(&rig, thread).is_some_and(Watch::reached));
    release_one(&mut rig);
    assert!(watch(&rig, thread).is_none(), "output clears the watch");
    finish_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::EndTurn), None));
    assert_eq!(answer(&log, turn), format!("Clause five says{rest}"));
    rig.finish();
}

#[test]
fn a_turn_streaming_only_thoughts_is_never_probed() {
    let mut lines = vec![PROMPT.to_owned()];
    lines.extend((0..9).map(|n| thought(&format!("Weighing clause {n}. "))));
    lines.push(END_TURN.to_owned());
    let probe = ReachProbe::fixed(false);
    let (mut rig, thread, count) = start("quiet-thoughts", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 10);
    // A thought every 20 s for 3 minutes.
    for _ in 0..9 {
        release_one(&mut rig);
        wait(&mut rig, thread, Duration::from_secs(20));
        assert!(watch(&rig, thread).is_none());
    }
    assert_eq!(probes(&count), 0);
    assert!(going(&rig, thread));
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::EndTurn), None)
    );
    rig.finish();
}

#[test]
fn one_miss_then_output_clears_the_quiet_watch() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five"),
        chunk(" says"),
        END_TURN.to_owned(),
    ];
    let probe = ReachProbe::sequence(vec![false, true]);
    let (mut rig, thread, count) = start("quiet-one-miss", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 3);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    assert_eq!(probes(&count), 1);
    assert!(going(&rig, thread) && watch(&rig, thread).is_some());
    wait(&mut rig, thread, Duration::from_secs(10));
    release_one(&mut rig);
    assert!(watch(&rig, thread).is_none(), "output clears the watch");
    // 30 s after that output the next quiet probe gets through: the turn goes on.
    wait(&mut rig, thread, Duration::from_secs(20));
    assert_eq!(probes(&count), 1);
    wait(&mut rig, thread, Duration::from_secs(10));
    assert_eq!(probes(&count), 2);
    assert!(going(&rig, thread));
    finish_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::EndTurn), None));
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

/// Cancels `thread`'s turn as the lawyer would; it ends `Cancelled`.
pub(super) fn cancel(rig: &mut Rig, thread: ThreadId, turn: TurnId) {
    rig.dispatch(30, HostCommand::Cancel { thread }).unwrap();
    finish_turn(rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Cancelled), None)
    );
}

#[test]
fn an_open_tool_call_is_never_probed() {
    let lines = [PROMPT.to_owned(), tool(), CANCELLED.to_owned()];
    let probe = ReachProbe::fixed(false);
    let (mut rig, thread, count) = start("quiet-tool", Some(AgentKind::Codex), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    // 10:30 with the tool call open and no word from it (its cap is 11 min, `offline_backstop`).
    wait_steps(&mut rig, thread, 21, QUIET_WAIT);
    assert_eq!(probes(&count), 0);
    assert!(going(&rig, thread));
    cancel(&mut rig, thread, turn);
    rig.finish();
}

/// Answers `thread`'s pending permission Allow once, as command `n`.
pub(super) fn allow(rig: &mut Rig, n: u128, thread: ThreadId) {
    let agent = rig.actor.threads[&thread].turn.agent.as_deref().unwrap();
    let request = *agent.permissions.keys().next().unwrap();
    let answer = HostCommand::AnswerPermission {
        thread,
        request,
        option_id: AgentChoiceId::new("allow-once").unwrap(),
    };
    rig.dispatch(n, answer).unwrap();
}

#[test]
fn a_pending_permission_is_never_probed() {
    let lines = [
        PROMPT.to_owned(),
        PERMISSION.to_owned(),
        END_TURN.to_owned(),
    ];
    let probe = ReachProbe::fixed(false);
    let (mut rig, thread, count) = start("quiet-asking", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    wait_steps(&mut rig, thread, 40, QUIET_WAIT);
    assert_eq!(probes(&count), 0);
    assert!(going(&rig, thread));
    allow(&mut rig, 11, thread);
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::EndTurn), None)
    );
    rig.finish();
}

#[test]
fn a_codex_note_after_a_missed_quiet_probe_ends_at_the_next_miss() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        retry("Reconnecting... 2/5"),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(false);
    let (mut rig, thread, count) = start("quiet-then-note", Some(AgentKind::Codex), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 2);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    assert_eq!(probes(&count), 1);
    wait(&mut rig, thread, Duration::from_secs(10));
    release_one(&mut rig);
    assert_eq!(watch(&rig, thread).map(Watch::by), Some(Arm::Note));
    // The note keeps the quiet watch's schedule: its next probe is 30 s after the miss.
    wait(&mut rig, thread, Duration::from_secs(20));
    assert_eq!(probes(&count), 2);
    finish_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

/// Case 2 of D-050: every `CONNECT` answered 502. Codex's fallback note says a server answered
/// (not offline, dropped), then it retries without end: the retry's watch and a probe that
/// cannot get through end the turn offline 30 s after the first retry.
#[test]
fn codex_502_fallback_and_retries_end_offline() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        notice(
            "Falling back from WebSockets to HTTPS transport. unexpected status 502 Bad Gateway",
        ),
        retry("Reconnecting... 1/5"),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(false);
    let (mut rig, thread, count) = start("quiet-codex-502", Some(AgentKind::Codex), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 3);
    for _ in 0..3 {
        release_one(&mut rig);
    }
    assert_eq!(watch(&rig, thread).map(Watch::by), Some(Arm::Note));
    wait(&mut rig, thread, QUIET_WAIT);
    assert_eq!(probes(&count), 1);
    finish_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    let shown = format!(
        "{:?}",
        log.iter()
            .filter(|event| { !matches!(event, clauseo_protocol::DomainEvent::TurnFailed(_)) })
            .collect::<Vec<_>>()
    );
    assert!(
        !shown.contains("Falling back") && !shown.contains("502"),
        "{shown}"
    );
    rig.finish();
}

#[test]
fn an_agent_of_no_known_kind_gets_no_quiet_watch() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(false);
    let (mut rig, thread, count) = start("quiet-no-kind", None, &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    wait_steps(&mut rig, thread, 12, QUIET_WAIT);
    assert_eq!(probes(&count), 0);
    assert!(going(&rig, thread) && watch(&rig, thread).is_none());
    cancel(&mut rig, thread, turn);
    rig.finish();
}

#[test]
fn the_window_coming_online_keeps_a_quiet_watch() {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        CANCELLED.to_owned(),
    ];
    let probe = ReachProbe::fixed(false);
    let (mut rig, thread, count) = start("quiet-online", Some(AgentKind::Claude), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    assert_eq!(probes(&count), 1);
    let online = rig.dispatch(20, HostCommand::SetNetwork { online: true });
    assert_eq!(online.unwrap(), CommandOutcome::Done);
    assert_eq!(watch(&rig, thread).map(Watch::by), Some(Arm::Quiet));
    wait(&mut rig, thread, QUIET_WAIT);
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    rig.finish();
}

/// A quiet probe got through, so the computer is online: Codex giving up then is the service's
/// failure, never "You're offline".
#[test]
fn codex_giving_up_after_a_reached_quiet_probe_is_not_offline() {
    let refused = "stream disconnected before completion: Connection refused (os error 61)";
    let result = json!({"stopReason": "end_turn", "_meta": failure("error", refused)});
    let gave_up = json!({"respond": {"to": "p1", "result": result}}).to_string();
    let lines = [PROMPT.to_owned(), chunk("Clause five says"), gave_up];
    let probe = ReachProbe::fixed(true);
    let (mut rig, thread, _) = start("quiet-outage", Some(AgentKind::Codex), &lines, probe);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 2);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    assert!(watch(&rig, thread).is_some_and(Watch::reached));
    finish_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (
            Some(StopReason::Failed),
            Some(ErrorCode::AgentProtocolError)
        )
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}
