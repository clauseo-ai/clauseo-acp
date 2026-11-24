//! A Tools change reaching the next prompt (`agents::restart_if_marked`): a prompt for a marked
//! agent, typed or taken from the queue, relaunches it before it goes, with nothing waited for.
//! A prompt during the grace, a prompt queued behind the turn the edit came in, an edit while the
//! agent launches, and a resumed session (`session/load` with the same id) each reach the relaunch
//! with the new server; an undo in flight keeps a marked agent; a relaunch that fails ends the
//! prompt's turn failed, with the message kept.

use clauseo_protocol::{
    AgentChoiceId, CommandEnvelope, DetachReason, DomainEvent, FileChoice, FileDecision,
    HostCommand, MatterPath, ThreadId,
};
use futures::channel::oneshot;
use serde_json::{Value, json};

use super::agents::send_prompt;
use super::idle::{advance, idle_stops, live, pending_of, until_idle};
use super::rig::{Rig, command_id};
use super::tools_live::{
    answer_with_notes, one_answer, relaunches_with_notes, rig_with, stale, stopped_after_the_grace,
    turn_on, turns_ended, with_notes,
};
use crate::actor::Actor;
use crate::actor::input::{Input, Request};
use crate::actor::turn::TurnInput;
use crate::actor::turn::idle::Pending;
use crate::config::CAP_MIN_IDLE;

/// How many times `thread`'s agent was stopped to start again for a turn.
fn restarts(rig: &mut Rig, thread: ThreadId) -> usize {
    rig.log(thread)
        .iter()
        .filter(|event| {
            matches!(event, DomainEvent::AgentSessionDetached(detached)
                if detached.reason == DetachReason::Restarting)
        })
        .count()
}

#[test]
fn a_prompt_during_the_grace_relaunches_with_the_new_server() {
    let mut rig = rig_with("tools-prompt-grace", &one_answer(), &with_notes(), false);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    turn_on(&mut rig, 11, "notes");
    assert!(stale(&rig.actor, thread));
    assert!(live(&rig.actor, thread), "stopped before the grace");
    relaunches_with_notes(&mut rig, 12, thread);
    assert_eq!(restarts(&mut rig, thread), 1);
    assert_eq!(idle_stops(&mut rig, thread), 0);
    assert!(!stale(&rig.actor, thread));
    rig.finish();
}

#[test]
fn a_resumed_session_is_loaded_with_the_same_id_and_the_new_server() {
    let mut relaunch = vec![
        json!({"expect":{"method":"session/load","as":"load2","params":{"sessionId":"sess-1",
            "mcpServers":[{"name":"notes","command":"{{any}}"}]}}}),
        json!({"respond":{"to":"load2","result":{}}}),
    ];
    relaunch.extend(answer_with_notes());
    let relaunch = Value::Array(relaunch);
    let mut rig = rig_with("tools-prompt-load", &one_answer(), &relaunch, true);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    turn_on(&mut rig, 11, "notes");
    // The fake agent answers only a `session/load` of `sess-1` carrying `notes`.
    relaunches_with_notes(&mut rig, 12, thread);
    assert_eq!(restarts(&mut rig, thread), 1);
    rig.finish();
}

