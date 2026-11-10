//! Going offline mid-answer (`offline.rs`, D-050): the agent's note about its connection never
//! reaches the log; a note that names a connection failure, then silence, ends the turn through
//! the Cancel path as `Failed { agent.offline }` with the answer so far kept; work after the
//! note clears it; a prompt that fails with a connection-class error ends `agent.offline` too,
//! and a server error keeps its own code.

use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, ThreadId, TurnId};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::rig::{Rig, kind};
use crate::actor::turn::TurnPhase;
use crate::actor::turn::offline::OFFLINE_WAIT;

/// Codex's warning, as codex-acp forwards it, from stage C's sweep (macOS).
const NOTE: &str = "Falling back from WebSockets to HTTPS transport. stream disconnected before \
                    completion: Connection refused (os error 61)";

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

/// Runs the prompt until the turn is live.
fn prompting(rig: &mut Rig) -> (ThreadId, TurnId) {
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
fn answer(log: &[DomainEvent], turn: TurnId) -> String {
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
fn ended(log: &[DomainEvent], turn: TurnId) -> (Option<StopReason>, Option<ErrorCode>) {
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
