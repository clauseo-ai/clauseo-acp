//! Going offline mid-answer (`offline.rs`, D-050): Codex's note about its connection never
//! reaches the log, and Claude's line is held back until the turn's end shows what it was; a
//! note that names a connection failure, then silence, ends the turn through the Cancel path as
//! `Failed { agent.offline }` with the answer so far kept; work or a permission request after
//! the note clears it; a prompt that fails with a connection-class error ends `agent.offline`
//! too, and a server error or a sign-in keeps its own code, watch or not. A turn that ends any
//! other way (a cancel the agent never answers) puts Claude's held line back.

use clauseo_protocol::{
    AgentChoiceId, DomainEvent, ErrorCode, HostCommand, StopReason, ThreadId, TurnId,
};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::rig::{Rig, kind};
use crate::actor::input::Input;
use crate::actor::turn::TurnInput;
use crate::actor::turn::offline::OFFLINE_WAIT;
use crate::actor::turn::offline::probe::ReachProbe;
use crate::actor::turn::{CANCEL_WAIT, TurnPhase};

/// Codex's warning, as codex-acp forwards it, from stage C's sweep (macOS).
const NOTE: &str = "Falling back from WebSockets to HTTPS transport. stream disconnected before \
                    completion: Connection refused (os error 61)";

/// Claude Code's line for a refused connection, as claude-agent-sdk 0.3.293 writes it.
const CLAUDE: &str = "API Error: Unable to connect to API (ECONNREFUSED)";

/// A partial answer, Codex's note, then nothing until the host cancels.
fn quiet() -> String {
    format!(
        r#"{{"expect":{{"method":"session/prompt","as":"p1","params":{{"sessionId":"sess-1"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{{"type":"text","text":"Clause five says"}}}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{NOTE}"}}}}}}}}
{{"expect":{{"method":"session/cancel","params":{{"sessionId":"sess-1"}}}}}}
{{"respond":{{"to":"p1","result":{{"stopReason":"cancelled"}}}}}}
"#
    )
}

/// A partial answer, the note, then more answer over HTTPS and a normal end.
fn recovered() -> String {
    format!(
        r#"{{"expect":{{"method":"session/prompt","as":"p1","params":{{"sessionId":"sess-1"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{{"type":"text","text":"Clause five says"}}}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{NOTE}"}}}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{{"type":"text","text":" the notice period is thirty days."}}}}}}}}
{{"respond":{{"to":"p1","result":{{"stopReason":"end_turn"}}}}}}
"#
    )
}

/// A prompt codex-acp fails the way it fails Codex's own error: "Internal error", with Codex's
/// message in `data.message`.
fn failing(message: &str) -> String {
    format!(
        r#"{{"expect":{{"method":"session/prompt","as":"p1","params":{{"sessionId":"sess-1"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{{"type":"text","text":"Clause five says"}}}}}}}}
{{"respond":{{"to":"p1","error":{{"code":-32603,"message":"Internal error","data":{{"message":"{message}","codex_error_info":"other"}}}}}}}}
"#
    )
}

/// A partial answer, Codex's note, then the prompt fails with `message` in `data.message`.
fn failing_after_note(message: &str) -> String {
    format!(
        r#"{{"expect":{{"method":"session/prompt","as":"p1","params":{{"sessionId":"sess-1"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{{"type":"text","text":"Clause five says"}}}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{NOTE}"}}}}}}}}
{{"respond":{{"to":"p1","error":{{"code":-32603,"message":"Internal error","data":{{"message":"{message}","codex_error_info":"other"}}}}}}}}
"#
    )
}

/// A partial answer, Claude's line, then `rest` (more steps, ending the prompt).
fn claude(rest: &str) -> String {
    format!(
        r#"{{"expect":{{"method":"session/prompt","as":"p1","params":{{"sessionId":"sess-1"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{{"type":"text","text":"Clause five says"}}}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{CLAUDE}"}}}}}}}}
{rest}
"#
    )
}

