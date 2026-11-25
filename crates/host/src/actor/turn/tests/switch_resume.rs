//! A switch back (`switch/resume.rs`) whose session's attach commit the store refuses: the
//! handover it carried was never logged, so no `HandoffSettled` names it, and the next turn
//! owes it and carries it. And a handover whose prompt went out before a cancel: unconfirmed,
//! and not handed over again.

use clauseo_protocol::{
    AgentId, ContextHandoff, Count, DomainEvent, Endpoint, HandoffId, HandoffSettled,
    HandoffStatus, HostCommand,
};

use super::rig::{HANDSHAKE, Rig};
use super::switch::{OTHER, count, one_turn, send, send_text, two_agents, until_idle};
use crate::actor::turn::{CANCEL_WAIT, queue, switch};
use crate::pipeline::tests::support::TempDir;

/// `fake-agent`'s steps after the handshake: one prompt, answered with text, so its session
/// has seen the conversation.
const ANSWERS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#,
    "\n",
    r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{"type":"text","text":"Clause 4 is fine."}}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// [`OTHER`]'s steps after the handshake: one prompt, answered with text.
pub(super) const OTHER_ANSWERS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-2","content":{"type":"text","text":"Clause 5 is not."}}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// A rig whose `fake-agent` plays [`ANSWERS`] and whose [`OTHER`] plays [`OTHER_ANSWERS`] in
/// session `sess-2`; each launch reads its scenario from the top.
fn answering(test: &str) -> Rig {
    let dir = TempDir::new(test);
    Rig::prepare(dir.path(), test, ANSWERS, None);
    let path = dir.path().join("data").join("settings.json");
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    settings["agents"][OTHER] = settings["agents"]["fake-agent"].clone();
    let scenario = dir.path().join(format!("{test}-other.jsonl"));
    let header = format!(
        "{{\"scenario\":\"{test}-other\",\"description\":\"a unit test\",\
         \"on_stdin_close\":\"exit\"}}\n"
    );
    let handshake = HANDSHAKE.replace(r#"{"sessionId":"sess-1"}"#, r#"{"sessionId":"sess-2"}"#);
    let text = format!("{header}{handshake}{OTHER_ANSWERS}");
    crate::write::write_file(&scenario, text.as_bytes()).unwrap();
    let scenario = scenario.display().to_string();
    settings["agents"][OTHER]["args"] = serde_json::json!(["--scenario", scenario]);
    crate::write::write_file(&path, settings.to_string().as_bytes()).unwrap();
    Rig::open(dir, 0, None)
}

fn handoffs(log: &[DomainEvent]) -> Vec<HandoffId> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::ContextHandoff(handoff) = event {
                Some(handoff.handoff)
            } else {
                None
            }
        })
        .collect()
}

fn settled(log: &[DomainEvent]) -> Vec<(HandoffId, HandoffStatus)> {
    log.iter()
        .filter_map(|event| {
            if let DomainEvent::HandoffSettled(settled) = event {
                Some((settled.handoff, settled.status))
            } else {
                None
            }
        })
        .collect()
}

/// Back to `fake-agent`, whose session saw the first turn: the attach commit holding the
/// handover is refused. The handover is never settled, since the log does not hold it; the
/// next turn owes it and logs and delivers its own.
#[test]
fn a_refused_attach_never_settles_its_handover_and_the_next_turn_owes_it() {
    let mut rig = answering("resume-attach-refused");
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let sent = rig.dispatch(11, send(thread, Some(OTHER)));
    assert!(sent.is_ok(), "{sent:?}");
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(handoffs(&log).len(), 1, "{log:?}");

    // The switch back holds its handover for the attach; the store refuses that commit.
    rig.fail(1, |event| matches!(event, DomainEvent::ContextHandoff(_)));
    let sent = rig.dispatch(12, send(thread, Some("fake-agent")));
    assert!(sent.is_ok(), "{sent:?}");
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    let logged = handoffs(&log);
    assert_eq!(
        logged.len(),
        1,
        "the refused handover is not in the log: {log:?}"
    );
    for (handoff, _) in settled(&log) {
        assert!(
            logged.contains(&handoff),
            "a settle names a handover the log does not hold: {log:?}"
        );
    }
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::AgentSwitched(_))),
        2,
        "{log:?}"
    );
    let (_, owed) = switch::seen(&rig.actor, thread);
    assert_eq!(
        owed.as_ref().map(clauseo_protocol::AgentId::as_str),
        Some(OTHER)
    );

    // The next turn carries it: a second handover, logged and delivered.
    one_turn(&mut rig, 13, thread);
    let log = rig.log(thread);
    let logged = handoffs(&log);
    assert_eq!(logged.len(), 2, "{log:?}");
    assert_eq!(
        settled(&log).last(),
        Some(&(logged[1], HandoffStatus::Delivered)),
        "{log:?}"
    );
    for (handoff, _) in settled(&log) {
        assert!(logged.contains(&handoff), "{log:?}");
    }
    assert_eq!(switch::seen(&rig.actor, thread).1, None, "nothing owed now");
    rig.finish();
}

