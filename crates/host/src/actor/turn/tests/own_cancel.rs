//! Stop on a turn the agent started on its own, in each phase before and after it is live: the
//! agent gets `session/cancel` (the scenario sends its late text only once it has), the output
//! waiting for the turn is dropped, the turn ends `Cancelled`, and the agent's late output opens
//! no turn of its own until it has been quiet for the late window's [`LATE_QUIET`].
//!
//! An agent-started turn never waits in `Launching` itself: its agent being attached keeps the
//! thread's stream loaded, so step 3 follows at once. Before its checkpoint it waits only for the
//! matter's writer (`Queued`), which takes the same cancel path as `Launching`.

use clauseo_acp::{AcpEvent, Origin};
use clauseo_protocol::{
    CommandEnvelope, CommandOutcome, DeliveryRequest, DomainEvent, HostCommand, PromptBlock,
    StopReason, ThreadId, TurnId,
};

use super::agents::send_prompt;
use super::rig::{Rig, command_id, kind};
use crate::actor::Actor;
use crate::actor::input::{Input, Request};
use crate::actor::turn::agents::Stage;
use crate::actor::turn::own_turn::{self, LATE_QUIET};
use crate::actor::turn::{Holder, TurnPhase};
use crate::agent::AgentInput;
use crate::error::Error;
use crate::matter_worker::MatterReply;

/// The answer to a command dispatched without waiting for it.
type Answer = futures::channel::oneshot::Receiver<Result<CommandOutcome, Error>>;

/// Prompt 1 ends; the agent starts a turn of its own (a tool call, then text) and, once it has
/// heard `session/cancel`, sends late text.
const OWN_THEN_LATE: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"tc-own-1","title":"Read build.log","kind":"read","status":"completed"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-own-1","content":{"type":"text","text":"The build finished."}}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-own-1","content":{"type":"text","text":" Late words."}}}}
"#;

/// [`OWN_THEN_LATE`], then two prompts of the lawyer's, each ended `end_turn`.
const OWN_LATE_THEN_TWO: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"update":{"update":{"sessionUpdate":"tool_call","toolCallId":"tc-own-1","title":"Read build.log","kind":"read","status":"completed"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-own-1","content":{"type":"text","text":"The build finished."}}}}
{"expect":{"method":"session/cancel","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-own-1","content":{"type":"text","text":" Late words."}}}}
{"expect":{"method":"session/prompt","as":"p2","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p2","result":{"stopReason":"end_turn"}}}
{"expect":{"method":"session/prompt","as":"p3","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p3","result":{"stopReason":"end_turn"}}}
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

/// Runs prompt 1 of `thread` and puts the agent-started turn after it where `at` says; how many
/// of its updates wait for the turn then.
fn own_turn_at(rig: &mut Rig, thread: ThreadId, at: At) -> usize {
    let other = rig.thread_by(2);
    send_prompt(rig, 10, thread);
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
    put_at(rig, thread, at);
    let waiting = own_turn::waiting(&mut rig.actor, thread);
    if at != At::Prompting {
        assert!(waiting > 0, "the agent's output waits for the turn");
    }
    waiting
}

/// Presses Stop on `thread` as command 11 and waits for the late window to open. Not waited
/// for: its receipt goes out behind a commit the test may hold.
fn press_stop(rig: &mut Rig, thread: ThreadId) -> Answer {
    let (reply, answer) = futures::channel::oneshot::channel();
    let command = CommandEnvelope {
        command_id: command_id(11),
        command: HostCommand::Cancel { thread },
    };
    rig.actor.on_request(Request::Dispatch { command, reply });
    rig.run_until("the window opened", |actor| {
        own_turn::late_dropped(actor, thread).is_some()
    });
    assert_eq!(own_turn::waiting(&mut rig.actor, thread), 0, "dropped");
    answer
}

fn stop_at(test: &str, at: At) {
    let mut rig = Rig::start(test, OWN_THEN_LATE);
    let thread = rig.thread();
    let waiting = own_turn_at(&mut rig, thread, at);
    let mut answer = press_stop(&mut rig, thread);
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

/// Sends a prompt to `thread` as command `n`, delivered as `delivery`.
fn send(rig: &mut Rig, n: u128, thread: ThreadId, delivery: DeliveryRequest) -> TurnId {
    let command = HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Read clause 9.".to_owned(),
        }],
        delivery,
    };
    match rig.dispatch(n, command) {
        Ok(CommandOutcome::PromptAccepted { turn, .. }) => turn,
        other => panic!("not accepted: {other:?}"),
    }
}