/// The note, then a permission request the lawyer answers after the wait.
fn asking() -> String {
    format!(
        r#"{{"expect":{{"method":"session/prompt","as":"p1","params":{{"sessionId":"sess-1"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{{"type":"text","text":"Clause five says"}}}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{NOTE}"}}}}}}}}
{{"request":{{"id":"perm-1","method":"session/request_permission","params":{{"sessionId":"sess-1","toolCall":{{"toolCallId":"t-1","title":"Run ls","kind":"execute","status":"pending"}},"options":[{{"optionId":"allow-once","name":"Allow once","kind":"allow_once"}},{{"optionId":"reject-once","name":"Reject","kind":"reject_once"}}]}}}}}}
{{"expect_response":{{"id":"perm-1","result":{{"outcome":{{"outcome":"selected","optionId":"allow-once"}}}}}}}}
{{"respond":{{"to":"p1","result":{{"stopReason":"end_turn"}}}}}}
"#
    )
}

/// Runs the prompt until the turn is live.
pub(super) fn prompting(rig: &mut Rig) -> (ThreadId, TurnId) {
    let thread = rig.thread();
    let turn = send_prompt(rig, 10, thread);
    rig.run_until("the prompt", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.phase == TurnPhase::Prompting)
    });
    (thread, turn)
}

/// The answer text the log holds for `turn`, all of it.
pub(super) fn answer(log: &[DomainEvent], turn: TurnId) -> String {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::TextAppended(appended) = event
                && appended.turn == turn
            {
                Some(appended.text.as_str())
            } else {
                None
            }
        })
        .collect()
}

/// How `turn` ended: its stop, and its failure's code when it failed.
pub(super) fn ended(log: &[DomainEvent], turn: TurnId) -> (Option<StopReason>, Option<ErrorCode>) {
    let stop = log.iter().find_map(|event| {
        if let DomainEvent::TurnEnded(ended) = event
            && ended.turn == turn
        {
            Some(ended.stop)
        } else {
            None
        }
    });
    let failed = log.iter().find_map(|event| {
        if let DomainEvent::TurnFailed(failed) = event
            && failed.turn == turn
        {
            Some(failed.code)
        } else {
            None
        }
    });
    (stop, failed)
}

#[test]
fn a_connection_note_then_silence_ends_the_turn_offline_and_keeps_the_answer() {
    let mut rig = Rig::start("offline-quiet", &quiet());
    let (thread, turn) = prompting(&mut rig);
    rig.run_until("the note's watch", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.offline.is_some())
    });
    rig.clock.advance(OFFLINE_WAIT);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    let kinds = || log.iter().map(kind).collect::<Vec<_>>();
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline)),
        "{:?}",
        kinds()
    );
    let text = answer(&log, turn);
    assert_eq!(text, "Clause five says", "{:?}", kinds());
    rig.finish();
}

#[test]
fn work_after_the_note_clears_it_and_the_note_stays_out_of_the_answer() {
    let mut rig = Rig::start("offline-recovered", &recovered());
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::EndTurn), None));
    assert_eq!(
        answer(&log, turn),
        "Clause five says the notice period is thirty days."
    );
    rig.finish();
}

#[test]
fn a_prompt_failing_on_a_refused_connection_ends_offline() {
    let message = "stream disconnected before completion: error sending request for url \
                   (https://chatgpt.com/backend-api/codex/responses): client error (Connect): \
                   tcp connect error: Connection refused (os error 111)";
    let mut rig = Rig::start("offline-prompt", &failing(message));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

#[test]
fn a_prompt_failing_on_a_server_error_keeps_its_own_code() {
    let message = "unexpected status 503 Service Unavailable: upstream connect error";
    let mut rig = Rig::start("offline-server", &failing(message));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (
            Some(StopReason::Failed),
            Some(ErrorCode::AgentProtocolError)
        )
    );
    rig.finish();
}

