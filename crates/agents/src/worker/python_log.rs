//! The Python runtime's install, for host.log: one line on the lifecycle target each time its
//! state changes kind. `Missing` says the download's size; a download says what it had and
//! what it needs as it starts and as it ends, never at each chunk; `Failed` says its cause and
//! code; `Ready` its version and how long the install took, when one ran. Never a path: the
//! states carry none but the failure's log, which is never read here.

use std::time::Duration;

use super::Ticks;
use crate::state::{FailureKind, PythonFailureKind, PythonState};

/// The target: host.log keeps info only from `clauseo_*::lifecycle` targets.
pub(crate) const LIFECYCLE: &str = "clauseo_agents::lifecycle";

/// The name the log gives `state`'s kind.
fn name(state: &PythonState) -> &'static str {
    match state {
        PythonState::Unavailable => "unavailable",
        PythonState::Missing { .. } => "missing",
        PythonState::Waiting { .. } => "waiting",
        PythonState::Downloading { .. } => "downloading",
        PythonState::Installing => "installing",
        PythonState::Ready { .. } => "ready",
        PythonState::Failed(_) => "failed",
    }
}

/// The error code a failure of `kind` reaches the windows with, or `none` for the two the
/// windows tell apart by their cause alone.
fn code(kind: PythonFailureKind) -> &'static str {
    match kind {
        PythonFailureKind::Blocked => FailureKind::Blocked.code(),
        PythonFailureKind::DiskFull => FailureKind::DiskFull.code(),
        PythonFailureKind::RunBlocked | PythonFailureKind::RunFailed => "none",
        PythonFailureKind::Failed => FailureKind::Failed.code(),
    }
}

/// Logs Python's state going from `old` to `new`, and keeps `began`: set as the install starts
/// working (waiting, downloading or installing), cleared once it rests again.
pub(super) fn changed(
    old: &PythonState,
    new: &PythonState,
    began: &mut Option<Duration>,
    ticks: &Ticks,
) {
    let from = name(old);
    let state = name(new);
    if let PythonState::Downloading { received, total } = old
        && from != state
    {
        tracing::info!(target: LIFECYCLE, received, total, "python runtime: download ended");
    }
    let working = matches!(
        new,
        PythonState::Waiting { .. } | PythonState::Downloading { .. } | PythonState::Installing
    );
    let took_ms = began.map(|at| ticks.since_ms(at));
    if working {
        began.get_or_insert_with(|| ticks.now());
    } else {
        *began = None;
    }
    if from == state {
        return;
    }
    let message = "python runtime: state changed";
    match new {
        PythonState::Missing { download_bytes } => {
            tracing::info!(target: LIFECYCLE, from, state, download_bytes, "{message}");
        }
        PythonState::Waiting { reason, .. } => {
            let reason = reason.word();
            tracing::info!(target: LIFECYCLE, from, state, reason, "{message}");
        }
        PythonState::Downloading { received, total } => {
            tracing::info!(target: LIFECYCLE, from, state, received, total, "{message}");
        }
        PythonState::Ready { version } => {
            // No `took_ms` when it was found installed, with no install run by this process.
            if let Some(took_ms) = took_ms {
                tracing::info!(target: LIFECYCLE, from, state, %version, took_ms, "{message}");
            } else {
                tracing::info!(target: LIFECYCLE, from, state, %version, "{message}");
            }
        }
        PythonState::Failed(failure) => {
            let cause = failure.kind.word();
            let code = code(failure.kind);
            let blocked_by = failure.blocked_by.map_or("none", |by| by.word());
            tracing::info!(target: LIFECYCLE, from, state, cause, code, blocked_by, "{message}");
        }
        PythonState::Unavailable | PythonState::Installing => {
            tracing::info!(target: LIFECYCLE, from, state, "{message}");
        }
    }
}

#[cfg(test)]
mod tests;