/// [`OTHER`]'s steps: a prompt it takes and never answers, not even after the cancel.
const SILENT_AFTER_PROMPT: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"expect":{"method":"session/cancel","params":{"sessionId":"sess-2"}}}"#,
    "\n",
);

/// The first turn on the new agent is cancelled after its prompt, carrying the handover, went
/// out, and the agent never answers: the cancel's fallback drops the prompt and stops the
/// agent. The handover settles `Unconfirmed`, not `Failed`, and is not owed.
#[test]
fn a_handover_whose_prompt_went_out_before_a_cancel_is_unconfirmed_and_not_owed() {
    let mut rig = two_agents("resume-unconfirmed", Some(SILENT_AFTER_PROMPT));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let sent = rig.dispatch(11, send(thread, Some(OTHER)));
    assert!(sent.is_ok(), "{sent:?}");
    rig.run_until("the prompt carrying the handover is called", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_deref()
            .is_some_and(|current| current.prompt_call.is_some())
    });
    // Its task gets past the recheck and sends it.
    let went = switch::handoff_went(&mut rig.actor, thread).expect("a handover");
    rig.run_tasks_until(
        "the prompt carrying the handover went out",
        futures::future::poll_fn(|cx| {
            if went.get() == switch::Went::Sent {
                std::task::Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        }),
    );
    let cancelled = rig.dispatch(12, HostCommand::Cancel { thread });
    assert!(cancelled.is_ok(), "{cancelled:?}");
    rig.clock.advance(CANCEL_WAIT);
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    let logged = handoffs(&log);
    assert_eq!(logged.len(), 1, "{log:?}");
    assert_eq!(
        settled(&log),
        [(logged[0], HandoffStatus::Unconfirmed)],
        "{log:?}"
    );
    assert_eq!(switch::seen(&rig.actor, thread).1, None, "not owed");
    rig.finish();
}