#[test]
fn a_permission_request_after_the_note_is_never_cancelled_as_offline() {
    let mut rig = Rig::start("offline-asking", &asking());
    let (thread, turn) = prompting(&mut rig);
    rig.run_until("the permission request", |actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| !agent.permissions.is_empty())
    });
    // The lawyer reads the card for longer than the wait.
    rig.clock.advance(OFFLINE_WAIT * 2);
    rig.run_tasks_until_stalled();
    let current = rig.actor.threads[&thread].turn.current.as_ref().unwrap();
    assert!(current.live() && current.failure.is_none() && current.offline.is_none());
    let agent = rig.actor.threads[&thread].turn.agent.as_deref().unwrap();
    let request = *agent.permissions.keys().next().unwrap();
    let answer = HostCommand::AnswerPermission {
        thread,
        request,
        option_id: AgentChoiceId::new("allow-once").unwrap(),
    };
    rig.dispatch(11, answer).unwrap();
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::EndTurn), None)
    );
    rig.finish();
}

/// Scenario A of the review: the note armed the watch, then the server answered 429.
#[test]
fn a_usage_limit_within_the_wait_is_never_offline() {
    let message = "You've hit your usage limit. Upgrade to Pro or try again at 3pm.";
    let mut rig = Rig::start("offline-limit", &failing_after_note(message));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let (stop, code) = ended(&rig.log(thread), turn);
    assert_eq!(stop, Some(StopReason::Failed));
    assert_ne!(code, Some(ErrorCode::AgentOffline));
    rig.finish();
}

/// Scenario B: the note armed the watch, then the token refresh asks for a sign-in.
#[test]
fn a_sign_in_within_the_wait_is_never_offline() {
    let message = "Your access token could not be refreshed: refresh_token_reused";
    let mut rig = Rig::start("offline-signin", &failing_after_note(message));
    rig.actor.wiring.sign_in.again = Some(|text| text.contains("refresh_token_reused"));
    let (thread, turn) = prompting(&mut rig);
    rig.run_until("the sign-in failure", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_none_or(|current| current.turn != turn || current.phase != TurnPhase::Prompting)
    });
    rig.settle_commits();
    let log = rig.log(thread);
    let code = log.iter().find_map(|event| {
        if let DomainEvent::TurnFailed(failed) = event
            && failed.turn == turn
        {
            Some(failed.code)
        } else {
            None
        }
    });
    assert_eq!(
        code,
        Some(ErrorCode::AgentAuthRequired),
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
    rig.finish();
}

#[test]
fn claude_s_line_goes_back_in_order_when_the_answer_goes_on() {
    let rest = r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{"type":"text","text":" thirty days."}}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#;
    let mut rig = Rig::start("offline-claude-on", &claude(rest));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::EndTurn), None));
    assert_eq!(
        answer(&log, turn),
        format!("Clause five says{CLAUDE} thirty days.")
    );
    rig.finish();
}

#[test]
fn claude_s_line_goes_back_when_the_prompt_ends_well() {
    let rest = r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#;
    let mut rig = Rig::start("offline-claude-ok", &claude(rest));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::EndTurn), None));
    assert_eq!(answer(&log, turn), format!("Clause five says{CLAUDE}"));
    rig.finish();
}

#[test]
fn claude_s_line_is_dropped_when_the_turn_ends_offline() {
    let rest = format!(
        r#"{{"respond":{{"to":"p1","error":{{"code":-32603,"message":"Internal error: {CLAUDE}"}}}}}}"#
    );
    let mut rig = Rig::start("offline-claude-failed", &claude(&rest));
    let (thread, turn) = prompting(&mut rig);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

/// The lawyer cancels while Claude's line is held, and the agent never answers the cancel: the
/// fallback stops the agent and the turn ends `Cancelled` with the line put back.
#[test]
fn claude_s_line_goes_back_when_a_cancel_the_agent_never_answers_ends_the_turn() {
    let rest = r#"{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}"#;
    let mut rig = Rig::start("offline-claude-cancel", &claude(rest));
    let (thread, turn) = prompting(&mut rig);
    rig.run_until("the held line", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.offline.is_some())
    });
    rig.dispatch(11, HostCommand::Cancel { thread }).unwrap();
    rig.clock.advance(CANCEL_WAIT);
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Cancelled), None),
        "{:?}",
        log.iter().map(kind).collect::<Vec<_>>()
    );
    assert_eq!(answer(&log, turn), format!("Clause five says{CLAUDE}"));
    rig.finish();
}

