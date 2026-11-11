//! Stop on a turn the agent started on its own, in each phase before and after it is live: the
//! agent gets `session/cancel` (the scenario sends its late text only once it has), the output
//! waiting for the turn is dropped, the turn ends `Cancelled`, and the agent's late output opens
//! no turn of its own until it has been quiet for the late window's [`LATE_QUIET`].
//!
//! An agent-started turn never waits in `Launching` itself: its agent being attached keeps the
//! thread's stream loaded, so step 3 follows at once. Before its checkpoint it waits only for the
//! matter's writer (`Queued`), which takes the same cancel path as `Launching`.

use clauseo_acp::{AcpEvent, Origin};
use clauseo_protocol::{CommandEnvelope, DomainEvent, HostCommand, StopReason, ThreadId};

use super::agents::send_prompt;
use super::rig::{Rig, command_id, kind};
use crate::actor::Actor;
use crate::actor::input::{Input, Request};
use crate::actor::turn::agents::Stage;
use crate::actor::turn::own_turn::{self, LATE_QUIET};
use crate::actor::turn::{Holder, TurnPhase};
use crate::agent::AgentInput;
use crate::matter_worker::MatterReply;

/// Prompt 1 ends; the agent starts a turn of its own (a tool call, then text) and, once it has
/// heard `session/cancel`, sends late text.
const OWN_THEN_LATE: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"tc-own-1","title":"Read build.log","kind":"read","status":"completed"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-own-1","content":{"type":"text","text":"The build finished."}}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-own-1","content":{"type":"text","text":" Late words."}}}}
"#;

/// Where the agent-started turn is when the lawyer presses Stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum At {
    /// Waiting for the matter's writer, before its checkpoint.
    Writer,
    /// Its before-turn checkpoint is being made.
    Checkpointing,
    /// Its `TurnStarted` commit is in flight.
    Starting,
    /// Live.
    Prompting,
}

fn idle_update(_: &Actor, input: &Input) -> bool {
    let Input::Agent {
        input: AgentInput::Event(event),
        ..
    } = input
    else {
        return false;
    };
    matches!(
        **event,
        AcpEvent::Update {
            origin: Origin::Idle,
            ..
        }
    )
}

fn phase(rig: &Rig, thread: ThreadId) -> Option<TurnPhase> {
    let state = &rig.actor.threads[&thread].turn;
    state.current.as_ref().map(|current| current.phase)
}

/// Whether `thread` has no turn, current or closing, and nothing in flight.
fn no_turn(actor: &Actor, thread: ThreadId) -> bool {
    let state = &actor.threads[&thread].turn;
    state.current.is_none()
        && state.closing.is_none()
        && actor.work.commits == 0
        && actor.work.matter_jobs == 0
}

/// Waits until the agent-started turn of `thread` is where `at` says, holding back the input that
/// would move it on.
fn put_at(rig: &mut Rig, thread: ThreadId, at: At) {
    match at {
        At::Writer => rig.run_until("the turn waiting for the writer", |actor| {
            let current = actor.threads[&thread].turn.current.as_ref();
            current.is_some_and(|c| c.phase == TurnPhase::Queued) && actor.work.commits == 0
        }),
        At::Checkpointing => rig.hold_next("the before-turn checkpoint", |_, input| {
            matches!(
                input,
                Input::Matter {
                    reply: MatterReply::BeforeTurn(_),
                    ..
                }
            )
        }),
        At::Starting => rig.hold_next("the TurnStarted commit", |actor, input| {
            matches!(input, Input::Committed { ticket, .. }
                if actor.pipeline.events(*ticket).iter()
                    .any(|e| matches!(e.event, DomainEvent::TurnStarted(_))))
        }),
        At::Prompting => rig.run_until("the live turn", |actor| {
            let current = actor.threads[&thread].turn.current.as_ref();
            current.is_some_and(|c| c.live() && c.own.is_some()) && actor.work.commits == 0
        }),
    }
    let expected = match at {
        At::Writer => TurnPhase::Queued,
        At::Checkpointing => TurnPhase::Checkpointing,
        At::Starting => TurnPhase::Starting,
        At::Prompting => TurnPhase::Prompting,
    };
    assert_eq!(phase(rig, thread), Some(expected));
    assert!(
        rig.actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .unwrap()
            .own
            .is_some()
    );
}

