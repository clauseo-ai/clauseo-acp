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
//! shows) and drops the reads' hold. Any try that finishes, the first or a try again, then
//! follows one rule ([`finish`]): `restart_agents` runs the held questions once when a stored
//! key, an endpoint or Clauseo's own login remains, else the hold stays for the next login and
//! the agent shows signed out.
//! `stuck` turns true [`STUCK_AFTER`](super::hold::STUCK_AFTER) after `since`, a try again between
//! changing neither. The hold lives in memory only: quitting Clauseo clears it, and the next
//! start's status check finds the split or the changed login and leaves again.
//!
//! **A new sign-in** (review A1) ends the leave, never the other way round. Before the first try
//! and each try again, and again right before S18's `stop_using_login` (under the sign-in gate),
//! the leave checks whether a sign-in runs or Clauseo's own login is signed in
//! ([`taken_over`](super::taken_over)), and whether a new `SignedIn` came since the leave began,
//! handled by the pump or still on its way (a "Use it" that finished as `SignedIn { Reused }`
//! as a try started, review H1′). A new login ends the leave, `Switching` with it. A running
//! sign-in only skips the try, never cancelled: the agent stays held, the sign-in shows over
//! `Switching`, and should it fail, `Switching` shows again and the next try runs, so no hold
//! is ever silent.
//! A new `SignedIn` the pump handles (the host told, the entry rewritten, `restart_agents`)
//! ends it too, whether it lands during the wait or during a try. The hold is then the new
//! login's to lift: the pump's `restart_agents` on its `SignedIn`, or the leave's own once more
//! when the pump had handled it (it may have come before the leave's last `stop_agents`).
//!
//! **A panic** (review A3) in a leave lifts its hold ([`HostHold::on_panic`]) and ends
//! `Switching`; [`follow`] catches it, so the thread's other followers (the settings, the
//! splits) and the other leaves go on. Release builds abort on a panic, so this holds only where
//! panics unwind (debug builds and the tests).

use std::panic::AssertUnwindSafe;
use std::pin::pin;
use std::sync::Arc;

use clauseo_agents::Found;
use clauseo_protocol::{AgentId, AgentKind, HostError};
use futures::channel::oneshot;
use futures::future::{Either, select};
use futures::{FutureExt as _, StreamExt as _};

use super::super::backend::AgentsBackend;
use super::after::{finish, own_login_shown};
use super::hold::HostHold;
use super::{NotLeft, Over, Wait, Whose, attempt, login_changed, retry_after, taken_over};

/// Runs each automatic leave queued, each on its own, until the queue ends. A leave that panics
/// is caught here: its hold lifted already, `Switching` ends, and the rest go on.
pub(crate) async fn follow(backend: &AgentsBackend) {
    let Some(queue) = backend.wiring.leaves.queue() else {
        tracing::warn!("automatic leaves are followed already");
        return;
    };
    queue
        .for_each_concurrent(None, |leave| async move {
            let agent = &leave.agent;
            let run = AssertUnwindSafe(auto_leave(backend, agent, leave.why));
            match run.catch_unwind().await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    let code = error.code().as_str();
                    tracing::info!(%agent, code, "an automatic leave waits");
                }
                Err(_panic) => {
                    backend.compose.end_switching(agent);
                    tracing::warn!(%agent, "an automatic leave panicked; its hold was lifted");
                }
            }
        })
        .await;
}

/// How an automatic leave ended.
enum End {
    /// A try finished, the first or a try again: [`finish`] lifts the hold or keeps it.
    Left,
    /// The pump handled a new `SignedIn`, `restart_agents` included.
    NewSignIn,
    /// A new login took the reused one's place ([`taken_over`]).
    TakenOver,
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
    if taken_over(backend, agent) == Some(Over::SignedIn) {
        tracing::info!(%agent, "a new sign-in took over: no automatic leave");
        slot.left();
        return Ok(());
    }
    tracing::info!(%agent, "leaving a login automatically");
    let signed_in = slot.on_signed_in();
    let reads = backend.wiring.readers.hold(kind).await;
    let hold = HostHold::on_panic(Arc::clone(&backend.host), agent.clone());
    let end = match attempt(backend, agent, kind, why, Whose::Auto).await {
        Ok(_) => End::Left,
        Err(NotLeft::NewSignIn) => End::NewSignIn,
        Err(NotLeft::TakenOver(Over::SignedIn)) => End::TakenOver,
        Err(not_left) => {
            log_try(agent, &not_left);
            backend.compose.start_switching(agent);
            let tries = pin!(try_again(backend, agent, kind, why, signed_in));
            let stuck = pin!(async {
                backend.wiring.timer.after(Wait::Stuck).await;
                backend.compose.set_stuck(agent);
                std::future::pending::<()>().await;
            });
            let end = match select(tries, stuck).await {
                Either::Left((end, _)) => end,
                Either::Right(((), _)) => End::NewSignIn,
            };
            backend.compose.end_switching(agent);
            end
        }
    };
    let leaves = &backend.wiring.leaves;
    slot.left();
    drop(reads);
    match end {
        End::Left => finish(backend, agent, hold).await,
        End::TakenOver if own_login_shown(backend, agent) || leaves.told_signed_in(agent) => {
            // The pump may have lifted it before this leave's last `stop_agents`.
            hold.lift(backend).await;
        }
        End::NewSignIn | End::TakenOver => hold.keep(),
    }
    login_changed(backend, agent).await;
    Ok(())
}

/// The tries again, until one finishes or a new sign-in ends them.
async fn try_again(
    backend: &AgentsBackend,
    agent: &AgentId,
    kind: AgentKind,
    why: Option<Found>,
    mut signed_in: oneshot::Receiver<()>,
) -> End {
    let mut n = 0;
    let end = loop {
        let wait = backend.wiring.timer.after(Wait::Retry(retry_after(n)));
        if let Either::Right(_) = select(wait, &mut signed_in).await {
            break End::NewSignIn;
        }
        match taken_over(backend, agent) {
            Some(Over::SignedIn) => break End::TakenOver,
            // Skipped, never cancelled: should the sign-in fail, the next try runs.
            Some(Over::Running) => log_try(agent, &NotLeft::TakenOver(Over::Running)),
            None => match attempt(backend, agent, kind, why, Whose::Auto).await {
                Ok(_) => return End::Left,
                Err(NotLeft::NewSignIn) => break End::NewSignIn,
                Err(NotLeft::TakenOver(Over::SignedIn)) => break End::TakenOver,
                Err(not_left) => log_try(agent, &not_left),
            },
        }
        if !matches!(signed_in.try_recv(), Ok(None)) {
            break End::NewSignIn;
        }
        n += 1;
    };
    tracing::info!(%agent, "a new sign-in ended an automatic leave");
    end
}

/// A try that did not finish, by its code.
fn log_try(agent: &AgentId, not_left: &NotLeft) {
    let code = not_left.code();
    tracing::info!(%agent, code = code.as_str(), "an automatic leave will try again");
}
