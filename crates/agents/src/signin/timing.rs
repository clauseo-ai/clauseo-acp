//! The start-up's timing marks for host.log (lifecycle info lines, which the file keeps): an
//! install's `Ready` sent, a status check's start, and each status run's start and end with its
//! milliseconds. No path, email or output: the agent, the run's kind (`own`, `detect`,
//! `reused`) and how it ended. The first [`LINES`] lines of a run, so the background re-checks
//! never fill the log.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use clauseo_protocol::AgentId;

use crate::config::Clock;
use crate::signin::env::RunKind;

/// The target: host.log keeps info only from `clauseo_*::lifecycle` targets.
pub(crate) const TARGET: &str = "clauseo_agents::signin::lifecycle";

/// How many lines a run logs.
const LINES: u32 = 100;

/// The lines logged so far.
static LOGGED: AtomicU32 = AtomicU32::new(0);

/// Whether one more line fits under the cap.
fn room() -> bool {
    LOGGED.fetch_add(1, Ordering::Relaxed) < LINES
}

/// `agent`'s install `Ready` was sent.
pub(crate) fn ready_sent(agent: &AgentId) {
    if room() {
        tracing::info!(target: TARGET, agent = agent.as_str(), "install ready sent");
    }
}

/// `agent`'s status check job started.
pub(crate) fn check_started(agent: &AgentId) {
    if room() {
        tracing::info!(target: TARGET, agent = agent.as_str(), "sign-in status check started");
    }
}

/// The word for a run's kind in the log.
pub(crate) fn kind_word(kind: RunKind<'_>) -> &'static str {
    match kind {
        RunKind::Own { .. } => "own",
        RunKind::Detect => "detect",
        RunKind::Reused { .. } => "reused",
    }
}

/// A status run of `kind` for `agent` starts; give its end [`run_ended`] the reading of
/// `clock` this returns.
pub(crate) fn run_started(agent: &AgentId, kind: RunKind<'_>, clock: &dyn Clock) -> Duration {
    if room() {
        tracing::info!(target: TARGET, agent = agent.as_str(), run = kind_word(kind),
            "sign-in status run started");
    }
    clock.monotonic()
}

/// The status run of `kind` that started at `began` ended as `ended` says (`exited`,
/// `timed_out`, `cancelled`, `not_started`).
pub(crate) fn run_ended(
    agent: &AgentId,
    kind: RunKind<'_>,
    clock: &dyn Clock,
    began: Duration,
    ended: &'static str,
) {
    if room() {
        let took = clock.monotonic().saturating_sub(began);
        let took_ms = u64::try_from(took.as_millis()).unwrap_or(u64::MAX);
        tracing::info!(target: TARGET, agent = agent.as_str(), run = kind_word(kind), took_ms,
            ended, "sign-in status run ended");
    }
}
