//! A switch's handover reaching the new agent, or not: the prompt that carries it, its
//! `HandoffSettled`, the end of a turn whose switch did not finish, and start-up's settle of a
//! handover a crash left pending.

use std::cell::Cell;
use std::rc::Rc;

use clauseo_protocol::{
    ContentBlock, DetachReason, HandoffEntryStatus, HandoffId, HandoffSettled, HandoffStatus,
    StreamId, ThreadId, TurnId, ViewEntry,
};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::agents::{self, Waiting};
use super::super::out::Out;
use super::super::{AgentReply, Current, Then, TurnInput, current, state};
use super::{Stage, handover};
use crate::log_file::{Cause, LIFECYCLE};

/// `thread`'s agent ended with `reason`: a switch waiting for that end keeps a `Switched` detach
/// to log with the switch (`None`); any other goes back to be logged now.
pub(in super::super) fn keeps_detach(
    actor: &mut Actor,
    thread: ThreadId,
    reason: DetachReason,
) -> Option<DetachReason> {
    if reason != DetachReason::Switched {
        return Some(reason);
    }
    match current(actor, thread)
        .and_then(|current| current.switch.as_deref_mut())
        .filter(|switch| switch.stage == Stage::Stopping)
    {
        Some(switch) => {
            switch.detach = Some(reason);
            None
        }
        None => Some(reason),
    }
}

/// Whether `thread`'s session starting now was started by a switch: it gets no "context not
/// restored" notice, since the handover carries the context.
pub(in super::super) fn started_by_switch(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.current.as_deref())
        .and_then(|current| current.switch.as_deref())
        .is_some_and(|switch| switch.stage == Stage::Switched)
}

/// How far the prompt carrying a handover got.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in super::super) enum Went {
    /// Not sent: the turn ended first, or the agent refused it.
    #[default]
    No,
    /// Sent, past the Codex `.codex` recheck, and not answered yet.
    Sent,
    /// The agent answered it with no error.
    Answered,
}

/// Set by the prompt's task as the prompt carrying a handover goes out and is answered.
pub(in super::super) type WentOut = Rc<Cell<Went>>;

/// The prompt `current` sends: the handover first when its switch has one, else as typed; and
/// when it carries a handover, the flag its task sets once it goes out.
pub(in super::super) fn prompt(current: &mut Current) -> (Vec<ContentBlock>, Option<WentOut>) {
    let carried = current
        .switch
        .as_deref_mut()
        .and_then(|switch| switch.handoff.as_mut())
        .and_then(|handoff| Some((handoff.text.take()?, Rc::clone(&handoff.went_out))));
    match carried {
        Some((text, went_out)) => (
            handover::wrap(&text, current.prompt.clone()),
            Some(went_out),
        ),
        None => (current.prompt.clone(), None),
    }
}

/// The agent answered the prompt of `turn` carrying `thread`'s handover with no error:
/// `HandoffSettled { Delivered }`.
pub(in super::super) fn delivered(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if current(actor, thread).is_some_and(|current| current.turn == turn) {
        settle(actor, thread, HandoffStatus::Delivered);
    }
}

/// The note, from a prompt's task, that the prompt carrying a handover goes out.
pub(in super::super) struct GoingOut {
    post: async_channel::Sender<Input>,
    thread: ThreadId,
    agent: u64,
    turn: TurnId,
    went_out: WentOut,
}

impl GoingOut {
    /// The prompt goes out to the agent, past the Codex `.codex` recheck.
    pub(in super::super) fn going(&self) {
        self.went_out.set(Went::Sent);
    }

    /// The agent refused the prompt: it did not take the handover.
    pub(in super::super) fn refused(&self) {
        self.went_out.set(Went::No);
    }

    /// The agent answered the prompt with no error, past the Codex `.codex` recheck.
    pub(in super::super) fn sent(self) {
        self.went_out.set(Went::Answered);
        let reply = AgentReply::PromptOut { turn: self.turn };
        let input = TurnInput::Agent {
            thread: self.thread,
            agent: self.agent,
            reply,
        };
        let _gone = self.post.try_send(Input::Turn(input));
    }
}

/// The note for the prompt of `turn` to `thread`'s agent `agent`, when it carries a handover.
pub(in super::super) fn going_out(
    actor: &Actor,
    (thread, agent, turn): (ThreadId, u64, TurnId),
    carries: Option<WentOut>,
) -> Option<GoingOut> {
    carries.map(|went_out| GoingOut {
        post: actor.post.clone(),
        thread,
        agent,
        turn,
        went_out,
    })
}

