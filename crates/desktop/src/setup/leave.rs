//! Leaving a reused login, and what follows a login change (decision record 0048, ruling 1; the
//! plan's piece B). [`leave_reused`] runs, in order:
//!
//! 1. the hold on the agent's limits reads (desktop keeps the [`ReadHold`]);
//! 2. the host's `stop_agents`, which holds the id until `restart_agents`. When it answers
//!    `TimedOut` (an agent process outlived the wait) the leave stops here, with nothing
//!    changed: `restart_agents` lifts the hold, the guard drops, and the command answers
//!    `signin.agents_still_running`, which may be retried;
//! 3. S18's `stop_using_login` (no-reuse and the home's link, one job), then S18's `SignedOut`,
//!    which the pump hands over ([`Waiters`]);
//! 4. `set_reused_login(false)`; 5. `remove_login_links` (Codex); 6. the entry rewritten as
//!    `Own`, and the readers with it; 7. the guard dropped; 8. `limits_login_changed`.
//!
//! `restart_agents` runs once the new login, key or endpoint is in place: on the `SignedIn` the
//! pump sees ([`signin_event`]), or after a key or an endpoint is stored. A
//! `SignedIn` whose source differs from the one the host was told sets it and rewrites the
//! entry first, so `UseFoundLogin` runs S18's call, then (on `SignedIn { Reused }`)
//! `set_reused_login(true)`, then the entry, then the restart.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use clauseo_agents::{Found, SigninState, Source};
use clauseo_host::AgentsStopped;
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::{AgentId, AgentKind, ErrorCode, HostError, SignInSource};
use futures::channel::oneshot;
use futures::future::{Either, select};

use super::backend::{AgentsBackend, signin_error};
use super::compose::is_running;
use super::readers::ReadHold;
use crate::bridge::lock;

/// How long the leave waits for S18's `SignedOut` after `stop_using_login`.
pub(crate) const LEAVE_WAIT: Duration = Duration::from_secs(30);

/// What waits on an agent's next resting S18 state.
pub(crate) enum Waiter {
    /// The leave order, for `stop_using_login`'s end.
    Leave(oneshot::Sender<SigninState>),
    /// `UseFoundLogin`: the hold on the reads, dropped once its end has been handled.
    Found(ReadHold),
}

/// Each agent's [`Waiter`], at most one.
#[derive(Default)]
pub(crate) struct Waiters(Mutex<BTreeMap<AgentId, Waiter>>);

impl Waiters {
    /// Waits on `agent`'s next resting state, in place of any waiter before.
    pub(crate) fn put(&self, agent: &AgentId, waiter: Waiter) {
        lock(&self.0).insert(agent.clone(), waiter);
    }

    /// Takes `agent`'s waiter.
    pub(crate) fn take(&self, agent: &AgentId) -> Option<Waiter> {
        lock(&self.0).remove(agent)
    }
}

/// 0048's leave order for `agent`'s reused login; `why` is the reason reuse stopped, if
/// one did (a split), else S18 writes no-reuse.
pub(crate) async fn leave_reused(
    backend: &AgentsBackend,
    agent: &AgentId,
    why: Option<Found>,
) -> Result<(), HostError> {
    let Some(kind) = AgentKind::of(agent) else {
        return Err(HostError::new(ErrorCode::CommandInvalid, "no such agent"));
    };
    tracing::info!(%agent, "leaving a reused login");
    let guard = backend.wiring.readers.hold(kind).await;
    match backend.host.stop_agents(agent.clone()).await {
        Ok(AgentsStopped::Exited) => {}
        Ok(AgentsStopped::TimedOut) => {
            abandon(backend, agent, guard).await;
            return Err(HostError::new(
                ErrorCode::SigninAgentsStillRunning,
                format!("an agent of {agent} is still running; the login was not changed"),
            ));
        }
        Err(error) => {
            abandon(backend, agent, guard).await;
            return Err(error);
        }
    }
    let (tell, stopped) = oneshot::channel();
    backend.wiring.waiters.put(agent, Waiter::Leave(tell));
    if let Err(error) = backend.agents.stop_using_login(agent, why) {
        drop(backend.wiring.waiters.take(agent));
        abandon(backend, agent, guard).await;
        return Err(signin_error(&error));
    }
    let wait = AsyncIoTimer.after(backend.wiring.leave_wait);
    let ended = match select(stopped, Box::pin(wait)).await {
        Either::Left((Ok(state), _)) => Some(state),
        Either::Left((Err(_), _)) | Either::Right(_) => None,
    };
    if !matches!(ended, Some(SigninState::SignedOut { .. })) {
        drop(backend.wiring.waiters.take(agent));
        abandon(backend, agent, guard).await;
        return Err(HostError::new(
            ErrorCode::SigninFailed,
            format!("{agent}'s login you already had did not stop; try again"),
        ));
    }
    let mut first_error = set_reused(backend, agent, false).await.err();
    if kind == AgentKind::Codex
        && let Err(error) = backend.host.remove_login_links(agent.clone()).await
    {
        tracing::warn!(%agent, code = error.code().as_str(), "matter links not removed");
        first_error.get_or_insert(error);
    }
    backend.write_entry_as(agent, SignInSource::Own).await;
    drop(guard);
    login_changed(backend, agent).await;
    first_error.map_or(Ok(()), Err)
}

