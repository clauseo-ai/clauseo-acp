//! What a message naming an agent keeps and is refused with (`switch.rs`): the agent named is
//! kept, the thread's own included, so a message queued behind a switch still reaches the agent
//! it named; a restart reads a queued switch back from its `UserMessage`; another agent whose
//! sign-in refuses a launch, or a thread with an open permission, refuses the message before it
//! is logged. A first message to another agent switches with no handover, a request too big for
//! even the handover's header fails and keeps the agent, and start-up settles a handover a stop
//! left pending.

use clauseo_protocol::{
    AgentId, CommandOutcome, ContextHandoff, Count, Delivery, DetachReason, DomainEvent, Endpoint,
    ErrorCode, HandoffStatus, HostCommand, NoticeKind, PermissionRequestId, StopReason,
    UserMessage,
};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::rig::Rig;
use super::switch::{OTHER, count, one_turn, send, send_text, two_agents, until_idle};
use crate::actor::turn::{AgentReply, TurnInput, queue};

/// The new agent's steps: prompts, each answered `end_turn`.
const PROMPTS_END: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

const FAKE: &str = "fake-agent";

/// A prompt's answer.
fn prompt_reply(_: &Actor, input: &Input) -> bool {
    matches!(
        input,
        Input::Turn(TurnInput::Agent {
            reply: AgentReply::Prompt { .. },
            ..
        })
    )
}

fn switches(log: &[DomainEvent]) -> Vec<(String, String)> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::AgentSwitched(s) = event {
                Some((s.from.to_string(), s.to.to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn messages(log: &[DomainEvent]) -> Vec<UserMessage> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::UserMessage(message) = event {
                Some(message.clone())
            } else {
                None
            }
        })
        .collect()
}

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap()
}

fn accepted(rig: &mut Rig, n: u128, command: HostCommand) {
    let sent = rig.dispatch(n, command);
    assert!(
        matches!(sent, Ok(CommandOutcome::PromptAccepted { .. })),
        "{sent:?}"
    );
}

/// The reviewer's case: "`ChatGPT`, review" queued on a Claude thread, then "Claude, redraft".
/// The second message names the thread's agent as it was sent, and reaches that agent after
/// the first switched the thread away.
#[test]
fn a_message_naming_the_threads_agent_behind_a_queued_switch_goes_to_it() {
    let mut rig = two_agents("switch-back", Some(PROMPTS_END));
    let thread = rig.thread();
    let sent = rig.dispatch_holding(10, send(thread, None), prompt_reply);
    assert!(sent.is_ok(), "{sent:?}");
    if rig.held.is_empty() {
        rig.hold_next("the first answer", prompt_reply);
    }
    accepted(
        &mut rig,
        11,
        send_text(thread, Some(OTHER), "ChatGPT, review"),
    );
    accepted(
        &mut rig,
        12,
        send_text(thread, Some(FAKE), "Claude, redraft"),
    );
    assert_eq!(
        queue::queued_agents(&rig.actor, thread),
        [Some(agent(OTHER)), Some(agent(FAKE))],
        "each keeps the agent it named"
    );
    rig.release_held();
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    let expected = [
        (FAKE.to_owned(), OTHER.to_owned()),
        (OTHER.to_owned(), FAKE.to_owned()),
    ];
    assert_eq!(switches(&log), expected, "{log:?}");
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), FAKE);
    let named: Vec<_> = messages(&log).into_iter().map(|m| m.agent).collect();
    assert_eq!(named, [None, Some(agent(OTHER)), Some(agent(FAKE))]);
    let ends = count(
        &log,
        |e| matches!(e, DomainEvent::TurnEnded(t) if t.stop == StopReason::EndTurn),
    );
    assert_eq!(ends, 3, "{log:?}");
    rig.finish();
}

/// A switch still queued when the host stops comes back with its agent, and switches once the
/// lawyer resumes the queue.
#[test]
fn a_queued_switch_survives_a_restart() {
    let mut rig = two_agents("switch-restart", Some(PROMPTS_END));
    let thread = rig.thread();
    let now = rig.actor.now().wall;
    let message = UserMessage {
        turn: rig.actor.ids.mint(now).unwrap(),
        message: rig.actor.ids.mint(now).unwrap(),
        acp_message_id: None,
        content: Vec::new(),
        delivery: Delivery::Queue,
        config: std::collections::BTreeMap::new(),
        copy_of: None,
        agent: Some(agent(OTHER)),
    };
    let turn = message.turn;
    rig.commit(thread, vec![message.into()]);
    rig.settle_commits();
    let mut rig = rig.restart(1);
    rig.run_until("the queue restored", |actor| {
        actor.work.loads == 0 && actor.work.commits == 0
    });
    assert_eq!(queue::queued_turns(&rig.actor, thread), [turn]);
    assert_eq!(
        queue::queued_agents(&rig.actor, thread),
        [Some(agent(OTHER))]
    );
    let resumed = rig.dispatch(20, HostCommand::ResumeQueue { thread });
    assert!(resumed.is_ok(), "{resumed:?}");
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        switches(&log),
        [(FAKE.to_owned(), OTHER.to_owned())],
        "{log:?}"
    );
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    rig.finish();
}

