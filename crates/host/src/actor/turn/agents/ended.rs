//! A thread's agent stopping and ending: why it was stopped ([`StopCause`]), the stop of an
//! agent whose connection is gone, the shutdown ladder's stop, and `Ended`: the detach it logs
//! and the turn the exit cut short.

use clauseo_protocol::{
    AgentSessionDetached, DetachReason, ErrorCode, StopReason, StreamId, ThreadId,
};

use super::super::super::Actor;
use super::super::launch_checks;
use super::super::out::Out;
use super::super::{End, Then, TurnPhase, current, end, state, updates};
use super::{Agent, Stage, Waiting, fail_waiting, start};
use crate::agent::{Ended, StopStep, leftovers};
use crate::error::Error;
use crate::log_file::Cause;
use crate::log_file::LIFECYCLE;

/// Why an agent is stopped, for the detach its `Ended::Stopped` logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StopCause {
    /// The host closed it (a stop, a failed launch): `Closed`.
    Asked,
    /// It starts again in its matter's new folder: `Restarting`.
    Restarting,
    /// It had nothing pending for the idle limit, or the live-agents cap needed its place
    /// (`idle.rs`): `IdleStopped`.
    IdleStopped,
    /// Its connection was already gone when the stop came, which only ends a process that
    /// outlived its pipes: the exit report's reason, as if it had exited first.
    Closed,
    /// The thread moves to another agent (`switch.rs`): `Switched`.
    Switched,
}

/// Stops an agent whose connection is gone; `waiting` goes to the agent started after its
/// `Ended`. The stop ends a process that outlived its pipes, and it can beat the owner's
/// exit report (Windows reports an exit late): `on_ended` then logs the report's exit.
pub(super) fn stop_closed(agent: &mut Agent, waiting: Waiting) {
    agent.link.stop();
    agent.stage = Stage::Stopping;
    agent.stop_cause = StopCause::Closed;
    agent.after_stop.push(waiting);
}

/// Stops `thread`'s agent with the shutdown ladder; `Ended` follows. While the host stops (a
/// quit) its listing waits the short [`leftovers::SNAPSHOT_WAIT`], which the quit's budget counts.
pub(in super::super) fn stop(actor: &mut Actor, thread: ThreadId) {
    let list_wait = if super::super::host_stopping(actor) {
        leftovers::SNAPSHOT_WAIT
    } else {
        leftovers::STOP_LIST_WAIT
    };
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    if agent.stage == Stage::Stopping {
        return;
    }
    agent.link.stop_listing_within(list_wait);
    if agent.stage == Stage::Closed || agent.link.closed() {
        agent.stop_cause = StopCause::Closed;
    }
    agent.stage = Stage::Stopping;
    let waiting = std::mem::take(&mut agent.waiting);
    for item in waiting {
        fail_waiting(actor, thread, &item, &Error::Closed);
    }
    // The session is ending: its case-law relay refuses from now on (R24).
    launch_checks::relay::end_for(actor, thread);
}

/// Stops `thread`'s agent for idling (`idle.rs`) with the shutdown ladder: its `Ended` logs
/// `AgentSessionDetached { reason: IdleStopped }`, unless its connection was already gone.
pub(in super::super) fn idle_stop(actor: &mut Actor, thread: ThreadId) {
    if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut())
        && agent.stage != Stage::Stopping
    {
        agent.stop_cause = StopCause::IdleStopped;
    }
    stop(actor, thread);
}

/// Stops `thread`'s agent for a switch to another agent (`switch.rs`) with the shutdown ladder:
/// its `Ended` gives the switch `AgentSessionDetached { reason: Switched }` to log, unless its
/// connection was already gone. An idle stop under way becomes the switch's. The config and mode
/// calls waiting for it, or for the agent after it, come back: the switch runs them on the new
/// agent.
pub(in super::super) fn switch_stop(actor: &mut Actor, thread: ThreadId) -> Vec<Waiting> {
    let mut calls = Vec::new();
    if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) {
        if matches!(agent.stop_cause, StopCause::Asked | StopCause::IdleStopped) {
            agent.stop_cause = StopCause::Switched;
        }
        for list in [&mut agent.waiting, &mut agent.after_stop] {
            let (turns, other): (Vec<_>, Vec<_>) = std::mem::take(list)
                .into_iter()
                .partition(|item| matches!(item, Waiting::Turn(_)));
            *list = turns;
            calls.extend(other);
        }
    }
    stop(actor, thread);
    calls
}

/// The switch `thread`'s agent stops for is over before its end (`switch.rs`): the end logs
/// the detach a plain stop logs, `Closed`.
pub(in super::super) fn unswitch_stop(actor: &mut Actor, thread: ThreadId) {
    if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut())
        && agent.stop_cause == StopCause::Switched
    {
        agent.stop_cause = StopCause::Asked;
    }
}

