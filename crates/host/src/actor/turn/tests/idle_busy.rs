//! The idle stop never touches an agent with something pending (R20b's hard rule): a running
//! turn, a queued prompt, a pending permission or elicitation, a turn held across an expired
//! sign-in, a launch or a load. Each test holds its item past twice the limit, sees no stop and
//! no idle clock, ends the item, and sees the clock start at that moment: no stop one
//! millisecond before a full limit after it, a stop at it.

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, CompactString, DeliveryRequest, DomainEvent, ElicitationId,
    HostCommand, PromptBlock, ThreadId, TurnId,
};

use super::agents::send_prompt;
use super::idle::{
    TWO_ANSWERS, advance, idle_since, idle_stops, pending_of, rig, stops_after_a_full_limit,
    until_idle,
};
use super::rig::Rig;
use crate::actor::Actor;
use crate::actor::input::Input;
use crate::actor::turn::agents::Stage;
use crate::actor::turn::idle::Pending;
use crate::actor::turn::{AgentReply, TurnInput, TurnPhase};
use crate::config::IDLE_LIMIT;

/// Twice the limit passes and `thread`'s agent stays live, its idle clock not started, for
/// `why`.
fn kept_past_the_limit(rig: &mut Rig, thread: ThreadId, why: Pending) {
    assert_eq!(pending_of(&rig.actor, thread), Some(why));
    advance(rig, IDLE_LIMIT * 2);
    let stage = rig.actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .map(|agent| agent.stage);
    let detached: Vec<_> = rig
        .log(thread)
        .into_iter()
        .filter(|event| matches!(event, DomainEvent::AgentSessionDetached(_)))
        .collect();
    assert!(
        stage.is_some_and(|stage| stage != Stage::Stopping),
        "stopped with {why:?} pending: {stage:?} {detached:?}"
    );
    assert_eq!(idle_since(&rig.actor, thread), None);
    assert_eq!(pending_of(&rig.actor, thread), Some(why));
    assert_eq!(idle_stops(rig, thread), 0);
}

/// Whether `input` is the answer to a prompt.
fn prompt_answer(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            reply: AgentReply::Prompt { .. },
            ..
        })
    )
}

/// Sends `text` to `thread` as command `n` with `delivery`.
fn send(rig: &mut Rig, n: u128, thread: ThreadId, text: &str, delivery: DeliveryRequest) -> TurnId {
    let content = vec![PromptBlock::Text {
        text: text.to_owned(),
    }];
    let command = HostCommand::SendPrompt {
        thread,
        content,
        delivery,
    };
    let outcome = rig.dispatch(n, command).unwrap();
    let CommandOutcome::PromptAccepted { turn, .. } = outcome else {
        panic!("not accepted: {outcome:?}");
    };
    turn
}

#[test]
fn a_running_turn_keeps_its_agent() {
    let mut rig = rig("idle-busy-turn", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    rig.hold_next("the prompt's answer", prompt_answer);
    kept_past_the_limit(&mut rig, thread, Pending::Turn);
    rig.release_held();
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

/// One prompt, answered in part, that the lawyer cancels.
const CANCELLED: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{"type":"text","text":"Working."}}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"cancelled"}}}
"#;

#[test]
fn a_queued_prompt_keeps_its_agent() {
    let mut rig = rig("idle-busy-queued", CANCELLED, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    // The agent has the prompt once its first words are in.
    rig.run_until("the prompt out", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| {
                current.phase == TurnPhase::Prompting && current.first_words.is_some()
            })
    });
    let queued = send(
        &mut rig,
        11,
        thread,
        "And then this.",
        DeliveryRequest::Queue,
    );
    // The Stop ends the turn cancelled; the queued prompt stays, held.
    rig.dispatch(12, HostCommand::Cancel { thread }).unwrap();
    rig.run_until("the cancelled turn's end", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.closing.is_none() && actor.work.commits == 0
    });
    kept_past_the_limit(&mut rig, thread, Pending::Queued);
    let remove = HostCommand::RemoveQueuedPrompt {
        thread,
        turn: queued,
    };
    rig.dispatch(13, remove).unwrap();
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