/// A leave given up before anything changed: the host's hold lifted, then the reads'.
async fn abandon(backend: &AgentsBackend, agent: &AgentId, guard: ReadHold) {
    if let Err(error) = backend.host.restart_agents(agent.clone()).await {
        tracing::warn!(%agent, code = error.code().as_str(), "the agents' hold not lifted");
    }
    drop(guard);
}

/// Tells the host, and the login check, whether `agent`'s login is reused.
pub(crate) async fn set_reused(
    backend: &AgentsBackend,
    agent: &AgentId,
    reused: bool,
) -> Result<(), HostError> {
    backend.wiring.reused.set(agent, reused);
    backend
        .host
        .set_reused_login(agent.clone(), reused)
        .await
        .inspect_err(|error| {
            tracing::warn!(%agent, code = error.code().as_str(), "reused login not told");
        })
}

/// `restart_agents` for `agent`, logged by code when it fails.
pub(crate) async fn restart(backend: &AgentsBackend, agent: &AgentId) {
    if let Err(error) = backend.host.restart_agents(agent.clone()).await {
        tracing::warn!(%agent, code = error.code().as_str(), "agents not restarted");
    }
}

/// `limits_login_changed` for `agent`, logged by code when it fails.
pub(crate) async fn login_changed(backend: &AgentsBackend, agent: &AgentId) {
    if let Err(error) = backend.host.login_changed(agent.clone()).await {
        tracing::warn!(%agent, code = error.code().as_str(), "limits not told of the login");
    }
}

/// A key or an endpoint changed: the agents restart with it, and the limits read it.
pub(crate) async fn restart_for_login(backend: &AgentsBackend, agent: &AgentId) {
    restart(backend, agent).await;
    login_changed(backend, agent).await;
}

/// The reads' hold for `agent` across a key or an endpoint change, if it has a kind.
pub(crate) async fn hold_reads(backend: &AgentsBackend, agent: &AgentId) -> Option<ReadHold> {
    let kind = AgentKind::of(agent)?;
    Some(backend.wiring.readers.hold(kind).await)
}

/// One S18 sign-in state from the pump. A resting one goes to the agent's waiter first. A
/// `SignedIn` that is new (none before, or another email) is handled before it is shown:
/// a source that differs from the one the host was told is told and the entry rewritten
/// (a Codex login of Clauseo's own on another account has its matter links removed), the
/// reads' hold `UseFoundLogin` took drops, then `restart_agents` and
/// `limits_login_changed`. A `SignedIn` the leave waited on is the leave's.
pub(crate) async fn signin_event(backend: &AgentsBackend, agent: &AgentId, state: SigninState) {
    let before = backend.compose.s18(agent);
    let mut found_hold = None;
    if !is_running(&state) {
        match backend.wiring.waiters.take(agent) {
            Some(Waiter::Leave(tell)) => {
                // The leave stopped waiting: its own timeout said so.
                let _gone = tell.send(state.clone());
                backend.compose.signin(agent, state);
                return;
            }
            Some(Waiter::Found(hold)) => found_hold = Some(hold),
            None => {}
        }
    }
    if let SigninState::SignedIn { email, source, .. } = &state
        && !matches!(&before, Some(SigninState::SignedIn { email: was, .. }) if was == email)
    {
        let reused = *source == Source::Reused;
        if reused != backend.wiring.reused.get(agent) {
            // Logged inside; the entry and the restart go on.
            let _told = set_reused(backend, agent, reused).await;
            let source = if reused {
                SignInSource::Reused
            } else {
                SignInSource::Own
            };
            backend.write_entry_as(agent, source).await;
        } else if !reused
            && AgentKind::of(agent) == Some(AgentKind::Codex)
            && matches!(&before, Some(SigninState::SignedIn { .. }))
            && let Err(error) = backend.host.remove_login_links(agent.clone()).await
        {
            tracing::warn!(%agent, code = error.code().as_str(), "matter links not removed");
        }
        drop(found_hold.take());
        backend.compose.signin(agent, state);
        restart_for_login(backend, agent).await;
        return;
    }
    drop(found_hold);
    let signed_out = matches!(&before, Some(SigninState::SignedIn { .. }))
        && !is_running(&state)
        && !matches!(state, SigninState::SignedIn { .. });
    backend.compose.signin(agent, state);
    if signed_out {
        login_changed(backend, agent).await;
    }
}