/// Stops `thread`'s agent for a sign-in hold or restart (`sign_in.rs`). An idle stop already
/// under way becomes the hold's own stop: its `Ended` logs `Closed`, once, not `IdleStopped`,
/// so the thread shows the hold beside its queue, not an idle stop.
pub(in super::super) fn hold_stop(actor: &mut Actor, thread: ThreadId) {
    if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut())
        && agent.stage == Stage::Stopping
        && agent.stop_cause == StopCause::IdleStopped
    {
        agent.stop_cause = StopCause::Asked;
    }
    stop(actor, thread);
}

/// The agent's tasks are done: log the detach, end a turn the exit cut short, and start a new
/// agent for work that came while it stopped.
pub(in super::super) fn on_ended(actor: &mut Actor, thread: ThreadId, ended: Ended) {
    // Updates the rate cap holds came before the exit, while the turn is still live.
    updates::flush_rate(actor, thread);
    // Claude's held line goes back while the agent is still there to log it.
    super::super::offline::turn_ending(actor, thread, None);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.take()) else {
        return;
    };
    // What it sent between turns went with its connection: no card of it is answered on the
    // next agent, whose request keys start again at 1.
    super::super::own_turn::refused::agent_ended(actor, thread);
    super::super::idle::ended(actor, thread);
    // Its pending questions went with it.
    super::super::super::limits::asking_changed(actor);
    launch_checks::remove_run(actor, thread, agent.link.tree_watch());
    let Agent {
        session,
        waiting,
        after_stop,
        stop_cause,
        denied_data_folder,
        start_failure,
        matter_spellings,
        ..
    } = *agent;
    let unresponsive = matches!(
        ended,
        Ended::Stopped {
            step: StopStep::Unresponsive,
            ..
        }
    );
    // Before its session was ready, after a failed launch or on its own: its start's stderr.
    if session.is_none() {
        match &ended {
            Ended::Exited(report) => {
                super::start_failure::log_start_tail(
                    thread,
                    start_failure,
                    report,
                    &matter_spellings,
                );
            }
            Ended::Stopped { report, .. }
                if start_failure.is_some() || stop_cause == StopCause::Closed =>
            {
                super::start_failure::log_start_tail(
                    thread,
                    start_failure,
                    report,
                    &matter_spellings,
                );
            }
            Ended::Stopped { .. } => {}
        }
    }
    let reason = match ended {
        Ended::Exited(reason) => reason,
        Ended::Stopped { step, report } => {
            if step == StopStep::Unresponsive {
                let (kind, code) = reason_kind(&report);
                tracing::warn!(%thread, kind, ?code, "the agent did not exit after its kill");
            }
            match stop_cause {
                StopCause::Asked => DetachReason::Closed,
                StopCause::Restarting => DetachReason::Restarting,
                StopCause::IdleStopped => DetachReason::IdleStopped,
                StopCause::Switched => DetachReason::Switched,
                // It was gone before the stop: the exit is its own, not a close.
                StopCause::Closed => report,
            }
        }
    };
    let (kind, code) = reason_kind(&reason);
    tracing::info!(
        target: LIFECYCLE,
        %thread,
        kind,
        ?code,
        denied_data_folder,
        "agent ended"
    );
    let exited = exited(session.is_none());
    for item in waiting {
        fail_waiting(actor, thread, &item, &exited);
    }
    let detach = end_own_unstarted(actor, thread, session.is_some().then_some(reason));
    let phase = current(actor, thread).map(|current| (current.phase, current.cancelled));
    match (phase, detach) {
        (Some((TurnPhase::Prompting, cancelled)), detach) => {
            let end = if cancelled {
                End::cancelled(detach)
            } else {
                End::failed(ErrorCode::AgentExited, None, detach)
            };
            end::end_turn(actor, thread, end);
        }
        (Some((TurnPhase::FallbackStop, _)), detach) => {
            if let Some(reason) = detach {
                log_detach(actor, thread, reason);
            }
            end::end_turn(actor, thread, End::cancelled(None));
        }
        (Some((TurnPhase::Ending, _)), Some(reason)) => {
            // Logged after the turn's `TurnEnded`, which is still to come.
            if let Some(current) = current(actor, thread) {
                match &mut current.end {
                    Some(end) => end.detach = Some(reason),
                    None => {
                        current.end = Some(End::failed(ErrorCode::AgentExited, None, Some(reason)));
                    }
                }
            }
        }
        (
            Some((TurnPhase::Queued | TurnPhase::Checkpointing | TurnPhase::Starting, _)),
            Some(reason),
        ) => {
            // The turn ends when it reaches its prompt and finds no agent; the detach goes
            // after its `TurnEnded`.
            if let Some(current) = current(actor, thread) {
                current.detach = Some(reason);
            }
        }
        (_, Some(reason)) => detach_unless_switching(actor, thread, reason),
        (_, None) => {}
    }
    start_after(actor, thread, after_stop);
    super::super::sign_in::agent_ended(actor, thread, unresponsive);
    actor.release(StreamId::Thread(thread));
}

