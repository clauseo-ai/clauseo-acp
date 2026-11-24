//! The switch's one commit (`AgentSessionDetached { Switched }`, `AgentSwitched`, the row,
//! `ContextHandoff`), and what its result does: launch the new agent, owe the handover to the
//! next turn when its own ended meanwhile, or put the row back.

use clauseo_protocol::{
    AgentId, AgentSessionDetached, AgentSwitched, ContextHandoff, DetachReason, Envelope,
    ErrorCode, HandoffId, HandoffStatus, ThreadId, TurnId,
};

use super::super::super::Actor;
use super::super::agents::{self, Waiting};
use super::super::out::Out;
use super::super::{Then, current, failure, lost, state, steps};
use super::handover::{self, Fit};
use super::{Handoff, Stage, set_stage, settle};
use crate::error::Error;
use crate::log_file::LIFECYCLE;
use crate::pipeline::Failure;

/// The detail of `prompt.handoff_too_long`.
const TOO_LONG: &str = "The request leaves too little room for the earlier conversation. \
     Shorten the request or choose a model with a larger context; your message was not shortened.";

/// A switch's commit in flight, kept on the thread: its turn may end before it returns.
#[derive(Debug)]
pub(in super::super) struct InFlight {
    turn: TurnId,
    /// The agent the handover comes from.
    from: AgentId,
    /// Its `ContextHandoff`, when it has one.
    handoff: Option<HandoffId>,
    /// The thread's row and picks before the commit, put back if it fails; none for a handover
    /// owed, which changes no row.
    before: Option<Box<Before>>,
    /// The old agent's detach is in the commit.
    detach: bool,
}

/// A thread's row and picks before a switch.
type Before = (
    clauseo_store::ThreadRow,
    Option<clauseo_protocol::ConfigPicks>,
);

/// The switch's commit; or, for a handover that does not fit even as its header,
/// `ContextHandoff` alone and the turn's failure.
pub(super) fn commit(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    log: &[Envelope],
) -> Result<(), Error> {
    let entry = actor.check_thread(thread)?;
    let current = entry
        .turn
        .current
        .as_deref()
        .ok_or_else(|| lost("a turn"))?;
    let switch = current.switch.as_deref().ok_or_else(|| lost("a switch"))?;
    let (to, owed_from) = (switch.to.clone(), switch.owed_from.clone());
    let detach = switch.detach.clone();
    let from = owed_from.clone().unwrap_or_else(|| entry.row.agent.clone());
    let message = current.message;
    let built = handover::build(log, &from, &current.prompt);
    // An owed handover's agent has switched already: `from`'s options are gone, `to`'s shown.
    let owed = owed_from.is_some();
    let logged = match built.fit {
        Fit::Nothing => None,
        Fit::Text(_) | Fit::TooLong => Some(ContextHandoff {
            handoff: actor.ids.mint(actor.now().wall)?,
            message,
            from: handover::endpoint(actor, thread, &from, !owed),
            to: handover::endpoint(actor, thread, &to, owed),
            answers_covered: built.answers,
            items_included: built.included,
            items_left_out: built.left_out,
        }),
    };
    let id = logged.as_ref().map(|logged| logged.handoff);
    let mut out = Out::new(actor, thread, None)?;
    let text = match built.fit {
        Fit::TooLong => {
            if let Some(logged) = logged {
                out.push(actor, logged)?;
            }
            out.send(actor, Then::Nothing)?;
            keep(actor, thread, &from, id, None);
            tracing::info!(target: LIFECYCLE, %thread, %turn, "a switch's handover does not fit");
            let code = ErrorCode::PromptHandoffTooLong;
            steps::end_launch_failed(actor, thread, turn, code, Some(TOO_LONG.to_owned()));
            return Ok(());
        }
        Fit::Nothing if owed => {
            // Nothing is left to hand over: the turn goes on as any other.
            if let Some(state) = state(actor, thread) {
                state.switch.owed = None;
            }
            if let Some(current) = super::super::current(actor, thread) {
                current.switch = None;
            }
            agents::ensure(actor, thread, Waiting::Turn(turn));
            return Ok(());
        }
        Fit::Nothing => None,
        Fit::Text(text) => Some(text),
    };
    if !owed {
        if let Some(reason) = detach.clone() {
            out.push(actor, AgentSessionDetached { reason })?;
        }
        let switched = AgentSwitched {
            from: from.clone(),
            to: to.clone(),
        };
        out.push(actor, switched)?;
    }
    if let Some(logged) = logged {
        out.push(actor, logged)?;
    }
    let before = if owed {
        None
    } else {
        let now = actor.now().wall;
        let entry = actor
            .threads
            .get_mut(&thread)
            .ok_or_else(|| lost("a thread"))?;
        let before = Box::new((entry.row.clone(), entry.picks.take()));
        entry.row.agent = to;
        entry.row.acp_session_id = None;
        entry.row.updated = now;
        out.draft.thread_row(entry.row.clone());
        Some(before)
    };
    keep(actor, thread, &from, id, text);
    set_stage(actor, thread, Stage::Committing);
    if let Some(state) = state(actor, thread) {
        state.switch.in_flight = Some(InFlight {
            turn,
            from,
            handoff: id,
            before,
            detach: detach.is_some() && !owed,
        });
    }
    let sent = out.send(actor, Then::Switched(turn));
    if sent.is_err() {
        refused(actor, thread, turn);
    }
    sent
}

