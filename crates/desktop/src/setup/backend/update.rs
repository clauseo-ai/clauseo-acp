//! An agent's update: one the lawyer added before (its `agents.<id>` entry is in the settings,
//! `refresh::installed_before`) whose install is not `Ready`, such as after an app update whose
//! new pins named a new install folder. S18 installs it again by itself at start
//! (`AgentsConfig::reinstall`), and every state goes out with `installed_before`, so no window
//! offers to add it.
//!
//! - **Its conversations wait.** The entry still names the old install, which is gone, so a
//!   launch would fail. From its first such state until its `Ready`, the host holds the agent
//!   ([`follow`]: `hold_for_update`, `StopFor::AgentInstall`): a message sent to it waits with
//!   the queue's `update` line, through a failed, waiting or cancelled download too, whose card
//!   offers Try again. At `Ready`, once the new entry is written, `restart_agents` lifts the
//!   hold and the waiting messages run, once. Only that agent is held.
//! - **Its sign-in stands.** At `Ready` the backend asks the status check, as at any install's
//!   end, and starts no sign-in. A sign-in press that meets that check before it answers
//!   ([`answered_by_ready_check`]) waits for it rather than replace it, and is answered by it
//!   when it says signed in: the lawyer's login, reused, used to fail such a press in a few
//!   milliseconds after an update.

use std::pin::pin;
use std::time::Duration;

use clauseo_agents::{InstallState, SigninState};
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::AgentId;
use futures::channel::oneshot;
use futures::future::select;

use super::AgentsBackend;
use crate::bridge::lock;

/// How long a sign-in press waits for the install's first status check: that check's two runs
/// (S18's 15 s and 20 s limits) and a margin. Past it the press goes on as before.
pub(crate) const READY_CHECK_WAIT: Duration = Duration::from_secs(40);

/// After `state` for `agent` is published (at `Ready`, after the entry's write): an agent added
/// before (`before`) whose install is neither `Ready` nor `Managed` is held in the host, once;
/// its `Ready` (or `Managed`) lifts the hold, so the messages kept for it run. A hold the host
/// refused is not marked, so the next state asks again; a lift the host refused is asked once
/// more, then logged.
pub(super) async fn follow(
    backend: &AgentsBackend,
    agent: &AgentId,
    state: &InstallState,
    before: bool,
) {
    let resting = matches!(state, InstallState::Ready { .. } | InstallState::Managed);
    if resting {
        if !lock(&backend.update_holds).remove(agent) {
            return;
        }
        for again in [false, true] {
            match backend.host.restart_agents(agent.clone()).await {
                Ok(()) => {
                    tracing::info!(%agent, "agent update ready: its conversations go on");
                    return;
                }
                Err(error) if !again => {
                    tracing::info!(%agent, code = error.code().as_str(), "update hold not lifted; again");
                }
                Err(error) => {
                    tracing::warn!(%agent, code = error.code().as_str(), "update hold not lifted");
                }
            }
        }
        return;
    }
    if !before || !lock(&backend.update_holds).insert(agent.clone()) {
        return;
    }
    match backend.host.hold_for_update(agent.clone()).await {
        Ok(_) => tracing::info!(%agent, "agent update: its conversations wait for it"),
        Err(error) => {
            lock(&backend.update_holds).remove(agent);
            tracing::warn!(%agent, code = error.code().as_str(), "agent update: no hold");
        }
    }
}

/// Whether `agent`'s sign-in press is answered by the status check its install's `Ready` asked
/// (`ready_checks`): while that check has not answered, the press waits for it, at most
/// [`READY_CHECK_WAIT`], rather than start a sign-in that would replace it. Signed in then: the
/// press is answered (its starting mark removed, as a stale press's is), and nothing starts.
/// Any other answer, or none in time: `false`, and the press goes on as before.
async fn answered_by_ready_check(backend: &AgentsBackend, agent: &AgentId) -> bool {
    let answer = {
        let checks = lock(&backend.ready_checks);
        if !checks.contains(agent) {
            return false;
        }
        let (tell, answer) = oneshot::channel();
        lock(&backend.ready_waiters)
            .entry(agent.clone())
            .or_default()
            .push(tell);
        answer
    };
    tracing::info!(%agent, "sign-in press: waits for the install's status check");
    let bound = pin!(AsyncIoTimer.after(READY_CHECK_WAIT));
    drop(select(answer, bound).await);
    let signed = matches!(
        backend.agents.signin_state(agent),
        Some(SigninState::SignedIn { .. })
    );
    if signed {
        tracing::info!(%agent, "sign-in press answered by the status check: signed in");
        backend.events.end_start(agent);
    }
    signed
}

/// Whether a sign-in press (`StartSignIn`, `UseDeviceCode`) for `agent` is answered with nothing
/// started: as stale (`switch::stale_start`: over Clauseo's own login, dropped at once), else by
/// the install's status check ([`answered_by_ready_check`]).
pub(super) async fn press_answered(backend: &AgentsBackend, agent: &AgentId) -> bool {
    super::switch::stale_start(backend, agent) || answered_by_ready_check(backend, agent).await
}

/// The status check the install's `Ready` asked for `agent` is over (a state, `SigninUnchanged`,
/// or not asked after all): its mark goes, and each press waiting for it is woken.
pub(super) fn ready_check_over(backend: &AgentsBackend, agent: &AgentId) {
    let mut checks = lock(&backend.ready_checks);
    checks.remove(agent);
    let waiting = lock(&backend.ready_waiters)
        .remove(agent)
        .unwrap_or_default();
    for tell in waiting {
        // A press that stopped waiting is gone already.
        let _gone = tell.send(());
    }
    drop(checks);
}