/// Stops the live agent-started turn of `thread` (sending `before` first, if any, as a prompt
/// queued behind it) and waits for its end and the late text, the window still holding starts.
fn stop_live(rig: &mut Rig, thread: ThreadId, before: bool) -> Option<TurnId> {
    let waiting = own_turn_at(rig, thread, At::Prompting);
    let queued = before.then(|| send(rig, 12, thread, DeliveryRequest::Queue));
    let _answer = press_stop(rig, thread);
    rig.run_until("the turn's end and the late text", |actor| {
        let dropped = own_turn::late_dropped(actor, thread).unwrap_or(0);
        no_turn(actor, thread) && dropped > waiting
    });
    assert!(own_turn::holds_starts(&mut rig.actor, thread));
    queued
}

/// The stops of the turns in `thread`'s log that ended, by turn.
fn ends(rig: &mut Rig, thread: ThreadId) -> Vec<(TurnId, StopReason)> {
    rig.log(thread)
        .iter()
        .filter_map(|event| {
            if let DomainEvent::TurnEnded(ended) = event {
                Some((ended.turn, ended.stop))
            } else {
                None
            }
        })
        .collect()
}

/// Waits until `turn` of `thread` has started and ended.
fn until_ran(rig: &mut Rig, thread: ThreadId, turn: TurnId) {
    rig.run_until("the prompt's start", |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current.is_some_and(|current| current.turn == turn && current.live())
    });
    rig.run_until("the prompt's end", |actor| {
        let current = actor.threads[&thread].turn.current.as_ref();
        current.is_none_or(|current| current.turn != turn)
    });
}

/// A prompt the lawyer queues while the window holds starts, the queue empty, starts once the
/// window lets it, with no Resume: it un-holds the queue as a send with nothing running does.
#[test]
fn a_prompt_sent_in_the_late_window_starts_when_it_closes() {
    let mut rig = Rig::start("own-late-send", OWN_LATE_THEN_TWO);
    let thread = rig.thread();
    stop_live(&mut rig, thread, false);
    let turn = send(&mut rig, 13, thread, DeliveryRequest::Queue);
    assert!(no_turn(&rig.actor, thread), "the window holds it");
    rig.clock.advance(LATE_QUIET);
    until_ran(&mut rig, thread, turn);
    rig.run_until("its end", |actor| no_turn(actor, thread));
    assert_eq!(
        ends(&mut rig, thread).last(),
        Some(&(turn, StopReason::EndTurn))
    );
    rig.finish();
}

/// A prompt queued behind the agent-started turn is held by the Stop, as after any stop: with
/// no new send from the lawyer, it does not start when the window closes.
#[test]
fn a_queue_the_stop_held_stays_held_after_the_late_window() {
    let mut rig = Rig::start("own-late-held", OWN_THEN_LATE);
    let thread = rig.thread();
    stop_live(&mut rig, thread, true);
    rig.clock.advance(LATE_QUIET);
    rig.run_until("the window's close", |actor| {
        own_turn::late_dropped(actor, thread).is_none()
    });
    rig.settle_commits();
    assert!(no_turn(&rig.actor, thread), "the queue stays held");
    let starts = rig
        .log(thread)
        .iter()
        .filter(|event| matches!(event, DomainEvent::TurnStarted(_)))
        .count();
    assert_eq!(starts, 2, "prompt 1's and the agent's own");
    rig.finish();
}

/// A prompt sent now in the window un-holds a queue the Stop held, as a send now with nothing
/// running does: it starts once the window lets it, and the queue moves on after it.
#[test]
fn a_send_now_in_the_late_window_moves_a_held_queue_on_after_it() {
    let mut rig = Rig::start("own-late-send-now", OWN_LATE_THEN_TWO);
    let thread = rig.thread();
    let queued = stop_live(&mut rig, thread, true).unwrap();
    let now = send(&mut rig, 13, thread, DeliveryRequest::SendNow);
    assert!(no_turn(&rig.actor, thread), "the window holds it");
    rig.clock.advance(LATE_QUIET);
    until_ran(&mut rig, thread, now);
    until_ran(&mut rig, thread, queued);
    rig.run_until("its end", |actor| no_turn(actor, thread));
    let ends = ends(&mut rig, thread);
    assert_eq!(
        ends[ends.len() - 2..],
        [(now, StopReason::EndTurn), (queued, StopReason::EndTurn)]
    );
    rig.finish();
}
