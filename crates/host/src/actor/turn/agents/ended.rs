//! A thread's agent stopping and ending: why it was stopped ([`StopCause`]), the stop of an
//! agent whose connection is gone, the shutdown ladder's stop, and `Ended`: the detach it logs
//! and the turn the exit cut short.

use clauseo_protocol::{
    AgentSessionDetached, DetachReason, ErrorCode, ProtectionTier, StreamId, ThreadId,
};

use super::super::super::Actor;
use super::super::launch_checks;
use super::super::out::Out;
use super::super::{End, Then, TurnPhase, current, end, state, updates};
use super::{Agent, Stage, Waiting, fail_all, fail_waiting, start};
use crate::agent::{Ended, StopStep};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

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
    // The session is ending: its case-law relay refuses from now on (R24).
    launch_checks::relay::end_for(actor, thread);
}

/// The launch failed after the agent started: stop it and fail what waits for it. A launch whose
/// connection closed (its process exited; the sandbox's first-act fail-closed exits here) leaves
/// what waits to its exit report: `on_ended` fails it with the exit's own stop, so an Escapee
/// exit before the session ends its turn `LeftSafeWorkspace`. `Ended` always follows the stop.
pub(in super::super) fn launch_failed(actor: &mut Actor, thread: ThreadId, error: &Error) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    tracing::warn!(%thread, error = %Cause(error), "the agent did not start");
    agent.link.stop();
    agent.stage = Stage::Stopping;
    if matches!(error, Error::Acp(clauseo_acp::Error::Closed)) {
        agent.stop_cause = StopCause::Closed;
        return;
    }
    let waiting = std::mem::take(&mut agent.waiting);
    fail_all(actor, thread, waiting, error);
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
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.take()) else {
        return;
    };
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
        tier,
        ..
    } = *agent;
    let unresponsive = matches!(
        ended,
        Ended::Stopped {
            step: StopStep::Unresponsive,
            ..
        }
    );
    if let Ended::Stopped {
        step: StopStep::Unresponsive,
        report,
    } = &ended
    {
        let (kind, code) = reason_kind(report);
        tracing::warn!(%thread, kind, ?code, "the agent did not exit after its kill");
    }
    let (reason, escaped) = detach_of(ended, stop_cause, tier == Some(ProtectionTier::B));
    if escaped {
        escaped_from(actor, thread);
    }
    let (kind, code) = reason_kind(&reason);
    tracing::info!(
        target: LIFECYCLE,
        %thread,
        kind,
        ?code,
        denied_data_folder,
        "agent ended"
    );
    let exited = Error::refused(ErrorCode::AgentExited, "the agent exited");
    for item in waiting {
        fail_waiting(actor, thread, &item, &exited);
    }
    let detach = session.is_some().then_some(reason);
    let phase = current(actor, thread).map(|current| (current.phase, current.cancelled));
    match (phase, detach) {
        (Some((TurnPhase::Prompting, cancelled)), detach) => {
            let end = if escaped {
                End::left_safe_workspace(detach)
            } else if cancelled {
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
            let end = if escaped {
                End::left_safe_workspace(None)
            } else {
                End::cancelled(None)
            };
            end::end_turn(actor, thread, end);
        }
        (Some((TurnPhase::Ending, _)), Some(reason)) => {
            // Logged after the turn's `TurnEnded`, which is still to come.
            if let Some(current) = current(actor, thread) {
                current.end = Some(match current.end.take() {
                    // The Escapee exit is the turn's stop, whatever ended it first.
                    _ if escaped => End::left_safe_workspace(Some(reason)),
                    Some(mut end) => {
                        end.detach = Some(reason);
                        end
                    }
                    None => End::failed(ErrorCode::AgentExited, None, Some(reason)),
                });
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
    if escaped {
        // Nothing starts again for what came while it stopped (a mode or config change in the
        // closed window, the turn it held): it fails with the exit, the turn `LeftSafeWorkspace`.
        fail_all(actor, thread, after_stop, &exited);
    } else if !after_stop.is_empty() {
        start(actor, thread, after_stop);
    }
    super::super::sign_in::agent_ended(actor, thread, unresponsive);
    // A turn whose close waited for this exit ends now, with it (`exit_wait.rs`).
    super::super::exit_wait::agent_ended(actor, thread);
    actor.release(StreamId::Thread(thread));
}

/// Whether `thread`'s agent's exit is due and not yet reported: its connection closed, or it
/// is stopping because it had (`StopCause::Closed`). An agent the host asked to stop is not:
/// its detach is the host's close, whatever its exit.
pub(in super::super) fn exit_due(actor: &mut Actor, thread: ThreadId) -> bool {
    state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .is_some_and(|agent| match agent.stage {
            Stage::Stopping => agent.stop_cause == StopCause::Closed,
            Stage::Closed => true,
            Stage::Initializing | Stage::Loading | Stage::Attached => agent.link.closed(),
        })
}

/// The detach `ended` logs for an agent stopped for `cause`, and whether its process left the
/// Safe workspace. The Escapee exit is checked first, whoever stopped the agent: its code stays
/// in the detach, as the agent's own exit, not the host's close, restart or idle stop. Only an
/// agent launched while the computer had tier B (`tier_b`) ran under the sandbox runner.
fn detach_of(ended: Ended, cause: StopCause, tier_b: bool) -> (DetachReason, bool) {
    let report = match &ended {
        Ended::Exited(report) | Ended::Stopped { report, .. } => report,
    };
    let escaped = tier_b && left_safe_workspace(report);
    let reason = match ended {
        Ended::Exited(reason) => reason,
        Ended::Stopped { report, .. } if escaped => report,
        Ended::Stopped { report, .. } => match cause {
            StopCause::Asked => DetachReason::Closed,
            StopCause::Restarting => DetachReason::Restarting,
            StopCause::IdleStopped => DetachReason::IdleStopped,
            // It was gone before the stop: the exit is its own, not a close.
            StopCause::Closed => report,
        },
    };
    (reason, escaped)
}

/// `thread`'s agent left the Safe workspace: one warning with ids only (no path, no user name),
/// and its turn, whatever its phase, ends `LeftSafeWorkspace` with no `TurnFailed`, and is
/// never queued again (`Current::workspace`). A turn still accepting its message used no agent.
fn escaped_from(actor: &mut Actor, thread: ThreadId) {
    if let Some(entry) = actor.threads.get(&thread) {
        tracing::warn!(
            agent = %entry.row.agent,
            matter = %entry.row.matter,
            "an agent's process left the Safe workspace; its session was stopped"
        );
    }
    if let Some(current) = current(actor, thread)
        && current.phase != TurnPhase::Accepting
    {
        current.workspace = super::super::state::Workspace::Left;
    }
}

/// Whether the agent's process ended with the sandbox runner's (`clauseo-sandbox-run.exe`) Escapee
/// exit, so its session ends with `StopReason::LeftSafeWorkspace`.
fn left_safe_workspace(reason: &DetachReason) -> bool {
    matches!(reason, DetachReason::Exited { code: Some(code), .. } if is_escapee(*code))
}

/// Whether `code`, as `ExitStatus::code` gives it, is `RunExit::Escapee` (`0xE0C1_500F`).
/// Windows reports the runner's 32-bit exit code as an `i32`: its bytes are the runner's `u32`.
/// Built for the unit tests everywhere, which rewrite a fake agent's exit to that code.
#[cfg(any(windows, test))]
fn is_escapee(code: i32) -> bool {
    use clauseo_sandbox::windows::runner::RunExit;
    RunExit::from_code(code.cast_unsigned()) == Some(RunExit::Escapee)
}

/// Off Windows no sandbox runner starts the agent, and an exit code is 0 to 255: never.
#[cfg(not(any(windows, test)))]
const fn is_escapee(_code: i32) -> bool {
    false
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

#[cfg(test)]
mod tests;