/// One prompt that asks one permission, then answers once it is answered.
const PERMISSION: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"request":{"id":"perm-1","method":"session/request_permission","params":{"sessionId":"sess-1","toolCall":{"toolCallId":"t-edit-1","title":"Edit brief.txt","kind":"edit","status":"pending"},"options":[{"optionId":"allow-once","name":"Allow once","kind":"allow_once"},{"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}}}
{"expect_response":{"id":"perm-1","result":{"outcome":{"outcome":"selected","optionId":"allow-once"}}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
"#;

#[test]
fn a_pending_permission_keeps_its_agent() {
    let mut rig = rig("idle-busy-permission", PERMISSION, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    rig.run_until("the permission request", |actor| {
        actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| !agent.permissions.is_empty())
    });
    rig.settle_commits();
    // The turn waits on the lawyer: both hold the agent.
    kept_past_the_limit(&mut rig, thread, Pending::Turn);
    let agent = rig.actor.threads[&thread].turn.agent.as_deref().unwrap();
    assert!(!agent.permissions.is_empty());
    let request = *agent.permissions.keys().next().unwrap();
    let answer = HostCommand::AnswerPermission {
        thread,
        request,
        option_id: AgentChoiceId::new("allow-once").unwrap(),
    };
    rig.dispatch(11, answer).unwrap();
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

/// An elicitation that outlives its turn (a URL elicitation the lawyer accepted counts until
/// the agent says it is complete) holds the agent on its own; the clock starts when it goes.
#[test]
fn a_pending_elicitation_keeps_its_agent() {
    let mut rig = rig("idle-busy-elicitation", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let now = rig.actor.now().wall;
    let id: ElicitationId = rig.actor.ids.mint(now).unwrap();
    let agent = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .agent
        .as_deref_mut()
        .unwrap();
    agent
        .url_elicitations
        .insert(CompactString::from("el-1"), id);
    crate::actor::turn::idle_tick(&mut rig.actor);
    kept_past_the_limit(&mut rig, thread, Pending::Ask);
    let agent = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .agent
        .as_deref_mut()
        .unwrap();
    agent.url_elicitations.clear();
    // The tick after the input that ended it.
    crate::actor::turn::idle_tick(&mut rig.actor);
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

/// One prompt that fails for the sign-in (JSON-RPC -32000).
const AUTH_REQUIRED: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","error":{"code":-32000,"message":"Authentication required"}}}
"#;

#[test]
fn a_turn_held_for_the_sign_in_keeps_its_agent() {
    let mut rig = rig("idle-busy-held", AUTH_REQUIRED, "exit");
    let thread = rig.thread();
    let failed = send_prompt(&mut rig, 10, thread);
    // The failed turn goes back to the queue's head, held for the sign-in.
    let put_back = |actor: &mut Actor| {
        actor.work.commits == 0
            && actor.threads[&thread].turn.current.is_none()
            && super::super::queue::queued_turns(actor, thread)
                .first()
                .is_some_and(|turn| *turn != failed)
    };
    rig.run_until("the turn put back", put_back);
    kept_past_the_limit(&mut rig, thread, Pending::Held);
    let held = super::super::queue::queued_turns(&rig.actor, thread)[0];
    rig.dispatch(11, HostCommand::RemoveQueuedPrompt { thread, turn: held })
        .unwrap();
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

#[test]
fn a_launch_and_a_load_keep_their_agent() {
    let mut rig = rig("idle-busy-launch", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    // The launch's folder checks run off the actor: no agent yet, nothing to stop.
    let prepared =
        |_: &Actor, input: &Input| matches!(input, Input::Turn(TurnInput::Prepared { .. }));
    rig.hold_next("the launch's checks", prepared);
    advance(&mut rig, IDLE_LIMIT * 2);
    assert_eq!(rig.actor.turn_path.idle.armed_at(), None);
    rig.release_held();
    // `session/new` is in flight: the agent is loading.
    let session = |_: &Actor, input: &Input| {
        matches!(
            input,
            Input::Turn(TurnInput::Agent {
                reply: AgentReply::Session { .. },
                ..
            })
        )
    };
    rig.hold_next("the session's answer", session);
    kept_past_the_limit(&mut rig, thread, Pending::Launch);
    rig.release_held();
    // The turn that waited for the session runs next; the clock starts at its end.
    stops_after_a_full_limit(&mut rig, thread);
    assert!(
        rig.log(thread)
            .iter()
            .any(|event| matches!(event, DomainEvent::TurnEnded(_)))
    );
    rig.finish();
}