/// Codex's note while a tool call is open: the model call failed, so the open tool call is stale
/// and the turn ends offline after the wait once the probe cannot get through either.
#[test]
fn a_note_ends_the_turn_offline_even_with_a_tool_call_open() {
    let script = format!(
        r#"{{"expect":{{"method":"session/prompt","as":"p1","params":{{"sessionId":"sess-1"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"tool_call","toolCallId":"t-1","title":"Run the check","kind":"execute","status":"in_progress"}}}}}}
{{"update":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{NOTE}"}}}}}}}}
{{"expect":{{"method":"session/cancel","params":{{"sessionId":"sess-1"}}}}}}
{{"respond":{{"to":"p1","result":{{"stopReason":"cancelled"}}}}}}
"#
    );
    let mut rig = Rig::start("offline-note-tool", &script);
    rig.actor.turn_path.probe = ReachProbe::fixed(false);
    let (thread, turn) = prompting(&mut rig);
    rig.run_until("the note's watch over the open tool call", |actor| {
        let open = actor
            .loaded(clauseo_protocol::StreamId::Thread(thread))
            .is_some_and(|loaded| loaded.sent.unfinished_tools(turn).next().is_some());
        let watched = actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.offline.is_some());
        open && watched
    });
    rig.clock.advance(OFFLINE_WAIT);
    until_ended(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    rig.finish();
}

/// Claude's line, the prompt fails offline, and the agent exits while the turn ends (its
/// after-turn checkpoint held back): the line stays out of the answer.
#[test]
fn claude_s_line_stays_dropped_when_the_agent_exits_during_an_offline_end() {
    let rest = format!(
        r#"{{"respond":{{"to":"p1","error":{{"code":-32603,"message":"Internal error: {CLAUDE}"}}}}}}
{{"exit":{{"code":1}}}}"#
    );
    let mut rig = Rig::start("offline-claude-exit", &claude(&rest));
    let (thread, turn) = prompting(&mut rig);
    rig.run_holding(
        "the agent's exit while the turn ends",
        |_, input| matches!(input, crate::actor::input::Input::Matter { .. }),
        |actor| actor.threads[&thread].turn.agent.is_none(),
    );
    let phase = rig.actor.threads[&thread]
        .turn
        .current
        .as_ref()
        .map(|current| current.phase);
    assert_eq!(phase, Some(TurnPhase::Ending));
    rig.release_held();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

/// Codex's fallback note, then 45 s of silence (a reasoning model retrying over HTTPS): the probe
/// gets through, so the watch waits again, and the answer goes on to a normal end.
#[test]
fn a_note_whose_probe_gets_through_waits_and_the_answer_goes_on() {
    let mut rig = Rig::start("offline-note-probe-ok", &recovered());
    rig.actor.turn_path.probe = ReachProbe::fixed(true);
    let (thread, turn) = prompting(&mut rig);
    let watch_since = |actor: &crate::actor::Actor| {
        let current = actor.threads[&thread].turn.current.as_ref()?;
        let watch = current.offline.as_ref().filter(|watch| !watch.probing())?;
        Some(watch.since())
    };
    // Whatever the agent sends after its note waits until the 45 s have passed.
    let hold = move |actor: &crate::actor::Actor, input: &Input| {
        let watched = actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.offline.is_some());
        watched
            && matches!(
                input,
                Input::Agent { .. } | Input::Turn(TurnInput::Agent { .. })
            )
    };
    rig.run_holding("the note's watch", hold, |actor| {
        watch_since(actor).is_some()
    });
    let since = watch_since(&rig.actor).unwrap();
    rig.clock.advance(OFFLINE_WAIT);
    rig.run_holding("the probe's answer", hold, |actor| {
        watch_since(actor).is_some_and(|armed| armed > since)
    });
    rig.clock.advance(OFFLINE_WAIT / 2);
    let current = rig.actor.threads[&thread].turn.current.as_ref().unwrap();
    assert!(current.live() && current.failure.is_none());
    rig.release_held();
    until_ended(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(ended(&log, turn), (Some(StopReason::EndTurn), None));
    assert_eq!(
        answer(&log, turn),
        "Clause five says the notice period is thirty days."
    );
    rig.finish();
}