/// `thread`'s turn ends, before its current is let go: a handover whose prompt never went out
/// settles `Failed` (the thread owes it when the switch was made), one whose prompt went out
/// though its `PromptOut` has not arrived settles `Delivered`, and a switch that did not
/// happen logs its old agent's detach as a plain stop's, `Closed`, or has the stopping agent
/// log that. A commit still in flight settles when it returns (`commit.rs`), and keeps the
/// config and mode changes held on the switch, to run them once it is durable or refused. Else
/// those come back, for [`release`] once the turn is let go.
pub(in super::super) fn ends(actor: &mut Actor, thread: ThreadId) -> Vec<Waiting> {
    let Some(switch) = current(actor, thread).and_then(|current| current.switch.as_deref_mut())
    else {
        return Vec::new();
    };
    let held = std::mem::take(&mut switch.held);
    let (stage, detach) = (switch.stage, switch.detach.take());
    let went_out = switch
        .handoff
        .as_ref()
        .map_or(Went::No, |handoff| handoff.went_out.get());
    match stage {
        Stage::Committing => {
            // Until the commit returns the row may name an agent the store does not have.
            return match state(actor, thread).and_then(|state| state.switch.in_flight.as_mut()) {
                Some(in_flight) => {
                    in_flight.held.extend(held);
                    Vec::new()
                }
                None => held,
            };
        }
        Stage::Stopping => agents::unswitch_stop(actor, thread),
        Stage::Asked | Stage::Reading | Stage::Switched => {}
    }
    let status = match went_out {
        Went::Answered => HandoffStatus::Delivered,
        Went::Sent => HandoffStatus::Unconfirmed,
        Went::No => HandoffStatus::Failed,
    };
    settle(actor, thread, status);
    if detach.is_some() {
        agents::log_detach(actor, thread, DetachReason::Closed);
    }
    held
}

/// Config and mode changes a switch held go to `thread`'s agent.
pub(in super::super) fn release(actor: &mut Actor, thread: ThreadId, held: Vec<Waiting>) {
    for waiting in held {
        agents::ensure(actor, thread, waiting);
    }
}

fn settle(actor: &mut Actor, thread: ThreadId, status: HandoffStatus) {
    let Some(switch) = current(actor, thread).and_then(|current| current.switch.as_deref_mut())
    else {
        return;
    };
    // A switch that was logged owes its new agent the handover until a prompt carries it.
    let switched = switch.stage == Stage::Switched;
    let from = switch.from.clone();
    // A handover waiting for the session to attach was never logged: owed all the same.
    if switch.pending.take().is_some() {
        if switched
            && status == HandoffStatus::Failed
            && let Some(state) = state(actor, thread)
        {
            state.switch.owed = from;
        }
        return;
    }
    let Some(handoff) = switch.handoff.as_mut().filter(|handoff| !handoff.settled) else {
        return;
    };
    handoff.settled = true;
    let id = handoff.id;
    if let Some(state) = state(actor, thread) {
        match status {
            HandoffStatus::Failed if switched => state.switch.owed = from,
            // The agent has it, or most likely has it: not handed over again.
            HandoffStatus::Delivered | HandoffStatus::Unconfirmed => state.switch.owed = None,
            HandoffStatus::Failed | HandoffStatus::Unknown => {}
        }
    }
    log(actor, thread, id, status);
}

/// Logs `HandoffSettled` of `handoff` with `status`.
pub(super) fn log(actor: &mut Actor, thread: ThreadId, handoff: HandoffId, status: HandoffStatus) {
    let sent = Out::new(actor, thread, None).and_then(|mut out| {
        out.push(actor, HandoffSettled { handoff, status })?;
        out.send(actor, Then::Nothing)
    });
    if let Err(error) = sent {
        tracing::warn!(%thread, error = %Cause(&error), "a handover's end was not logged");
    }
}

/// Start-up: each handover of `thread` still pending (the host stopped between the switch's
/// commit and its settle) settles `Failed`; its turn did not survive the stop. When the thread's
/// last handover did not reach its agent (pending, or settled `Failed`) and that agent is the
/// thread's now, the switch was made and its handover is owed: the next turn carries it, as
/// within a run (`Kept::owed`, which lives in memory only).
pub(in super::super) fn at_start(actor: &mut Actor, thread: ThreadId) {
    let agent = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone());
    let (pending, last) = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .map(|state| {
            let handoffs: Vec<_> = state
                .entries()
                .values()
                .filter_map(|entry| {
                    if let ViewEntry::Handoff(handoff) = entry {
                        Some(handoff)
                    } else {
                        None
                    }
                })
                .collect();
            let pending: Vec<HandoffId> = handoffs
                .iter()
                .filter(|handoff| handoff.status == HandoffEntryStatus::Pending)
                .map(|handoff| handoff.handoff)
                .collect();
            // The last handover to the thread's agent now: a later one to another agent (a
            // move that did not fit) leaves it as it was.
            let last = handoffs
                .iter()
                .filter(|handoff| Some(&handoff.to.agent) == agent.as_ref())
                .max_by_key(|handoff| handoff.order)
                .filter(|handoff| {
                    !matches!(
                        handoff.status,
                        HandoffEntryStatus::Delivered | HandoffEntryStatus::Unconfirmed
                    )
                })
                .map(|handoff| (handoff.from.agent.clone(), handoff.to.agent.clone()));
            (pending, last)
        })
        .unwrap_or_default();
    for handoff in pending {
        tracing::info!(target: LIFECYCLE, %thread, "a handover left pending by a stop settles failed");
        log(actor, thread, handoff, HandoffStatus::Failed);
    }
    if let (Some((from, to)), Some(agent)) = (last, agent)
        && to == agent
        && from != agent
        && let Some(state) = state(actor, thread)
    {
        tracing::info!(target: LIFECYCLE, %thread, "the thread's next turn carries the handover a stop kept from its agent");
        state.switch.owed = Some(from);
    }
    // A handover a switch back held for its session's attach (`resume.rs`).
    super::resume::at_start(actor, thread);
}