/// The thread's log once the agent-started turn ended: prompt 1 ended `EndTurn`, the agent's
/// turn `Cancelled` (with its `TurnStarted` only if it got that far), and nothing else started.
fn assert_ended(rig: &mut Rig, thread: ThreadId, at: At) -> Vec<DomainEvent> {
    let log = rig.log(thread);
    let kinds = || log.iter().map(kind).collect::<Vec<_>>();
    let ends: Vec<StopReason> = log
        .iter()
        .filter_map(|event| {
            if let DomainEvent::TurnEnded(ended) = event {
                Some(ended.stop)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        ends,
        [StopReason::EndTurn, StopReason::Cancelled],
        "{:?}",
        kinds()
    );
    let starts = log
        .iter()
        .filter(|event| matches!(event, DomainEvent::TurnStarted(_)))
        .count();
    let live = matches!(at, At::Starting | At::Prompting);
    assert_eq!(starts, 1 + usize::from(live), "{:?}", kinds());

    log
}

fn stop_at(test: &str, at: At) {
    let mut rig = Rig::start(test, OWN_THEN_LATE);
    let thread = rig.thread();
    let other = rig.thread_by(2);
    send_prompt(&mut rig, 10, thread);
    // The agent's own output waits until prompt 1 has ended.
    rig.run_holding("prompt 1's end", idle_update, |actor| {
        let attached = actor.threads[&thread]
            .turn
            .agent
            .as_deref()
            .is_some_and(|agent| agent.stage == Stage::Attached);
        attached && no_turn(actor, thread)
    });
    if rig.held.is_empty() {
        rig.hold_next("the agent's own output", idle_update);
    }
    if at == At::Writer {
        let matter = rig.matter();
        rig.actor.matters.get_mut(&matter).unwrap().turns.holder = Some(Holder::Thread(other));
    }
    rig.release_held();
    put_at(&mut rig, thread, at);
    let waiting = own_turn::waiting(&mut rig.actor, thread);
    if at != At::Prompting {
        assert!(waiting > 0, "the agent's output waits for the turn");
    }

    // Not waited for: its receipt goes out behind a commit the test may hold.
    let (reply, mut answer) = futures::channel::oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(11),
        command: HostCommand::Cancel { thread },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.run_until("the window opened", |actor| {
        own_turn::late_dropped(actor, thread).is_some()
    });
    assert_eq!(own_turn::waiting(&mut rig.actor, thread), 0, "dropped");
    rig.release_held();
    // The late text comes only once the agent heard `session/cancel`; it opens no turn.
    rig.run_until("the turn's end and the late text", |actor| {
        let dropped = own_turn::late_dropped(actor, thread).unwrap_or(0);
        no_turn(actor, thread) && dropped > waiting
    });
    assert!(
        matches!(answer.try_recv(), Ok(Some(Ok(_)))),
        "the Cancel is done"
    );
    let log = assert_ended(&mut rig, thread, at);
    // Quiet for the window's length: it closes, and still nothing opens.
    rig.clock.advance(LATE_QUIET);
    rig.run_until("the window's close", |actor| {
        own_turn::late_dropped(actor, thread).is_none()
    });
    rig.settle_commits();
    let state = &rig.actor.threads[&thread].turn;
    assert!(
        no_turn(&rig.actor, thread),
        "{:?} {:?} {} {}",
        state.current.as_ref().map(|c| (c.phase, c.own.is_some())),
        state.closing.is_some(),
        rig.actor.work.commits,
        rig.actor.work.matter_jobs,
    );
    let after = rig.log(thread);
    let new: Vec<_> = after[log.len()..].iter().map(kind).collect();
    assert!(new.is_empty(), "nothing logged since: {new:?}");
    rig.finish();
}

#[test]
fn stop_while_an_agent_started_turn_waits_for_the_writer() {
    stop_at("own-stop-writer", At::Writer);
}

#[test]
fn stop_while_an_agent_started_turn_checkpoints() {
    stop_at("own-stop-checkpointing", At::Checkpointing);
}

#[test]
fn stop_while_an_agent_started_turn_starts() {
    stop_at("own-stop-starting", At::Starting);
}

#[test]
fn stop_of_a_live_agent_started_turn() {
    stop_at("own-stop-prompting", At::Prompting);
}
