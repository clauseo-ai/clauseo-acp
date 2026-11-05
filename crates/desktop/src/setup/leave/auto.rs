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
//!
//! **A new sign-in** (review A1) ends the leave, never the other way round. Before the first try
//! and each try again, and again right before S18's `stop_using_login` (under the sign-in gate),
//! the leave checks whether a sign-in runs or Clauseo's own login is signed in
//! ([`taken_over`](super::taken_over)); then it asks S18 nothing more and ends, `Switching` with
//! it. A new `SignedIn` the pump handles (the host told, the entry rewritten, `restart_agents`)
//! ends it too, whether it lands during the wait or during a try. The hold is then the new
//! login's to lift: the pump's `restart_agents` on its `SignedIn`, or the leave's own when the
//! pump had shown it before the leave's last `stop_agents`. A running sign-in that fails leaves
//! the hold on until a login, a key or an endpoint is in place.
//!
//! **A panic** (review A3) in a leave lifts its hold ([`HostHold::on_panic`]) and ends
//! `Switching`; [`follow`] catches it, so the thread's other followers (the settings, the
//! splits) and the other leaves go on. Release builds abort on a panic, so this holds only where
//! panics unwind (debug builds and the tests).

use std::panic::AssertUnwindSafe;
use std::pin::pin;
use std::sync::Arc;

use clauseo_agents::{Found, SigninState, Source};
use clauseo_protocol::{AgentId, AgentKind, HostError};
use futures::channel::oneshot;
use futures::future::{Either, select};
use futures::{FutureExt as _, StreamExt as _};

use super::super::backend::AgentsBackend;
use super::hold::HostHold;
use super::{NotLeft, Wait, Whose, attempt, login_changed, retry_after, taken_over};

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
    /// Its first try finished: the hold stays until the new login is in place.
    First,
    /// A try again finished: `restart_agents` runs the held questions.
    Again,
    /// The pump handled a new `SignedIn`, `restart_agents` included.
    NewSignIn,
    /// A sign-in runs, or Clauseo's own login is signed in ([`taken_over`]).
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
    if taken_over(backend, agent) {
        tracing::info!(%agent, "a new sign-in took over: no automatic leave");
        slot.left();
        return Ok(());
    }
    tracing::info!(%agent, "leaving a login automatically");
    let signed_in = slot.on_signed_in();
    let reads = backend.wiring.readers.hold(kind).await;
    let hold = HostHold::on_panic(Arc::clone(&backend.host), agent.clone());
    let end = match attempt(backend, agent, kind, why, Whose::Auto).await {
        Ok(_) => End::First,
        Err(NotLeft::NewSignIn) => End::NewSignIn,
        Err(NotLeft::TakenOver) => End::TakenOver,
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
    slot.left();
    drop(reads);
    match end {
        End::Again => hold.lift().await,
        End::TakenOver if own_login_shown(backend, agent) => {
            // The pump may have lifted it before this leave's last `stop_agents`.
            hold.lift().await;
        }
        End::First | End::NewSignIn | End::TakenOver => hold.keep(),
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
        if taken_over(backend, agent) {
            break End::TakenOver;
        }
        match attempt(backend, agent, kind, why, Whose::Auto).await {
            Ok(_) => return End::Again,
            Err(NotLeft::NewSignIn) => break End::NewSignIn,
            Err(NotLeft::TakenOver) => break End::TakenOver,
            Err(not_left) => log_try(agent, &not_left),
        }
        if !matches!(signed_in.try_recv(), Ok(None)) {
            break End::NewSignIn;
        }
        n += 1;
    };
    tracing::info!(%agent, "a new sign-in ended an automatic leave");
    end
}

/// Whether the pump has shown Clauseo's own login for `agent`: it has handled its `SignedIn`.
fn own_login_shown(backend: &AgentsBackend, agent: &AgentId) -> bool {
    matches!(
        backend.compose.s18(agent),
        Some(SigninState::SignedIn {
            source: Source::Own,
            ..
        })
    )
}

/// A try that did not finish, by its code.
fn log_try(agent: &AgentId, not_left: &NotLeft) {
    let code = not_left.code();
    tracing::info!(%agent, code = code.as_str(), "an automatic leave will try again");
}