/// No earlier finished turn: the switch goes ahead with no handover and no notice.
#[test]
fn a_first_message_to_another_agent_switches_with_no_handover() {
    let mut rig = two_agents("switch-first", Some(PROMPTS_END));
    let thread = rig.thread();
    accepted(&mut rig, 10, send(thread, Some(OTHER)));
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        switches(&log),
        [(FAKE.to_owned(), OTHER.to_owned())],
        "{log:?}"
    );
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::ContextHandoff(_))),
        0
    );
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::HandoffSettled(_))),
        0
    );
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::AgentSessionDetached(_))),
        0
    );
    let notices = count(
        &log,
        |e| matches!(e, DomainEvent::Notice(n) if n.kind == Some(NoticeKind::ContextNotRestored)),
    );
    assert_eq!(notices, 0);
    let ends = count(
        &log,
        |e| matches!(e, DomainEvent::TurnEnded(t) if t.stop == StopReason::EndTurn),
    );
    assert_eq!(ends, 1, "{log:?}");
    rig.finish();
}

/// A request that leaves no room even for the handover's header: the turn fails with
/// `prompt.handoff_too_long`, no switch is logged, and the old agent's stop detaches `Closed`.
#[test]
fn a_request_too_big_even_for_the_header_fails_and_keeps_the_agent() {
    let mut rig = two_agents("switch-too-long", Some(PROMPTS_END));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    accepted(
        &mut rig,
        11,
        send_text(thread, Some(OTHER), &"x".repeat(100_000)),
    );
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    let failed = log.iter().find_map(|event| {
        if let DomainEvent::TurnFailed(failed) = event {
            Some(failed.code)
        } else {
            None
        }
    });
    assert_eq!(failed, Some(ErrorCode::PromptHandoffTooLong), "{log:?}");
    assert_eq!(switches(&log), []);
    let handoffs: Vec<_> = log
        .iter()
        .filter_map(|event| {
            if let DomainEvent::ContextHandoff(handoff) = event {
                Some(handoff.items_included.get())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(handoffs, [0]);
    let settled: Vec<_> = log
        .iter()
        .filter_map(|event| {
            if let DomainEvent::HandoffSettled(settled) = event {
                Some(settled.status)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(settled, [HandoffStatus::Failed]);
    let detached: Vec<_> = log
        .iter()
        .filter_map(|event| {
            if let DomainEvent::AgentSessionDetached(detached) = event {
                Some(detached.reason.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        detached,
        [DetachReason::Closed],
        "a plain stop's reason: {log:?}"
    );
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), FAKE);
    rig.finish();
}

/// Another agent whose sign-in would refuse its launch refuses the message with that launch's
/// code, before anything is logged; a hold for the sign-in refuses it `agent.auth_required`.
#[test]
fn a_switch_to_an_agent_its_sign_in_refuses_is_refused_at_send() {
    let mut rig = two_agents("switch-sign-in", Some(PROMPTS_END));
    let thread = rig.thread();
    let code = ErrorCode::SigninPolicyUnreadable;
    rig.actor
        .wiring
        .sign_in
        .refused
        .set(agent(OTHER), Some(code))
        .unwrap();
    let refused = rig.dispatch(10, send(thread, Some(OTHER))).unwrap_err();
    assert_eq!(refused.code(), code);
    rig.actor
        .wiring
        .sign_in
        .refused
        .set(agent(OTHER), None)
        .unwrap();
    let hold = crate::agent::login_link::StopFor::SignInAgain;
    rig.actor.wiring.sign_in.held.insert(agent(OTHER), hold);
    let refused = rig.dispatch(11, send(thread, Some(OTHER))).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::AgentAuthRequired);
    rig.settle_commits();
    assert_eq!(messages(&rig.log(thread)), [], "nothing logged");
    rig.finish();
}

/// An open permission on the thread's agent refuses a switch with `thread.background_work`.
#[test]
fn an_open_permission_refuses_a_switch() {
    let mut rig = two_agents("switch-permission", Some(PROMPTS_END));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let now = rig.actor.now().wall;
    let id: PermissionRequestId = rig.actor.ids.mint(now).unwrap();
    let state = &mut rig.actor.threads.get_mut(&thread).unwrap().turn;
    let live = state.agent.as_deref_mut().expect("a live agent");
    live.permissions.insert(id, clauseo_acp::PendingKey(7));
    let refused = rig.dispatch(11, send(thread, Some(OTHER))).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::ThreadBackgroundWork);
    let state = &mut rig.actor.threads.get_mut(&thread).unwrap().turn;
    state.agent.as_deref_mut().unwrap().permissions.clear();
    rig.settle_commits();
    assert_eq!(
        messages(&rig.log(thread)).len(),
        1,
        "only the first turn's message"
    );
    rig.finish();
}

/// A handover a stop left pending (the host stopped between the switch's commit and its
/// settle) settles `Failed` at the next start.
#[test]
fn start_up_settles_a_pending_handover_failed() {
    let mut rig = two_agents("switch-pending-start", None);
    let thread = rig.thread();
    let now = rig.actor.now().wall;
    let end = |agent: &str| Endpoint {
        agent: AgentId::new(agent).unwrap(),
        model: None,
        effort: None,
    };
    let handoff = ContextHandoff {
        handoff: rig.actor.ids.mint(now).unwrap(),
        message: rig.actor.ids.mint(now).unwrap(),
        from: end(FAKE),
        to: end(OTHER),
        answers_covered: Count::clamped(1),
        items_included: Count::clamped(2),
        items_left_out: Count::clamped(0),
        resumed: false,
    };
    let id = handoff.handoff;
    rig.commit(thread, vec![handoff.into()]);
    rig.settle_commits();
    let mut rig = rig.restart(1);
    rig.run_until("start-up's repairs", |actor| {
        actor.work.loads == 0 && actor.work.commits == 0
    });
    let log = rig.log(thread);
    let settled: Vec<_> = log
        .iter()
        .filter_map(|event| {
            if let DomainEvent::HandoffSettled(settled) = event {
                Some((settled.handoff, settled.status))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(settled, [(id, HandoffStatus::Failed)], "{log:?}");
    rig.finish();
}
