//! A switch of the thread's agent while that agent may still work in the background
//! (`switch.rs`): a message naming another agent is refused with `thread.background_work` before
//! its `UserMessage` is logged, and a queued one that meets such work when its turn starts goes
//! back to the held queue unchanged, to switch once the lawyer resumes it.

use clauseo_protocol::{
    AgentId, CommandOutcome, DeliveryRequest, DomainEvent, ErrorCode, HandoffStatus, HostCommand,
    PromptBlock, ThreadId,
};

use super::rig::Rig;
use crate::actor::turn::{TurnPhase, queue, steps, switch};
use crate::pipeline::tests::support::TempDir;

/// The agent's steps after its handshake: one prompt, answered `end_turn`.
const PROMPT_ENDS: &str = concat!(
    r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}"#,
    "\n",
    r#"{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}"#,
    "\n",
);

/// The agent a message switches to, in the settings beside `fake-agent`.
const OTHER: &str = "other-agent";

/// A rig whose settings hold `fake-agent` and [`OTHER`], both playing [`PROMPT_ENDS`].
fn rig(test: &str) -> Rig {
    let dir = TempDir::new(test);
    Rig::prepare(dir.path(), test, PROMPT_ENDS, None);
    let path = dir.path().join("data").join("settings.json");
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    settings["agents"][OTHER] = settings["agents"]["fake-agent"].clone();
    crate::write::write_file(&path, settings.to_string().as_bytes()).unwrap();
    Rig::open(dir, 0, None)
}

fn send(thread: ThreadId, agent: Option<&str>) -> HostCommand {
    HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "And clause 5?".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
        config: None,
        agent: agent.map(|agent| AgentId::new(agent).unwrap()),
    }
}

/// Runs one turn of `thread` to its end, as command `n`.
fn one_turn(rig: &mut Rig, n: u128, thread: ThreadId) {
    let sent = rig.dispatch(n, send(thread, None));
    assert!(
        matches!(sent, Ok(CommandOutcome::PromptAccepted { .. })),
        "{sent:?}"
    );
    until_idle(rig, thread);
}

fn until_idle(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the turn ended", |actor| {
        let state = &actor.threads[&thread].turn;
        state.current.is_none() && state.closing.is_none()
    });
    rig.settle_commits();
}

/// Marks `thread`'s live agent as one whose last turn closed after a steer during a tool call,
/// now: a command it put in the background may still run (`idle::background_may_run`).
fn background_now(rig: &mut Rig, thread: ThreadId) {
    let now = rig.actor.now().mono;
    let state = &mut rig.actor.threads.get_mut(&thread).unwrap().turn;
    state
        .agent
        .as_deref_mut()
        .expect("a live agent")
        .idle
        .steered_end = Some(now);
}

fn count(log: &[DomainEvent], pick: impl Fn(&DomainEvent) -> bool) -> usize {
    log.iter().filter(|event| pick(event)).count()
}

#[test]
fn background_work_refuses_a_switch_before_its_message_is_logged() {
    let mut rig = rig("switch-background");
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    background_now(&mut rig, thread);
    let refused = rig.dispatch(11, send(thread, Some(OTHER))).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::ThreadBackgroundWork);
    // An agent the settings do not have is refused as `CreateThread` refuses it.
    let unknown = rig.dispatch(12, send(thread, Some("nobody"))).unwrap_err();
    assert_eq!(unknown.code(), ErrorCode::CommandInvalid);
    rig.settle_commits();
    let log = rig.log(thread);
    let messages = count(&log, |event| matches!(event, DomainEvent::UserMessage(_)));
    assert_eq!(messages, 1, "only the first turn's message: {log:?}");
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::AgentSwitched(_))),
        0
    );
    // A message to the thread's own agent is no switch, and goes ahead.
    one_turn(&mut rig, 13, thread);
    rig.finish();
}

#[test]
fn a_queued_switch_meeting_background_work_waits_held_then_switches() {
    let mut rig = rig("switch-held");
    let thread = rig.thread();
    one_turn(&mut rig, 10, thread);
    // A queued message naming the other agent reaches step 2 while background work may run.
    let turn = rig.current_turn(thread, TurnPhase::Accepting);
    let other = AgentId::new(OTHER).unwrap();
    if let Some(current) = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .current
        .as_deref_mut()
    {
        current.switch = Some(switch::Switch::new(other.clone()));
    }
    background_now(&mut rig, thread);
    steps::on_accepted(&mut rig.actor, thread, turn);
    assert!(rig.actor.threads[&thread].turn.current.is_none());
    assert_eq!(queue::queued_turns(&rig.actor, thread), [turn]);
    assert!(
        queue::queue_held(&rig.actor, thread),
        "back in the held queue"
    );
    assert_eq!(rig.actor.threads[&thread].row.agent.as_str(), "fake-agent");

    // The work is over and the lawyer resumes the queue: the turn switches the thread.
    let state = &mut rig.actor.threads.get_mut(&thread).unwrap().turn;
    state.agent.as_deref_mut().unwrap().idle.steered_end = None;
    let resumed = rig.dispatch(20, HostCommand::ResumeQueue { thread });
    assert!(resumed.is_ok(), "{resumed:?}");
    until_idle(&mut rig, thread);
    assert_eq!(rig.actor.threads[&thread].row.agent, other);
    let log = rig.log(thread);
    assert_eq!(
        count(&log, |e| matches!(e, DomainEvent::AgentSwitched(_))),
        1
    );
    let delivered = count(&log, |event| {
        matches!(event, DomainEvent::HandoffSettled(settled)
            if settled.status == HandoffStatus::Delivered)
    });
    assert_eq!(delivered, 1, "{log:?}");
    rig.finish();
}
