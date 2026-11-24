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

/// Set once the prompt carrying a handover goes out.
pub(in super::super) type WentOut = Rc<Cell<bool>>;

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

/// The prompt of `turn` carrying `thread`'s handover went out, past the Codex `.codex`
/// recheck: `HandoffSettled { Delivered }`.
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
    /// The prompt passed the Codex `.codex` recheck and goes out now.
    pub(in super::super) fn sent(self) {
        self.went_out.set(true);
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
        .is_some_and(|handoff| handoff.went_out.get());
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
    let status = if went_out {
        HandoffStatus::Delivered
    } else {
        HandoffStatus::Failed
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
    let Some(handoff) = switch.handoff.as_mut().filter(|handoff| !handoff.settled) else {
        return;
    };
    handoff.settled = true;
    let id = handoff.id;
    if let Some(state) = state(actor, thread) {
        match status {
            HandoffStatus::Failed if switched => state.switch.owed = from,
            HandoffStatus::Delivered => state.switch.owed = None,
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
/// commit and its settle) settles `Failed`; its turn did not survive the stop.
pub(in super::super) fn at_start(actor: &mut Actor, thread: ThreadId) {
    let pending: Vec<HandoffId> = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .map(|state| {
            state
                .entries()
                .values()
                .filter_map(|entry| {
                    if let ViewEntry::Handoff(handoff) = entry
                        && handoff.status == HandoffEntryStatus::Pending
                    {
                        Some(handoff.handoff)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    for handoff in pending {
        tracing::info!(target: LIFECYCLE, %thread, "a handover left pending by a stop settles failed");
        log(actor, thread, handoff, HandoffStatus::Failed);
    }
}