/// As above, then the next message relaunches [`OTHER`], which cannot load the session that got
/// the unconfirmed handover and starts a new one: the handover is given again, in full.
#[test]
fn an_unconfirmed_handover_goes_again_to_a_new_session() {
    let test = "resume-unconfirmed-new";
    let mut rig = two_agents(test, Some(SILENT_AFTER_PROMPT));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    assert!(rig.dispatch(11, send(thread, Some(OTHER))).is_ok());
    rig.run_until("the handover's prompt is called", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_deref()
            .is_some_and(|current| current.prompt_call.is_some())
    });
    let went = switch::handoff_went(&mut rig.actor, thread).expect("a handover");
    rig.run_tasks_until(
        "the handover's prompt went out",
        futures::future::poll_fn(|cx| {
            if went.get() == switch::Went::Sent {
                std::task::Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        }),
    );
    assert!(rig.dispatch(12, HostCommand::Cancel { thread }).is_ok());
    rig.clock.advance(CANCEL_WAIT);
    until_idle(&mut rig, thread);
    // Its next launch answers; `loadSession` is false, so the session is a new one.
    let other = rig.root().join(format!("{test}-other.jsonl"));
    let handshake = HANDSHAKE.replace(r#"{"sessionId":"sess-1"}"#, r#"{"sessionId":"sess-2"}"#);
    let header = format!(
        "{{\"scenario\":\"{test}-other\",\"description\":\"a unit test\",\
         \"on_stdin_close\":\"exit\"}}\n"
    );
    let text = format!("{header}{handshake}{OTHER_ANSWERS}");
    crate::write::write_file(&other, text.as_bytes()).unwrap();
    one_turn(&mut rig, 13, thread);
    let log = rig.log(thread);
    let logged = handoffs(&log);
    assert_eq!(logged.len(), 2, "given again: {log:?}");
    assert_eq!(
        settled(&log),
        [
            (logged[0], HandoffStatus::Unconfirmed),
            (logged[1], HandoffStatus::Delivered)
        ],
        "{log:?}"
    );
    let resumed = log
        .iter()
        .any(|event| matches!(event, DomainEvent::ContextHandoff(handoff) if handoff.resumed));
    assert!(!resumed, "a new session gets it in full: {log:?}");
    rig.finish();
}

/// A logged handover of `from` to `to`, settled `status`.
fn handed(rig: &mut Rig, from: &str, to: &str, status: HandoffStatus) -> Vec<DomainEvent> {
    let now = rig.actor.now().wall;
    let end = |agent: &str| Endpoint {
        agent: AgentId::new(agent).unwrap(),
        model: None,
        effort: None,
    };
    let handoff: HandoffId = rig.actor.ids.mint(now).unwrap();
    vec![
        ContextHandoff {
            handoff,
            message: rig.actor.ids.mint(now).unwrap(),
            from: end(from),
            to: end(to),
            answers_covered: Count::clamped(1),
            items_included: Count::clamped(2),
            items_left_out: Count::clamped(0),
            resumed: false,
        }
        .into(),
        HandoffSettled { handoff, status }.into(),
    ]
}

/// The handover to the thread's agent failed, then a move to another agent did not fit (its
/// handover logged and failed, the thread kept its agent): start-up reads the last handover to
/// the thread's agent, not the last one, and owes it.
#[test]
fn a_move_too_long_after_a_failed_handover_leaves_that_handover_owed_after_a_restart() {
    let mut rig = two_agents("resume-too-long-owed", None);
    let thread = rig.thread();
    let mut events = handed(&mut rig, OTHER, "fake-agent", HandoffStatus::Failed);
    events.extend(handed(
        &mut rig,
        "fake-agent",
        "third-agent",
        HandoffStatus::Failed,
    ));
    rig.commit(thread, events);
    rig.settle_commits();
    let mut rig = rig.restart(1);
    rig.run_until("start-up's repairs", |actor| {
        actor.work.loads == 0 && actor.work.commits == 0
    });
    let (_, owed) = switch::seen(&rig.actor, thread);
    assert_eq!(owed.as_ref().map(AgentId::as_str), Some(OTHER));
    rig.finish();
}

/// [`OTHER`]'s steps: two prompts in `sess-2`, each answered `end_turn`.
const TWO_PROMPTS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
    r#"{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-2"}}}"#,
    "\n",
    r#"{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// Runs `thread` until no turn runs or closes, its queue left as it is.
fn until_no_turn(rig: &mut Rig, thread: clauseo_protocol::ThreadId) {
    rig.run_until("no turn runs", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.closing.is_none()
    });
    rig.settle_commits();
}

/// The texts of the lawyer's messages in the order their turns started.
fn started_texts(log: &[DomainEvent]) -> Vec<String> {
    let mut texts = std::collections::BTreeMap::new();
    let mut started = Vec::new();
    for event in log {
        if let DomainEvent::UserMessage(message) = event
            && let Some(clauseo_protocol::ContentBlock::Text { text }) = message.content.first()
        {
            texts.insert(message.turn, text.clone());
        } else if let DomainEvent::TurnStarted(turn) = event {
            started.extend(texts.get(&turn.turn).cloned());
        }
    }
    started
}

