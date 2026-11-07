//! A sign-in pressed before its agent is installed (`StartSignIn`, `UseDeviceCode`,
//! `UseDifferentAccount`): the press is that agent's choice, so it starts or joins the install
//! and is kept here as a pending press (the agent, the method, Claude's console flag), shown as
//! `Queued`; the command answers `Done`.
//!
//! - **On the install's `Ready`** the pump runs it ([`run_press`]) through the
//!   same gate and checks as a direct press (`signin_gate`, `refuse_reused`, `refuse_start`),
//!   checked then: a key, an endpoint or a policy that bars it now refuses it, with the
//!   refusal logged by its code and the sign-in state back to S18's.
//! - **Kept** while the install downloads, installs or waits offline to try again (`Waiting`).
//! - **Dropped** when the install ends `Failed` or `Paused` (the failed view shows; a fresh
//!   press is needed), when the agent turns out `Managed`, on `CancelSignIn` (the install
//!   carries on), and when a key or an endpoint takes over (`AgentsBackend::end_sign_in`).
//! - One per agent: a second press replaces the first (`Setup` lets it past the sign-in join).
//! - Memory only: nothing of it outlives a quit.
//!
//! S18 itself queues a sign-in until the install is `Ready`, but runs it then with no new check
//! and keeps it through a failure, so desktop never hands it a sign-in for an agent not installed.

use clauseo_agents::{InstallState, Method, SigninState};
use clauseo_protocol::{AgentId, HostError};

use super::{AgentsBackend, signin_error};
use crate::bridge::lock;

/// A sign-in press kept until the agent's install is `Ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Press {
    /// `StartSignIn` (its plan login, by the console flag) or `UseDeviceCode`.
    SignIn(Method),
    /// `UseDifferentAccount`.
    DifferentAccount,
}

/// Whether an agent in install `state` can sign in now: installed, or the firm's runtime.
fn installed(state: Option<&InstallState>) -> bool {
    matches!(
        state,
        Some(InstallState::Ready { .. } | InstallState::Managed)
    )
}

/// A press for `agent` whose checks passed, with the gate held: when the agent is not
/// installed, or a press is still kept for it, keeps `press` (replacing the one kept before
/// it), shows `Queued` and starts or joins the install; `true` then. `false` when the agent
/// is installed and nothing is kept, so the caller starts the sign-in at once.
///
/// A kept press is replaced even once the install is `Ready`: S18 says `Ready` before the
/// pump has handled it (it writes the entry first), and a press started directly then would be
/// restarted by the older press at [`run_press`]. Replaced, the latest is the one that runs
/// (the install of a ready agent says `Ready` again, which the pump handles too).
pub(super) fn keep_press(backend: &AgentsBackend, agent: &AgentId, press: Press) -> bool {
    let kept = lock(&backend.pending).contains_key(agent);
    if !kept && installed(backend.agents.state(agent).as_ref()) {
        return false;
    }
    let replaced = lock(&backend.pending)
        .insert(agent.clone(), press)
        .is_some();
    tracing::info!(%agent, method = ?press, replaced, "sign-in pending: installing first");
    backend.compose.signin(agent, SigninState::Queued);
    backend.agents.install(agent);
    true
}

/// Whether a `StartSignIn` that went past the sign-in join (`Setup` saw a kept press) finds,
/// with the gate held, no press kept any more but a sign-in running: [`run_press`] started the
/// kept one meanwhile, so this press joins it rather than restarting it. Only such a press gets
/// here with a running state: any other was joined in `Setup`.
pub(super) fn joins_started(backend: &AgentsBackend, agent: &AgentId) -> bool {
    !lock(&backend.pending).contains_key(agent) && backend.events.sign_in_running(agent)
}

/// Drops `agent`'s pending press, if any, for `why` (logged), with the gate held: the
/// sign-in state goes back to S18's. Whether one was dropped.
pub(super) fn drop_press(backend: &AgentsBackend, agent: &AgentId, why: &'static str) -> bool {
    let Some(press) = lock(&backend.pending).remove(agent) else {
        return false;
    };
    tracing::info!(%agent, method = ?press, why, "pending sign-in dropped");
    show_s18(backend, agent);
    true
}

/// The pump's install `state` for `agent` other than `Ready`: `Failed`, `Paused` and
/// `Managed` drop a pending press; any other keeps it.
pub(super) fn follow_press(backend: &AgentsBackend, agent: &AgentId, state: &InstallState) {
    let why = match state {
        InstallState::Failed(_) => "the install failed",
        InstallState::Paused => "the install paused",
        InstallState::Managed => "the agent is the firm's",
        _ => return,
    };
    let _gate = lock(&backend.signin_gate);
    drop_press(backend, agent, why);
}

/// `agent`'s install is `Ready`: its pending press, if any, runs now as a direct press does,
/// checked now. Whether a sign-in started.
pub(super) fn run_press(backend: &AgentsBackend, agent: &AgentId) -> bool {
    let _gate = lock(&backend.signin_gate);
    let Some(press) = lock(&backend.pending).remove(agent) else {
        return false;
    };
    match start_press(backend, agent, press) {
        Ok(()) => {
            tracing::info!(%agent, method = ?press, "sign-in started: pressed before the install");
            backend.replaced(agent);
            true
        }
        Err(error) => {
            tracing::warn!(
                %agent,
                code = error.code().as_str(),
                "a sign-in pressed before the install was refused at its end"
            );
            show_s18(backend, agent);
            false
        }
    }
}

/// `press`'s checks and its S18 call, as the direct press's.
fn start_press(backend: &AgentsBackend, agent: &AgentId, press: Press) -> Result<(), HostError> {
    backend.refuse_reused(agent)?;
    backend.refuse_start(agent)?;
    match press {
        Press::SignIn(method) => backend.agents.sign_in(agent, method),
        Press::DifferentAccount => backend.agents.use_different_account(agent),
    }
    .map_err(|error| signin_error(&error))
}

/// The sign-in state back to S18's latest (signed out when it has none) in place of the
/// pending press's `Queued`.
fn show_s18(backend: &AgentsBackend, agent: &AgentId) {
    let state = backend
        .agents
        .signin_state(agent)
        .unwrap_or(SigninState::SignedOut { found: None });
    backend.compose.signin(agent, state);
}
