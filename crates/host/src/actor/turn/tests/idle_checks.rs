//! Each check of the idle stop's hard rule on its own (R20b): a pending permission, a pending
//! form elicitation, a launch and a turn held for the sign-in, each with no turn and no queued
//! prompt beside it, so the test fails if that one check is taken out of `idle::pending`.
//! Permissions, elicitations and the held marks are put in place by hand, as the URL
//! elicitation's test does (`idle_busy.rs`): in a real flow they come with a turn or a queued
//! prompt, which hold the agent too. The launch is real: a `SetMode` on a thread with no agent.

use clauseo_acp::PendingKey;
use clauseo_protocol::{
    AgentChoiceId, CommandEnvelope, ElicitationId, HostCommand, PermissionRequestId, ThreadId,
};
use futures::channel::oneshot;

use super::agents::send_prompt;
use super::idle::{idle_since, rig, stops_after_a_full_limit, until_idle};
use super::idle_busy::kept_past_the_limit;
use super::rig::{Rig, command_id};
use crate::actor::Actor;
use crate::actor::input::{Input, Request};
use crate::actor::turn::agents::Agent;
use crate::actor::turn::idle::Pending;
use crate::actor::turn::{AgentReply, TurnInput, TurnState, idle_tick, queue};

/// One prompt, answered.
const ONE_ANSWER: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{"type":"text","text":"First."}}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
"#;

/// One mode change, answered.
const ONE_MODE: &str = r#"{"expect":{"method":"session/set_mode","as":"m1","params":{"sessionId":"sess-1","modeId":"plan"}}}
{"respond":{"to":"m1","result":{}}}
"#;

/// `thread`'s turn state.
fn turn_state(rig: &mut Rig, thread: ThreadId) -> &mut TurnState {
    &mut rig.actor.threads.get_mut(&thread).unwrap().turn
}

/// `thread`'s agent.
fn agent(rig: &mut Rig, thread: ThreadId) -> &mut Agent {
    turn_state(rig, thread).agent.as_deref_mut().unwrap()
}

/// Nothing beside the item under test: no turn, nothing queued.
fn alone(rig: &Rig, thread: ThreadId) {
    let state = &rig.actor.threads[&thread].turn;
    assert!(state.current.is_none() && state.closing.is_none());
    assert_eq!(queue::waits(&state.queue), None);
}

/// A thread whose one turn has ended and whose agent is idle.
fn idle_thread(test: &str) -> (Rig, ThreadId) {
    let mut rig = rig(test, ONE_ANSWER, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    (rig, thread)
}

#[test]
fn a_pending_permission_alone_keeps_its_agent() {
    let (mut rig, thread) = idle_thread("idle-alone-permission");
    let now = rig.actor.now().wall;
    let id: PermissionRequestId = rig.actor.ids.mint(now).unwrap();
    agent(&mut rig, thread)
        .permissions
        .insert(id, PendingKey(7));
    idle_tick(&mut rig.actor);
    alone(&rig, thread);
    kept_past_the_limit(&mut rig, thread, Pending::Ask);
    agent(&mut rig, thread).permissions.clear();
    idle_tick(&mut rig.actor);
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

#[test]
fn a_pending_form_elicitation_alone_keeps_its_agent() {
    let (mut rig, thread) = idle_thread("idle-alone-elicitation");
    let now = rig.actor.now().wall;
    let id: ElicitationId = rig.actor.ids.mint(now).unwrap();
    agent(&mut rig, thread)
        .elicitations
        .insert(id, PendingKey(8));
    idle_tick(&mut rig.actor);
    alone(&rig, thread);
    kept_past_the_limit(&mut rig, thread, Pending::Ask);
    agent(&mut rig, thread).elicitations.clear();
    idle_tick(&mut rig.actor);
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

/// Both of the held marks, one after the other: the turn the sign-in stop cancelled
/// (`stopped_for_sign_in`), and the turn ended for the sign-in that goes back to the queue
/// once its close is durable (the queue's `requeue`).
#[test]
fn a_turn_held_for_the_sign_in_alone_keeps_its_agent() {
    let (mut rig, thread) = idle_thread("idle-alone-held");
    let now = rig.actor.now().wall;
    let held = rig.actor.ids.mint(now).unwrap();

    turn_state(&mut rig, thread).stopped_for_sign_in = Some(held);
    idle_tick(&mut rig.actor);
    alone(&rig, thread);
    kept_past_the_limit(&mut rig, thread, Pending::Held);
    turn_state(&mut rig, thread).stopped_for_sign_in = None;
    idle_tick(&mut rig.actor);
    assert_eq!(idle_since(&rig.actor, thread), Some(rig.actor.now().mono));

    queue::set_requeue_for_test(&mut turn_state(&mut rig, thread).queue, Some(held));
    idle_tick(&mut rig.actor);
    assert!(turn_state(&mut rig, thread).current.is_none());
    kept_past_the_limit(&mut rig, thread, Pending::Held);
    queue::set_requeue_for_test(&mut turn_state(&mut rig, thread).queue, None);
    idle_tick(&mut rig.actor);
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

/// A `SetMode` on a thread with no agent launches one, with no turn: while its `session/new`
/// is in flight the launch alone holds it. The clock starts once the mode call is answered.
#[test]
fn a_launch_with_no_turn_keeps_its_agent() {
    let mut rig = rig("idle-alone-launch", ONE_MODE, "exit");
    let thread = rig.thread();
    let (reply, _answer) = oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(10),
        command: HostCommand::SetMode {
            thread,
            mode_id: AgentChoiceId::new("plan").unwrap(),
        },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
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
    alone(&rig, thread);
    kept_past_the_limit(&mut rig, thread, Pending::Launch);
    rig.release_held();
    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}