/// The thread's agent is held for its sign-in, and a message naming it waits in the queue. A
/// message to another agent passes the hold and switches; the queue moves on after it, and the
/// message naming the old agent, still held for its sign-in, goes back to the queue held,
/// rather than failing with the sign-in's code. Once the hold lifts it runs, switching back.
#[test]
fn a_message_naming_the_old_agent_stays_held_after_a_switch_passed_its_sign_in() {
    let mut rig = two_agents("resume-named-old-held", Some(TWO_PROMPTS));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let fake = AgentId::new("fake-agent").unwrap();
    let hold = crate::agent::login_link::StopFor::SignInAgain;
    rig.actor.wiring.sign_in.held.insert(fake.clone(), hold);
    let sent = rig.dispatch(
        11,
        send_text(thread, Some("fake-agent"), "Claude, clause 6"),
    );
    assert!(sent.is_ok(), "{sent:?}");
    assert_eq!(queue::queued_turns(&rig.actor, thread).len(), 1, "held");
    let sent = rig.dispatch(12, send_text(thread, Some(OTHER), "ChatGPT, review"));
    assert!(sent.is_ok(), "{sent:?}");
    until_no_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::TurnFailed(_))),
        0,
        "{log:?}"
    );
    assert_eq!(
        queue::queued_turns(&rig.actor, thread).len(),
        1,
        "the message naming the old agent waits: {log:?}"
    );
    assert!(queue::queue_held(&rig.actor, thread), "held");
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    rig.actor.wiring.sign_in.held.remove(&fake);
    let resumed = rig.dispatch(13, HostCommand::ResumeQueue { thread });
    assert!(resumed.is_ok(), "{resumed:?}");
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::TurnFailed(_))),
        0,
        "{log:?}"
    );
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::AgentSwitched(_))),
        2,
        "away and back: {log:?}"
    );
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), "fake-agent");
    rig.finish();
}

/// As above, and the old agent's sign-in restarts (`restart_agents`) with no press of Resume:
/// the thread is on the other agent by then, but its held head names the old one, so the
/// restart moves it on and the message switches back.
#[test]
fn a_held_switch_back_runs_when_the_old_agents_sign_in_restarts() {
    let mut rig = two_agents("resume-named-old-restart", Some(TWO_PROMPTS));
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    let fake = AgentId::new("fake-agent").unwrap();
    let hold = crate::agent::login_link::StopFor::SignInAgain;
    rig.actor.wiring.sign_in.held.insert(fake.clone(), hold);
    let back = send_text(thread, Some("fake-agent"), "Claude, clause 6");
    assert!(rig.dispatch(11, back).is_ok());
    let away = send_text(thread, Some(OTHER), "ChatGPT, review");
    assert!(rig.dispatch(12, away).is_ok());
    until_no_turn(&mut rig, thread);
    assert!(queue::queue_held(&rig.actor, thread), "held");
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), OTHER);
    let (reply, _answer) = futures::channel::oneshot::channel();
    let restart = crate::actor::input::SignInRequest::Restart { agent: fake, reply };
    rig.actor
        .on_request(crate::actor::input::Request::SignIn(restart));
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::AgentSwitched(_))),
        2,
        "away and back: {log:?}"
    );
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), "fake-agent");
    rig.finish();
}

/// The queue is held because the last turn stopped at the agent's limit, with a message to
/// another agent queued in it: a second message to another agent does not start ahead of it,
/// so the two moves run in the order the lawyer sent them.
#[test]
fn a_switch_does_not_pass_a_held_queue_ahead_of_an_earlier_switch() {
    let mut rig = two_agents("resume-switch-order", Some(TWO_PROMPTS));
    let thread = rig.thread();
    let reply = |_: &crate::actor::Actor, input: &crate::actor::input::Input| {
        matches!(
            input,
            crate::actor::input::Input::Turn(crate::actor::turn::TurnInput::Agent {
                reply: crate::actor::turn::AgentReply::Prompt { .. },
                ..
            })
        )
    };
    let sent = rig.dispatch_holding(10, send(thread, None), reply);
    assert!(sent.is_ok(), "{sent:?}");
    if rig.held.is_empty() {
        rig.hold_next("the first answer", reply);
    }
    let sent = rig.dispatch(11, send_text(thread, Some(OTHER), "first move"));
    assert!(sent.is_ok(), "{sent:?}");
    queue::hold_for_limit(&mut rig.actor, thread);
    rig.release_held();
    until_no_turn(&mut rig, thread);
    assert_eq!(queue::queued_turns(&rig.actor, thread).len(), 1, "held");
    let sent = rig.dispatch(12, send_text(thread, Some(OTHER), "second move"));
    assert!(sent.is_ok(), "{sent:?}");
    until_idle(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        started_texts(&log),
        ["And clause 5?", "first move", "second move"],
        "{log:?}"
    );
    rig.finish();
}