/// Keeps the handover `id` (none when there is no handover) on the turn's switch, with its text.
fn keep(
    actor: &mut Actor,
    thread: ThreadId,
    from: &AgentId,
    id: Option<HandoffId>,
    text: Option<String>,
) {
    if let Some(switch) = current(actor, thread).and_then(|current| current.switch.as_deref_mut()) {
        switch.from = Some(from.clone());
        switch.handoff = id.map(|id| Handoff {
            id,
            text,
            settled: false,
        });
    }
}

/// The switch's commit of `turn` was refused: the row and picks go back, its handover was never
/// logged, and its turn's switch is back to before the commit. Whether the old agent's detach
/// went with it.
fn refused(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    let in_flight = state(actor, thread).and_then(|state| {
        state
            .switch
            .in_flight
            .take_if(|in_flight| in_flight.turn == turn)
    });
    if let Some(switch) = current(actor, thread)
        .filter(|current| current.turn == turn)
        .and_then(|current| current.switch.as_deref_mut())
        .filter(|switch| switch.stage == Stage::Committing)
    {
        switch.handoff = None;
        switch.stage = Stage::Reading;
    }
    let Some(in_flight) = in_flight else {
        return false;
    };
    if let Some(before) = in_flight.before
        && let Some(entry) = actor.threads.get_mut(&thread)
    {
        let (row, picks) = *before;
        entry.row = row;
        entry.picks = picks;
    }
    in_flight.detach
}

/// The switch's commit for `turn` returned. Durable: the new agent launches for the turn, the
/// config and mode changes held for it first. Durable after the turn ended (a cancel while it
/// was in flight): the thread has switched all the same, so the next turn owes the handover.
/// Refused: the row goes back and the turn ends as a turn whose commit failed.
pub(in super::super) fn committed(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    outcome: Result<(), &Failure>,
) {
    let found = current(actor, thread).is_some_and(|current| {
        current.turn == turn
            && current
                .switch
                .as_deref()
                .is_some_and(|switch| switch.stage == Stage::Committing)
    });
    let Err(failed) = outcome else {
        let in_flight = state(actor, thread).and_then(|state| {
            state
                .switch
                .in_flight
                .take_if(|in_flight| in_flight.turn == turn)
        });
        if found {
            switched(actor, thread, turn);
        } else if let Some(InFlight {
            from,
            handoff: Some(id),
            ..
        }) = in_flight
        {
            tracing::info!(target: LIFECYCLE, %thread, %turn, "a switch's turn ended before its commit; the next turn carries the handover");
            if let Some(state) = state(actor, thread) {
                state.switch.owed = Some(from);
            }
            settle::log(actor, thread, id, HandoffStatus::Failed);
        }
        return;
    };
    let detach = refused(actor, thread, turn);
    if found {
        failure::on_store_failure(actor, thread, failed);
    } else if detach {
        agents::log_detach(actor, thread, DetachReason::Closed);
    }
}

/// `turn`'s switch is durable: the held changes, then the turn, wait for the new agent.
fn switched(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    set_stage(actor, thread, Stage::Switched);
    // This turn carries the handover now; a failure owes it again (`settle`).
    if let Some(state) = state(actor, thread) {
        state.switch.owed = None;
    }
    let held = current(actor, thread)
        .and_then(|current| current.switch.as_deref_mut())
        .map(|switch| {
            switch.detach = None;
            std::mem::take(&mut switch.held)
        })
        .unwrap_or_default();
    super::release(actor, thread, held);
    agents::ensure(actor, thread, Waiting::Turn(turn));
}
