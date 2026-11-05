//! Automatic leaves (rulings 1 and 2, review H2): a split the host's check found, or a reuse S18
//! ended by itself (`ReusedFailed`, `ReusedChanged`, `ReusedSignedOut` from its status check).
//! Each runs on its own task ([`follow`]), never inside the pump, since the leave waits on what
//! the pump hands over.
//!
//! An automatic leave holds the agent's reads for its whole run. A try that does not finish
//! (`stop_agents` timed out, no `SignedOut` within 30 s, or a call failed) **keeps the host's
//! hold**: nothing of the agent launches on the old or changed login, or on a split copy of the
//! lawyer's file, and the questions sent meanwhile queue held and marked (P4d). `restart_agents`
//! is not called. The agent shows `Switching { since, stuck }` from that first end, and the leave
//! tries again after 5 s, 30 s, then every 5 minutes, until a try finishes or a new `SignedIn`
//! arrives. It never gives up. A try again that finishes ends `Switching` (the state after it
//! shows), drops the reads' hold, and calls `restart_agents`, so the held questions run once.
//! `stuck` turns true [`STUCK_AFTER`](super::hold::STUCK_AFTER) after `since`, a try again between
//! changing neither. The hold lives in memory only: quitting Clauseo clears it, and the next
//! start's status check finds the split or the changed login and leaves again.

use std::pin::pin;

use clauseo_agents::Found;
use clauseo_protocol::{AgentId, AgentKind, HostError};
use futures::StreamExt as _;
use futures::channel::oneshot;
use futures::future::{Either, select};

use super::super::backend::AgentsBackend;
use super::hold::lift;
use super::{NotLeft, Wait, attempt, login_changed, retry_after};

/// Runs each automatic leave queued, each on its own, until the queue ends.
pub(crate) async fn follow(backend: &AgentsBackend) {
    let Some(queue) = backend.wiring.leaves.queue() else {
        tracing::warn!("automatic leaves are followed already");
        return;
    };
    queue
        .for_each_concurrent(None, |leave| async move {
            let agent = &leave.agent;
            if let Err(error) = auto_leave(backend, agent, leave.why).await {
                tracing::info!(%agent, code = error.code().as_str(), "an automatic leave waits");
            }
        })
        .await;
}

/// One automatic leave of `agent`, `why` for S18: see the module docs. `command.conflict`
/// while another leave of the agent runs; this one is then kept beside it (`Leaves::defer`).
pub(crate) async fn auto_leave(
    backend: &AgentsBackend,
    agent: &AgentId,
    why: Option<Found>,
) -> Result<(), HostError> {
    let Some(kind) = AgentKind::of(agent) else {
        return Ok(());
    };
    let mut slot = match backend.wiring.leaves.begin(agent) {
        Ok(slot) => slot,
        Err(conflict) => {
            backend.wiring.leaves.defer(agent, why);
            return Err(conflict);
        }
    };
    tracing::info!(%agent, "leaving a login automatically");
    let signed_in = slot.on_signed_in();
    let reads = backend.wiring.readers.hold(kind).await;
    let retried = match attempt(backend, agent, kind, why).await {
        Ok(_) => false,
        Err(not_left) => {
            log_try(agent, &not_left);
            backend.compose.start_switching(agent);
            let tries = pin!(try_again(backend, agent, kind, why, signed_in));
            let stuck = pin!(async {
                backend.wiring.timer.after(Wait::Stuck).await;
                backend.compose.set_stuck(agent);
                std::future::pending::<()>().await;
            });
            let left = match select(tries, stuck).await {
                Either::Left((left, _)) => left,
                Either::Right(((), _)) => false,
            };
            backend.compose.end_switching(agent);
            // A new sign-in has lifted the hold already (`restart_agents` in the pump).
            left
        }
    };
    slot.left();
    drop(reads);
    if retried {
        lift(&*backend.host, agent).await;
    }
    login_changed(backend, agent).await;
    Ok(())
}

/// The tries again: `true` once one finishes, `false` once a new sign-in arrives.
async fn try_again(
    backend: &AgentsBackend,
    agent: &AgentId,
    kind: AgentKind,
    why: Option<Found>,
    mut signed_in: oneshot::Receiver<()>,
) -> bool {
    let mut n = 0;
    loop {
        let wait = backend.wiring.timer.after(Wait::Retry(retry_after(n)));
        if let Either::Right(_) = select(wait, &mut signed_in).await {
            break;
        }
        match attempt(backend, agent, kind, why).await {
            Ok(_) => return true,
            Err(not_left) => log_try(agent, &not_left),
        }
        if !matches!(signed_in.try_recv(), Ok(None)) {
            break;
        }
        n += 1;
    }
    tracing::info!(%agent, "a new sign-in ended an automatic leave");
    false
}

/// A try that did not finish, by its code.
fn log_try(agent: &AgentId, not_left: &NotLeft) {
    let code = not_left.code();
    tracing::info!(%agent, code = code.as_str(), "an automatic leave will try again");
}
