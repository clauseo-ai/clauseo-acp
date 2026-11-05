//! A thread's agent stopping and ending: why it was stopped ([`StopCause`]), the stop of an
//! agent whose connection is gone, the shutdown ladder's stop, and `Ended`: the detach it logs
//! and the turn the exit cut short.

use clauseo_protocol::{AgentSessionDetached, DetachReason, ErrorCode, StreamId, ThreadId};

use super::super::super::Actor;
use super::super::launch_checks;
use super::super::out::Out;
use super::super::{End, Then, TurnPhase, current, end, state, updates};
use super::{Agent, Stage, Waiting, fail_waiting, start};
use crate::agent::{Ended, StopStep};
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
    /// Its connection was already gone when the stop came, which only ends a process that
    /// outlived its pipes: the exit report's reason, as if it had exited first.
    Closed,
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

/// Stops `thread`'s agent with the shutdown ladder; `Ended` follows.
pub(in super::super) fn stop(actor: &mut Actor, thread: ThreadId) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    if agent.stage == Stage::Stopping {
        return;
    }
    agent.link.stop();
    if agent.stage == Stage::Closed || agent.link.closed() {
        agent.stop_cause = StopCause::Closed;
    }
    agent.stage = Stage::Stopping;
    let waiting = std::mem::take(&mut agent.waiting);
    for item in waiting {
        fail_waiting(actor, thread, &item, &Error::Closed);
    }
}

/// The agent's tasks are done: log the detach, end a turn the exit cut short, and start a new
/// agent for work that came while it stopped.
pub(in super::super) fn on_ended(actor: &mut Actor, thread: ThreadId, ended: Ended) {
    // Updates the rate cap holds came before the exit, while the turn is still live.
    updates::flush_rate(actor, thread);
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.take()) else {
        return;
    };
    // Its pending questions went with it.
    super::super::super::limits::asking_changed(actor);
    launch_checks::remove_run(actor, thread);
    let Agent {
        session,
        waiting,
        after_stop,
        stop_cause,
        ..
    } = *agent;
    let unresponsive = matches!(
        ended,
        Ended::Stopped {
            step: StopStep::Unresponsive,
            ..
        }
    );
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
                // It was gone before the stop: the exit is its own, not a close.
                StopCause::Closed => report,
            }
        }
    };
    let (kind, code) = reason_kind(&reason);
    tracing::info!(target: LIFECYCLE, %thread, kind, ?code, "agent ended");
    let exited = Error::refused(ErrorCode::AgentExited, "the agent exited");
    for item in waiting {
        fail_waiting(actor, thread, &item, &exited);
    }
    let detach = session.is_some().then_some(reason);
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
        (_, Some(reason)) => log_detach(actor, thread, reason),
        (_, None) => {}
    }
    if !after_stop.is_empty() {
        start(actor, thread, after_stop);
    }
    super::super::sign_in::agent_ended(actor, thread, unresponsive);
    actor.release(StreamId::Thread(thread));
}

/// A detach reason as the log file may show it: its kind and exit code, never `stderr_tail`,
/// which is the agent's output (decision record 0047).
fn reason_kind(reason: &DetachReason) -> (&'static str, Option<i32>) {
    match reason {
        DetachReason::Exited { code, .. } => ("exited", *code),
        DetachReason::Closed => ("closed", None),
        DetachReason::Restarting => ("restarting", None),
        DetachReason::IdleStopped => ("idle_stopped", None),
        DetachReason::Unknown { .. } => ("unknown", None),
    }
}

/// Logs `AgentSessionDetached { reason }`.
pub(in super::super) fn log_detach(actor: &mut Actor, thread: ThreadId, reason: DetachReason) {
    let sent = Out::new(actor, thread, None).and_then(|mut out| {
        out.push(actor, AgentSessionDetached { reason })?;
        out.send(actor, Then::Nothing)
    });
    if let Err(error) = sent {
        tracing::warn!(%thread, error = %Cause(&error), "the detach was not logged");
    }
}
