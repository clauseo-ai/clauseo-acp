//! The switch's one commit (`AgentSessionDetached { Switched }`, `AgentSwitched`, the row,
//! `ContextHandoff`), and what its result does: launch the new agent, owe the handover to the
//! next turn when its own ended meanwhile, or put the row back. Back to an agent whose own
//! session saw part of the conversation, the `ContextHandoff` waits for that session's attach
//! instead (`resume.rs`).

use clauseo_protocol::{
    AgentId, AgentSessionDetached, AgentSwitched, DetachReason, Envelope, ErrorCode, HandoffId,
    HandoffStatus, StreamId, ThreadId, TurnId,
};

use super::super::super::Actor;
use super::super::agents::{self, Waiting};
use super::super::out::Out;
use super::super::{Then, current, failure, lost, state, steps};
use super::handover::Fit;
use super::{Handoff, Stage, resume, set_stage, settle};
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
    /// Its handover waits for the new agent's session to attach (`resume.rs`).
    waits: bool,
    /// The thread's row, picks and kept header before the commit, put back if it fails; none
    /// for a handover owed, which changes no row.
    before: Option<Box<Before>>,
    /// The old agent's detach is in the commit.
    detach: bool,
    /// The config and mode changes the switch held when its turn ended first: they run once
    /// the commit returns, on the new agent when it is durable, else on the old one.
    pub(super) held: Vec<Waiting>,
}

/// A thread's row, picks and kept header before a switch.
type Before = (
    clauseo_store::ThreadRow,
    Option<(AgentId, clauseo_protocol::ConfigPicks)>,
    Option<clauseo_protocol::HeaderEntry>,
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
    let (detach, unconfirmed) = (switch.detach.clone(), switch.unconfirmed);
    let from = owed_from.clone().unwrap_or_else(|| entry.row.agent.clone());
    let owed = owed_from.is_some();
    let resume::Plan {
        from,
        built,
        resumed,
        ends,
        session,
        pending,
    } = resume::plan(actor, (thread, turn), log, (&from, &to, owed, unconfirmed))?;
    if owed && let Some(pending) = pending {
        owed_waits(actor, (thread, turn), &from, pending);
        return Ok(());
    }
    let logged = match built.fit {
        Fit::Nothing => None,
        Fit::Text(_) | Fit::TooLong => {
            let handoff = actor.ids.mint(actor.now().wall)?;
            Some(resume::event(handoff, ends, &built, resumed))
        }
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
            // The old session's helpers end with it, as at any detach (`agents::log_detach`).
            super::super::helpers::on_switch_detach(actor, thread, &mut out)?;
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
        let before = Box::new((entry.row.clone(), entry.picks.take(), entry.header.clone()));
        // The header kept for an unloaded stream folds nothing: it takes the switch's rule here,
        // so the old agent's options, mode, commands and tools are not read as the thread's
        // (`picks.rs`) before a refold.
        if let Some(header) = entry.header.as_mut() {
            clauseo_reducer::switched_header(header, &to);
        }
        entry.row.agent = to;
        entry.row.acp_session_id = None;
        entry.row.updated = now;
        // The old agent's `CreateThread` picks went with `take`; the new session starts with
        // the message's own (`SendPrompt.config`), as a thread's first session does
        // (`prompt_picks.rs`), and so does one that loads.
        super::super::prompt_picks::switched(actor, thread, turn);
        // The session the new agent last had here, which its launch loads (4b); none when it
        // never ran here, and it starts a new one.
        let entry = actor
            .threads
            .get_mut(&thread)
            .ok_or_else(|| lost("a thread"))?;
        entry.row.acp_session_id = session;
        out.draft.thread_row(entry.row.clone());
        Some(before)
    };
    keep(actor, thread, &from, id, text);
    let waits = pending.is_some();
    if let Some(switch) =
        super::super::current(actor, thread).and_then(|current| current.switch.as_deref_mut())
    {
        switch.pending = pending;
    }
    set_stage(actor, thread, Stage::Committing);
    if let Some(state) = state(actor, thread) {
        state.switch.in_flight = Some(InFlight {
            turn,
            from,
            handoff: id,
            waits,
            before,
            detach: detach.is_some() && !owed,
            held: Vec::new(),
        });
    }
    let sent = out.send(actor, Then::Switched(turn));
    if sent.is_err() {
        let _no_turn_ended = refused(actor, thread, turn);
    }
    sent
}

