//! Leaving a reused login, and what follows a login change (decision record 0048, ruling 1; the
//! plan's piece B; the P5b section). One try of the leave order ([`attempt`]) runs:
//!
//! 1. the host's `stop_agents`, which holds the id until `restart_agents`;
//! 2. S18's `stop_using_login` (no-reuse and the home's link, one job), then S18's `SignedOut`,
//!    which the pump hands over ([`Waiters`]) within [`LEAVE_WAIT`](hold::LEAVE_WAIT);
//! 3. [`after_stop`]: `set_reused_login(false)`, `remove_login_links` (Codex), and the entry
//!    rewritten as `Own`, and the readers with it.
//!
//! Around it the leave holds the agent's limits reads (desktop keeps the [`ReadHold`]), then drops
//! them and sends `limits_login_changed`. One leave runs per agent ([`Leaves`]): a second one
//! answers `command.conflict`.
//!
//! **A lawyer's leave** ([`leave_reused`]: `StopUsingLogin`, `UseDifferentAccount`, a key or an
//! endpoint set on a reused login) that does not finish changes nothing: `restart_agents` lifts
//! the hold, the reads' guard drops, and the command answers `signin.agents_still_running`
//! (`stop_agents` timed out) or `signin.failed` (no `SignedOut` in time), both retryable. A
//! leave that finishes leaves the hold: `restart_agents` runs once the new login, key or
//! endpoint is in place, on the `SignedIn` the pump sees or after a key or an endpoint is stored.
//!
//! **An automatic leave** (`auto.rs`: a split, or a reuse S18 ended by itself) that does not
//! finish keeps the hold and tries again, showing `Switching`.

mod auto;
mod hold;
mod pump;
mod slots;
mod waiters;

use clauseo_agents::{Found, SigninState};
use clauseo_host::AgentsStopped;
use clauseo_protocol::{AgentId, AgentKind, ErrorCode, HostError, SignInSource};
use futures::channel::oneshot;
use futures::future::{Either, select};

#[cfg(test)]
pub(crate) use self::auto::auto_leave;
pub(crate) use self::auto::follow as follow_auto_leaves;
use self::hold::HostHold;
pub(crate) use self::hold::{LeaveTimer, Wait, retry_after};
pub(crate) use self::pump::signin_event;
pub(crate) use self::slots::Leaves;
use self::waiters::Put;
pub(crate) use self::waiters::{Token, Waiter, Waiters};
use super::backend::{AgentsBackend, signin_error};
use super::readers::ReadHold;

/// Why one try of the leave order did not finish; nothing of the login changed.
pub(crate) enum NotLeft {
    /// `stop_agents` answered `TimedOut`: an agent process outlived the wait.
    StillRunning,
    /// S18's `SignedOut` did not come within [`LEAVE_WAIT`](hold::LEAVE_WAIT).
    NotStopped,
    /// A call failed.
    Error(HostError),
}

impl NotLeft {
    /// The answer a lawyer's command gets.
    fn into_error(self, agent: &AgentId) -> HostError {
        match self {
            Self::StillRunning => HostError::new(
                ErrorCode::SigninAgentsStillRunning,
                format!("an agent of {agent} is still running; the login was not changed"),
            ),
            Self::NotStopped => HostError::new(
                ErrorCode::SigninFailed,
                format!("{agent}'s login you already had did not stop; try again"),
            ),
            Self::Error(error) => error,
        }
    }

    /// Its code, for the logs.
    fn code(&self) -> ErrorCode {
        match self {
            Self::StillRunning => ErrorCode::SigninAgentsStillRunning,
            Self::NotStopped => ErrorCode::SigninFailed,
            Self::Error(error) => error.code(),
        }
    }
}

/// A lawyer's leave of `agent`'s reused login: see the module docs.
pub(crate) async fn leave_reused(
    backend: &AgentsBackend,
    agent: &AgentId,
) -> Result<(), HostError> {
    let Some(kind) = AgentKind::of(agent) else {
        return Err(HostError::new(ErrorCode::CommandInvalid, "no such agent"));
    };
    let mut slot = backend.wiring.leaves.begin(agent)?;
    tracing::info!(%agent, "leaving a reused login");
    let reads = backend.wiring.readers.hold(kind).await;
    // Declared after `reads`, so on a panic it drops first: the host's hold, then the reads'.
    let hold = HostHold::new(std::sync::Arc::clone(&backend.host), agent.clone());
    match attempt(backend, agent, kind, None).await {
        Ok(first_error) => {
            hold.keep();
            slot.left();
            drop(reads);
            login_changed(backend, agent).await;
            first_error.map_or(Ok(()), Err)
        }
        Err(not_left) => {
            tracing::info!(%agent, code = not_left.code().as_str(), "a leave did not finish");
            hold.lift().await;
            drop(reads);
            Err(not_left.into_error(agent))
        }
    }
}

/// One try of the leave order (module docs, steps 1 to 3). `Ok` carries the first error of
/// step 3, whose steps all run.
pub(crate) async fn attempt(
    backend: &AgentsBackend,
    agent: &AgentId,
    kind: AgentKind,
    why: Option<Found>,
) -> Result<Option<HostError>, NotLeft> {
    match backend.host.stop_agents(agent.clone()).await {
        Ok(AgentsStopped::Exited) => {}
        Ok(AgentsStopped::TimedOut) => return Err(NotLeft::StillRunning),
        Err(error) => return Err(NotLeft::Error(error)),
    }
    let (tell, stopped) = oneshot::channel();
    let Some(token) = wait_on(backend, agent, Waiter::Leave(tell)) else {
        return Err(NotLeft::Error(HostError::new(
            ErrorCode::CommandConflict,
            format!("{agent}'s login is already being left"),
        )));
    };
    let _put = Put {
        waiters: &backend.wiring.waiters,
        agent: agent.clone(),
        token,
    };
    if let Err(error) = backend.agents.stop_using_login(agent, why) {
        return Err(NotLeft::Error(signin_error(&error)));
    }
    let wait = backend.wiring.timer.after(Wait::SignedOut);
    let ended = match select(stopped, wait).await {
        Either::Left((Ok(state), _)) => Some(state),
        Either::Left((Err(_), _)) | Either::Right(_) => None,
    };
    if !matches!(ended, Some(SigninState::SignedOut { .. })) {
        return Err(NotLeft::NotStopped);
    }
    Ok(after_stop(backend, agent, kind).await)
}

/// The steps after S18 stopped the reuse: `set_reused_login(false)`, `remove_login_links`
/// (Codex), the entry rewritten as `Own`. All run; the first error comes back.
pub(crate) async fn after_stop(
    backend: &AgentsBackend,
    agent: &AgentId,
    kind: AgentKind,
) -> Option<HostError> {
    let mut first_error = set_reused(backend, agent, false).await.err();
    if kind == AgentKind::Codex
        && let Err(error) = backend.host.remove_login_links(agent.clone()).await
    {
        tracing::warn!(%agent, code = error.code().as_str(), "matter links not removed");
        first_error.get_or_insert(error);
    }
    backend.write_entry_as(agent, SignInSource::Own).await;
    first_error
}

/// Puts `waiter` for `agent` with S18's latest state (see [`Waiters::put`]).
pub(crate) fn wait_on(backend: &AgentsBackend, agent: &AgentId, waiter: Waiter) -> Option<Token> {
    let latest = backend.agents.signin_state(agent);
    backend.wiring.waiters.put(agent, waiter, latest)
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