/// Logs the detach of `thread`'s agent with `reason`, unless a switch logs it with the switch
/// (`switch.rs`).
fn detach_unless_switching(actor: &mut Actor, thread: ThreadId, reason: DetachReason) {
    if let Some(reason) = super::super::switch::keeps_detach(actor, thread, reason) {
        log_detach(actor, thread, reason);
    }
}

/// After an agent's end: a new agent for the work that came while it stopped, or a switch that
/// waited for the end goes on (`switch.rs`).
fn start_after(actor: &mut Actor, thread: ThreadId, after_stop: Vec<Waiting>) {
    if !after_stop.is_empty() {
        start(actor, thread, after_stop);
    }
    super::super::switch::agent_ended(actor, thread);
}

/// An agent-started turn of `thread` that had not started when its agent ended was opened for that
/// agent's output, which went with it (`refused::agent_ended`): it never starts on another agent
/// with nothing to show, where only its quiet timer would end it, a minute later, as an empty
/// exchange with an Undo step, Stop showing and the lawyer's prompts queued behind it. With its
/// `TurnQueued` logged and waiting (for an undo, a restore or a relocate holding the matter, or
/// running in it for an agent the matter's move stopped, `restart_if_moved`) it ends now,
/// `Cancelled` with no `TurnStarted`, carrying `detach`, which it takes; checkpointing, starting,
/// or not yet logged, it is marked cancelled and ends, still with no `TurnStarted` but for a start
/// already in flight, when that step answers. Not while the host stops: start-up ends such a turn
/// (`own_turn/restart.rs`). A prompt of the lawyer's is left alone. Returns the detach still to
/// log.
fn end_own_unstarted(
    actor: &mut Actor,
    thread: ThreadId,
    detach: Option<DetachReason>,
) -> Option<DetachReason> {
    if super::super::host_stopping(actor) {
        return detach;
    }
    let running = actor
        .check_thread(thread)
        .map(|entry| entry.row.matter)
        .ok()
        .and_then(|matter| actor.matters.get(&matter))
        .is_some_and(|entry| entry.turns.running.contains(&thread));
    let Some(current) = current(actor, thread)
        .filter(|current| current.own.is_some() && !current.cancelled && !current.live())
    else {
        return detach;
    };
    let turn = current.turn;
    let now = match current.phase {
        TurnPhase::Queued => true,
        TurnPhase::Launching => running,
        TurnPhase::Accepting | TurnPhase::Checkpointing | TurnPhase::Starting => false,
        TurnPhase::Prompting | TurnPhase::FallbackStop | TurnPhase::Ending => return detach,
    };
    tracing::info!(target: LIFECYCLE, %thread, %turn, now, "the agent ended before its agent-started turn started; the turn ends");
    if !now {
        current.cancelled = true;
        return detach;
    }
    if detach.is_some() {
        current.detach = detach;
    }
    super::super::steps::end_unstarted(actor, thread, turn, None, StopReason::Cancelled, None);
    None
}

/// A detach reason as the log file may show it: its kind and exit code, never `stderr_tail`,
/// which is the agent's output (decision record 0047).
fn reason_kind(reason: &DetachReason) -> (&'static str, Option<i32>) {
    match reason {
        DetachReason::Exited { code, .. } => ("exited", *code),
        DetachReason::Closed => ("closed", None),
        DetachReason::Restarting => ("restarting", None),
        DetachReason::IdleStopped => ("idle_stopped", None),
        DetachReason::Switched => ("switched", None),
        DetachReason::Unknown { .. } => ("unknown", None),
    }
}

/// Logs `AgentSessionDetached { reason }`, after ending every helper still open `Stopped`.
pub(in super::super) fn log_detach(actor: &mut Actor, thread: ThreadId, reason: DetachReason) {
    let sent = Out::new(actor, thread, None).and_then(|mut out| {
        super::super::helpers::on_detach(actor, thread, &mut out)?;
        out.push(actor, AgentSessionDetached { reason })?;
        out.send(actor, Then::Nothing)
    });
    if let Err(error) = sent {
        tracing::warn!(%thread, error = %Cause(&error), "the detach was not logged");
    }
}

/// What the work waiting for an agent that ended fails with: before its session was ready the
/// agent did not start (`agent.launch_failed`); after, the answer stopped (`agent.exited`).
fn exited(before_session: bool) -> Error {
    if before_session {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            super::start_failure::EXITED_AT_START,
        )
    } else {
        Error::refused(ErrorCode::AgentExited, "the agent exited")
    }
}
