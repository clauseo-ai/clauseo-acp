//! The idle stop with a real `fake_agent` (R20b): an agent with nothing pending stops at the
//! limit, not a millisecond before, with the shutdown ladder (stdin close, or SIGTERM for an
//! agent that ignores it); its session detaches `IdleStopped`; no timer is left while no agent
//! is live; and a deadline that fires just after a prompt was handled stops nothing.

use std::time::Duration;

use clauseo_protocol::{
    CommandEnvelope, DeliveryRequest, DetachReason, DomainEvent, HostCommand, PromptBlock,
    StreamId, ThreadId, ThreadStatus, TurnId,
};
use futures::channel::oneshot;

use super::agents::send_prompt;
use super::rig::{Rig, command_id};
use crate::actor::Actor;
use crate::actor::input::{Input, Request};
use crate::actor::turn::agents::Stage;
use crate::actor::turn::idle::{Pending, pending, pending_in};
use crate::agent::{AgentInput, Ended, StopStep};
use crate::config::{HostConfig, IDLE_LIMIT};
use crate::pipeline::tests::support::TempDir;

/// One prompt answered, then a second one answered.
pub(super) const TWO_ANSWERS: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-1","content":{"type":"text","text":"First."}}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-2","content":{"type":"text","text":"Second."}}}}
{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}
"#;

/// The step one millisecond makes.
pub(super) const MS: Duration = Duration::from_millis(1);

/// A rig with the idle stop at its default limit and no cap; the agent does `on_stdin_close`.
pub(super) fn rig(test: &str, steps: &str, on_stdin_close: &str) -> Rig {
    start_idle(test, steps, on_stdin_close, |config| {
        config
            .with_idle_limit(IDLE_LIMIT)
            .with_live_agents(usize::MAX)
    })
}