#[test]
fn a_prompt_queued_behind_the_turn_of_the_edit_goes_to_a_relaunched_agent() {
    let steps = vec![
        json!({"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}),
        json!({"request":{"id":"perm-1","method":"session/request_permission","params":{
            "sessionId":"sess-1","toolCall":{"toolCallId":"t-1","title":"Edit brief.txt",
            "kind":"edit","status":"pending"},"options":[{"optionId":"allow-once",
            "name":"Allow once","kind":"allow_once"}]}}}),
        json!({"expect_response":{"id":"perm-1","result":{"outcome":{"outcome":"selected",
            "optionId":"allow-once"}}}}),
        json!({"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}),
    ];
    let mut rig = rig_with("tools-prompt-queued", &steps, &with_notes(), false);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    rig.run_until("the permission request", |actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| !agent.permissions.is_empty())
    });
    send_prompt(&mut rig, 11, thread);
    turn_on(&mut rig, 12, "notes");
    assert!(live(&rig.actor, thread), "stopped in its turn");
    let agent = rig.actor.threads[&thread].turn.agent.as_deref().unwrap();
    let request = *agent.permissions.keys().next().unwrap();
    let answer = HostCommand::AnswerPermission {
        thread,
        request,
        option_id: AgentChoiceId::new("allow-once").unwrap(),
    };
    rig.dispatch(13, answer).unwrap();
    // The turn ends; the queued prompt relaunches the agent and is answered with `notes`.
    rig.run_until("the queued turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.agent.is_some() && actor.work.commits == 0
    });
    assert_eq!(turns_ended(&mut rig, thread), 2);
    let log = format!("{:?}", rig.log(thread));
    assert!(
        log.contains("With notes."),
        "the queued prompt went to the old agent"
    );
    assert_eq!(restarts(&mut rig, thread), 1);
    assert_eq!(idle_stops(&mut rig, thread), 0);
    rig.finish();
}

#[test]
fn an_edit_while_the_agent_launches_relaunches_it_before_the_prompt_goes() {
    let mut rig = rig_with("tools-prompt-launching", &[], &with_notes(), false);
    let thread = rig.thread();
    // The launch's prep (which takes the servers' key) is held while the edit comes.
    let prepared =
        |_: &Actor, input: &Input| matches!(input, Input::Turn(TurnInput::Prepared { .. }));
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![clauseo_protocol::PromptBlock::Text {
            text: "Read the brief.".to_owned(),
        }],
        delivery: clauseo_protocol::DeliveryRequest::Queue,
        config: None,
    };
    rig.dispatch_holding(10, command, prepared).unwrap();
    if rig.held.is_empty() {
        rig.hold_next("the launch's prep", prepared);
    }
    turn_on(&mut rig, 11, "notes");
    rig.release_held();
    rig.run_until("the turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.agent.is_some() && actor.work.commits == 0
    });
    assert_eq!(turns_ended(&mut rig, thread), 1);
    let log = format!("{:?}", rig.log(thread));
    assert!(
        log.contains("With notes."),
        "the prompt went to the agent launched first"
    );
    assert_eq!(restarts(&mut rig, thread), 1);
    rig.finish();
}

#[test]
fn an_undo_in_flight_keeps_a_marked_agent() {
    let steps = vec![
        json!({"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}),
        json!({"write_file":{"path":"brief.txt","content":"Clause 4 final\n"}}),
        json!({"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}),
    ];
    let mut rig = rig_with("tools-prompt-undo", &steps, &with_notes(), false);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    // The edit first (its receipt's commit would wait behind the undo's), then the undo.
    turn_on(&mut rig, 12, "notes");
    let undo = HostCommand::UndoStep {
        thread,
        turn,
        files: vec![FileDecision {
            path: MatterPath::new("brief.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    };
    let (reply, _answer) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(11),
        command: undo,
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    // The undo's commit is held: the undo stays in flight.
    rig.hold_next("the undo's commit", |actor, input| {
        matches!(input, Input::Committed { ticket, .. }
            if actor.pipeline.events(*ticket).iter()
                .any(|e| matches!(e.event, DomainEvent::StepUndone(_))))
    });
    assert!(stale(&rig.actor, thread));
    advance(&mut rig, CAP_MIN_IDLE * 2);
    assert!(live(&rig.actor, thread), "stopped with an undo in flight");
    assert_eq!(pending_of(&rig.actor, thread), Some(Pending::Turn));
    rig.release_held();
    let since = until_idle(&mut rig, thread);
    stopped_after_the_grace(&mut rig, thread, since);
    rig.finish();
}

#[test]
fn a_relaunch_that_fails_ends_the_prompt_s_turn_failed_with_the_message_kept() {
    let relaunch = json!([
        {"expect":{"method":"session/new","as":"new2","params":{
            "mcpServers":[{"name":"notes","command":"{{any}}"}]}}},
        {"respond":{"to":"new2","error":{"code":-32603,"message":"Internal error"}}}
    ]);
    let mut rig = rig_with("tools-prompt-fails", &one_answer(), &relaunch, false);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    turn_on(&mut rig, 11, "notes");
    let turn = send_prompt(&mut rig, 12, thread);
    rig.run_until("the failed turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && actor.work.commits == 0
    });
    let log = rig.log(thread);
    let asked = log
        .iter()
        .any(|event| matches!(event, DomainEvent::UserMessage(message) if message.turn == turn));
    assert!(asked, "the message was lost");
    let failed: Vec<_> = log
        .iter()
        .filter(|event| matches!(event, DomainEvent::TurnFailed(failed) if failed.turn == turn))
        .collect();
    assert_eq!(failed.len(), 1, "the turn did not fail: {failed:?}");
    rig.finish();
}
