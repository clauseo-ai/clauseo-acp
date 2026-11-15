//! The start's bound: an agent whose session is not ready [`START_WAIT`] after its launch
//! (`initialize` through `session/new`, `session/load` or `session/resume`, and a Codex or
//! Claude session's checks before its attach) has failed to start. It is stopped through the
//! shutdown ladder and what waits for it fails `agent.launch_failed`, its question kept; the
//! offline probe may make that `agent.start_offline` (`offline/start.rs`). Without the bound a
//! start that never answers left the turn starting until the lawyer pressed Stop.
//!
//! The limit: the slowest start measured in the repo is Codex's cold start on a Windows VM,
//! 3.7 s (`agents/src/signin/codex.rs`); claude-agent-acp 0.89.1's session-start phases total
//! milliseconds (`start_failure.rs`). A `session/load` replays the whole conversation and a
//! cold Windows start behind a scanner can be many times slower, so the bound is generous: as
//! long as the folder preflight waits for a lawyer's answer (`folder_access::PREFLIGHT_WAIT`).

use std::time::Duration;

use clauseo_protocol::{ErrorCode, ThreadId};

use super::super::{TimerKind, TurnTimer, state};
use super::{Stage, launch_failed};
use crate::actor::Actor;
use crate::error::Error;

/// How long an agent may take from its launch to its attached session.
pub(crate) const START_WAIT: Duration = Duration::from_secs(120);

/// The detail of a start that ran out of [`START_WAIT`], for logs; `offline/start.rs` reads it
/// to ask the probe.
pub(crate) const START_TIMED_OUT: &str = "the agent did not finish starting within 120 s";

/// Arms the bound for `thread`'s agent `id`, just launched.
pub(super) fn arm(actor: &Actor, thread: ThreadId, id: u64) {
    #[cfg(test)]
    if actor.turn_path.no_start_bound {
        return;
    }
    let timer = TurnTimer {
        thread,
        kind: TimerKind::Start(id),
    };
    let _handle = actor.arm_timer(START_WAIT, timer);
}

/// The bound of `thread`'s agent `id` ran out: when that agent is still starting, its launch
/// fails. An agent attached, stopping or gone meanwhile is left as it is.
pub(in super::super) fn on_timer(actor: &mut Actor, thread: ThreadId, id: u64) {
    let starting = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .is_some_and(|agent| {
            agent.id == id && matches!(agent.stage, Stage::Initializing | Stage::Loading)
        });
    if !starting {
        return;
    }
    tracing::warn!(%thread, wait_s = START_WAIT.as_secs(), "the agent did not finish starting");
    launch_failed(
        actor,
        thread,
        &Error::refused(ErrorCode::AgentLaunchFailed, START_TIMED_OUT),
    );
}