/// The handover `turn` owes from `from` waits for the session the thread's agent loads: no
/// commit until it attaches (`resume::attached`).
fn owed_waits(
    actor: &mut Actor,
    (thread, turn): (ThreadId, TurnId),
    from: &AgentId,
    pending: Box<resume::Pending>,
) {
    keep(actor, thread, from, None, None);
    if let Some(switch) = current(actor, thread).and_then(|current| current.switch.as_deref_mut()) {
        switch.pending = Some(pending);
    }
    switched(actor, thread, turn);
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
            went_out: settle::WentOut::default(),
        });
    }
}

/// The switch's commit of `turn` was refused: the row and picks go back, its handover was never
/// logged, and its turn's switch is back to before the commit. Whether the old agent's detach
/// went with it, and the changes held on the commit once its turn ended.
fn refused(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> (bool, Vec<Waiting>) {
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
        switch.pending = None;
        switch.stage = Stage::Reading;
    }
    let Some(in_flight) = in_flight else {
        return (false, Vec::new());
    };
    if let Some(before) = in_flight.before
        && let Some(entry) = actor.threads.get_mut(&thread)
    {
        let (row, picks, header) = *before;
        entry.row = row;
        entry.picks = picks;
        entry.header = header;
    }
    (in_flight.detach, in_flight.held)
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
        // The old session's helpers ended in the commit: the thread forgets them now.
        if in_flight.as_ref().is_some_and(|in_flight| in_flight.detach) {
            super::super::helpers::forget(actor, thread);
        }
        // The old session's tools leave the header with it; the new session's show once it
        // starts (`header_tools.rs`).
        if in_flight
            .as_ref()
            .is_some_and(|in_flight| in_flight.before.is_some())
        {
            crate::actor::header_tools::switched(actor, thread);
        }
        if found {
            switched(actor, thread, turn);
        } else if let Some(InFlight {
            from,
            handoff,
            waits,
            held,
            ..
        }) = in_flight
        {
            if handoff.is_some() || waits {
                tracing::info!(target: LIFECYCLE, %thread, %turn, "a switch's turn ended before its commit; the next turn carries the handover");
                if let Some(state) = state(actor, thread) {
                    state.switch.owed = Some(from);
                }
            }
            if let Some(id) = handoff {
                settle::log(actor, thread, id, HandoffStatus::Failed);
            }
            // Durable: the held changes run on the new agent.
            super::release(actor, thread, held);
        }
        return;
    };
    let (detach, held) = refused(actor, thread, turn);
    if found {
        failure::on_store_failure(actor, thread, failed);
    } else if detach {
        // The refusal blocked the stream until its commits drain: the detach waits for that.
        if actor.pipeline.blocked(StreamId::Thread(thread)).is_some() {
            if let Some(state) = state(actor, thread) {
                state.switch.detach_owed = true;
            }
        } else {
            // The helpers the refused commit ended are open again for the detach to end. Only
            // with no commit of the thread in flight, as `failure::closing_sent` does.
            if !actor.pipeline.in_flight(StreamId::Thread(thread)) {
                super::super::helpers::reopen_from_view(actor, thread);
            }
            agents::log_detach(actor, thread, DetachReason::Closed);
        }
    }
    // Refused: the row is back, so the changes held since the turn ended run on the old agent.
    super::release(actor, thread, held);
}

/// `thread`'s stream drained after a failure: the `Closed` detach a refused switch commit
/// owes is logged now.
pub(in super::super) fn unblocked(actor: &mut Actor, thread: ThreadId) {
    let owed =
        state(actor, thread).is_some_and(|state| std::mem::take(&mut state.switch.detach_owed));
    if owed {
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