/// [`Rig::start`] with the idle stop on: `adjust` sets the idle limit, the live-agents cap and
/// the cap's minimum idle in the host config, and the agent does `on_stdin_close` (`"exit"` or
/// `"hang"`) when its stdin closes.
pub(super) fn start_idle(
    test: &str,
    steps: &str,
    on_stdin_close: &str,
    adjust: impl FnOnce(HostConfig) -> HostConfig,
) -> Rig {
    let dir = TempDir::new(test);
    Rig::prepare(dir.path(), test, steps, None);
    let scenario = dir.path().join(format!("{test}.jsonl"));
    let text = std::fs::read_to_string(&scenario).unwrap();
    let text = text.replacen(
        r#""on_stdin_close":"exit""#,
        &format!(r#""on_stdin_close":"{on_stdin_close}""#),
        1,
    );
    crate::write::write_file(&scenario, text.as_bytes()).unwrap();
    Rig::open_with(dir, 0, None, adjust)
}

/// When `thread`'s agent's idle clock started, if it has one.
pub(super) fn idle_since(actor: &Actor, thread: ThreadId) -> Option<Duration> {
    actor.threads[&thread].turn.agent.as_deref()?.idle.since
}

/// Whether `thread`'s agent is live and not being stopped.
pub(super) fn live(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .is_some_and(|agent| agent.stage == Stage::Attached)
}

/// What keeps `thread`'s agent from idling.
pub(super) fn pending_of(actor: &Actor, thread: ThreadId) -> Option<Pending> {
    pending(&actor.threads[&thread].turn)
}

/// Handles inputs until `thread`'s agent is idle: its clock started and no commit in flight.
pub(super) fn until_idle(rig: &mut Rig, thread: ThreadId) -> Duration {
    rig.run_until("the agent idle", |actor| {
        actor.work.commits == 0
            && actor.work.matter_jobs == 0
            && idle_since(actor, thread).is_some()
    });
    idle_since(&rig.actor, thread).unwrap()
}

/// Moves the clock by `by` and handles what that posts, waiting for nothing else.
pub(super) fn advance(rig: &mut Rig, by: Duration) {
    rig.clock.advance(by);
    rig.drain();
}

/// Handles inputs until `thread`'s agent has ended and nothing is in flight.
pub(super) fn until_ended(rig: &mut Rig, thread: ThreadId) {
    rig.run_until("the agent's end", |actor| {
        actor.threads[&thread].turn.agent.is_none() && actor.work.commits == 0
    });
}

/// How many `AgentSessionDetached { IdleStopped }` `thread`'s log holds.
pub(super) fn idle_stops(rig: &mut Rig, thread: ThreadId) -> usize {
    rig.log(thread)
        .iter()
        .filter(|event| {
            matches!(event, DomainEvent::AgentSessionDetached(detached)
                if detached.reason == DetachReason::IdleStopped)
        })
        .count()
}

/// `thread`'s agent is idle now, its clock started at this very time: it is not stopped one
/// millisecond before the limit, and is stopped at it with `IdleStopped`.
pub(super) fn stops_after_a_full_limit(rig: &mut Rig, thread: ThreadId) {
    let since = until_idle(rig, thread);
    assert_eq!(
        since,
        rig.actor.now().mono,
        "the clock starts when the item ends"
    );
    assert_eq!(
        rig.actor.turn_path.idle.armed_at(),
        Some(since + IDLE_LIMIT)
    );
    advance(rig, IDLE_LIMIT.checked_sub(MS).unwrap());
    assert!(live(&rig.actor, thread), "stopped before the limit");
    advance(rig, MS);
    until_ended(rig, thread);
    assert_eq!(idle_stops(rig, thread), 1);
}

/// The thread's status as the rail reads it: the loaded state's header, else the one kept.
pub(super) fn status(actor: &Actor, thread: ThreadId) -> Option<ThreadStatus> {
    let loaded = actor.loaded(StreamId::Thread(thread));
    loaded
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(actor.threads[&thread].header.as_ref())
        .map(|header| header.status)
}

/// Holds back `thread`'s agent's `Ended` and gives the ladder step that ended it.
fn held_step(rig: &mut Rig) -> StopStep {
    let ended = |_: &Actor, input: &Input| {
        matches!(
            input,
            Input::Agent {
                input: AgentInput::Ended(_),
                ..
            }
        )
    };
    rig.hold_next("the agent's end", ended);
    match rig.held.last() {
        Some(Input::Agent {
            input: AgentInput::Ended(Ended::Stopped { step, .. }),
            ..
        }) => *step,
        other => panic!("not a stop: {other:?}"),
    }
}

#[test]
fn an_idle_agent_stops_at_the_limit_on_stdin_close_and_shows_idle_stopped() {
    let mut rig = rig("idle-stop-stdin", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    assert_eq!(
        rig.actor.turn_path.idle.armed_at(),
        None,
        "no timer with no agent"
    );
    send_prompt(&mut rig, 10, thread);
    let since = until_idle(&mut rig, thread);
    assert_eq!(pending_of(&rig.actor, thread), None);
    let pid = rig.actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .unwrap()
        .link
        .pid();
    assert_eq!(
        rig.actor.turn_path.idle.armed_at(),
        Some(since + IDLE_LIMIT)
    );

    advance(&mut rig, IDLE_LIMIT.checked_sub(MS).unwrap());
    assert!(live(&rig.actor, thread), "stopped before the limit");
    advance(&mut rig, MS);
    // The first step of the ladder ended it: the agent exits when its stdin closes.
    assert_eq!(held_step(&mut rig), StopStep::StdinClosed);
    rig.release_held();
    until_ended(&mut rig, thread);

    assert!(!clauseo_process::running(pid), "agent {pid} still runs");
    assert_eq!(idle_stops(&mut rig, thread), 1);
    assert_eq!(status(&rig.actor, thread), Some(ThreadStatus::IdleStopped));
    assert_eq!(
        rig.actor.turn_path.idle.armed_at(),
        None,
        "no timer with no agent"
    );
    rig.finish();
}

#[cfg(unix)]
#[test]
fn an_agent_that_ignores_stdin_close_is_idle_stopped_by_sigterm() {
    let mut rig = rig("idle-stop-sigterm", TWO_ANSWERS, "hang");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);
    let pid = rig.actor.threads[&thread]
        .turn
        .agent
        .as_deref()
        .unwrap()
        .link
        .pid();

    advance(&mut rig, IDLE_LIMIT);
    assert_eq!(
        rig.actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .map(|agent| agent.stage),
        Some(Stage::Stopping)
    );
    // Stdin is closed and the agent keeps running: the ladder's 2 s wait, then SIGTERM.
    let clock = rig.clock.clone();
    rig.run_tasks_until("the ladder's stdin wait", async {
        while clock.next_armed().await != Duration::from_secs(2) {}
    });
    rig.clock.advance(Duration::from_secs(2));
    assert_eq!(held_step(&mut rig), StopStep::Terminated);
    rig.release_held();
    until_ended(&mut rig, thread);

    assert!(!clauseo_process::running(pid), "agent {pid} still runs");
    assert_eq!(idle_stops(&mut rig, thread), 1);
    rig.finish();
}

/// The race: the deadline has fired and its input waits behind a `SendPrompt` handled in the
/// same tick. The stop looks again when its input is handled, finds the turn and stops nothing;
/// the clock starts again when that turn ends.
#[test]
fn a_deadline_that_fires_as_a_prompt_arrives_stops_nothing() {
    let mut rig = rig("idle-stop-race", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_idle(&mut rig, thread);

    // The deadline fires: its input is posted, then held back.
    rig.clock.advance(IDLE_LIMIT);
    rig.run_tasks_until_stalled();
    let due = |_: &Actor, input: &Input| matches!(input, Input::IdleDeadline { .. });
    rig.hold_next("the idle deadline", due);
    assert!(live(&rig.actor, thread));

    // The prompt is handled first, then the deadline.
    let (reply, _answer) = oneshot::channel();
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "One more thing.".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
    };
    let command = CommandEnvelope {
        command_id: command_id(11),
        command,
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.release_held();
    assert!(
        live(&rig.actor, thread),
        "the deadline stopped an agent given a prompt"
    );
    assert!(pending_in(&rig.actor, thread).is_some());
    assert_eq!(idle_since(&rig.actor, thread), None);
    // The prompt runs, then its turn ends.
    rig.run_until("the prompt accepted", |actor| {
        actor.threads[&thread].turn.current.is_some()
    });

    stops_after_a_full_limit(&mut rig, thread);
    rig.finish();
}

/// The agent stopped idle relaunches for the next prompt, which runs to its end.
#[test]
fn the_next_prompt_after_an_idle_stop_relaunches_the_agent() {
    let mut rig = rig("idle-stop-relaunch", TWO_ANSWERS, "exit");
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    stops_after_a_full_limit(&mut rig, thread);
    let turn: TurnId = send_prompt(&mut rig, 11, thread);
    rig.run_until("the second turn's end", |actor| {
        actor.threads[&thread].turn.current.is_none() && actor.work.commits == 0
    });
    let ended = rig.log(thread).iter().any(|event| {
        matches!(event, DomainEvent::TurnEnded(ended)
            if ended.turn == turn && ended.stop == clauseo_protocol::StopReason::EndTurn)
    });
    assert!(ended);
    rig.finish();
}
