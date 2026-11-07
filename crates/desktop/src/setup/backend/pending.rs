//! A sign-in pressed before its agent is installed (`StartSignIn`, `UseDeviceCode`,
//! `UseDifferentAccount`): the press is that agent's choice, so it starts or joins the install
//! and is kept here as a pending press (the agent, the method, Claude's console flag), shown as
//! `Queued`; the command answers `Done`.
//!
//! - **On the install's `Ready`** the pump runs it ([`run_press`]) through the
//!   same gate and checks as a direct press (`signin_gate`, `refuse_reused`, `refuse_start`),
//!   checked then: a key, an endpoint or a policy that bars it now refuses it, with the
//!   refusal logged by its code and the sign-in state back to S18's. That holds for Codex's
//!   switch check too: a press keeps the lawyer's yes to `Ask::SwitchAccount` (`switch.rs`),
//!   asked when pressed; a press kept without one that finds Clauseo's own login at `Ready`
//!   is refused (`SigninError::OwnLoginPresent`) and dropped, never asked then, so no
//!   `codex login` starts over that login unasked.
//! - **Kept** while the install downloads, installs or waits offline to try again (`Waiting`),
//!   and when it ends `Failed`: the failed view shows with its Retry, and a retry that ends
//!   `Ready` carries on to the sign-in the lawyer asked for.
//! - **Dropped** when the install ends `Paused`, when the agent turns out `Managed`, on
//!   `CancelSignIn` (the install carries on), on `SignOut`, and when a key or an endpoint takes
//!   over (`AgentsBackend::end_sign_in`).
//! - One per agent: a second press replaces the first (`Setup` lets it past the sign-in join).
//! - Memory only: nothing of it outlives a quit.
//!
//! S18 itself queues a sign-in until the install is `Ready`, but runs it then with no new check
//! and keeps it through a failure, so desktop never hands it a sign-in for an agent not installed.

use clauseo_agents::{InstallState, Method, SigninError, SigninState};
use clauseo_protocol::{AgentId, HostError};

use super::{AgentsBackend, signin_error, switch};
use crate::bridge::lock;

/// A sign-in press kept until the agent's install is `Ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Press {
    /// `StartSignIn` (its plan login, by the console flag) or `UseDeviceCode`.
    SignIn(Method),
    /// `UseDifferentAccount`.
    DifferentAccount,
}

/// A kept press and whether the lawyer said yes to replacing Clauseo's own Codex login when it
/// was pressed (`switch.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kept {
    pub(crate) press: Press,
    pub(crate) switch: bool,
}

/// Whether an agent in install `state` can sign in now: installed, or the firm's runtime.
fn installed(state: Option<&InstallState>) -> bool {
    matches!(
        state,
        Some(InstallState::Ready { .. } | InstallState::Managed)
    )
}

/// A press for `agent` whose checks passed, with the gate held: when the agent is not
/// installed, or a press is still kept for it, keeps `press` with `switch`, the lawyer's yes
/// (replacing the one kept before it), shows `Queued` and starts or joins the install; `true`
/// then. `false` when the agent is installed and nothing is kept, so the caller starts the
/// sign-in at once.
///
/// A kept press is replaced even once the install is `Ready`: S18 says `Ready` before the
/// pump has handled it (it writes the entry first), and a press started directly then would be
/// restarted by the older press at [`run_press`]. Replaced, the latest is the one that runs
/// (the install of a ready agent says `Ready` again, which the pump handles too).
pub(super) fn keep_press(
    backend: &AgentsBackend,
    agent: &AgentId,
    press: Press,
    switch: bool,
) -> bool {
    let kept = lock(&backend.pending).contains_key(agent);
    if !kept && installed(backend.agents.state(agent).as_ref()) {
        return false;
    }
    let replaced = lock(&backend.pending)
        .insert(agent.clone(), Kept { press, switch })
        .is_some();
    tracing::info!(%agent, method = ?press, switch, replaced, "sign-in pending: installing first");
    backend.compose.signin(agent, SigninState::Queued);
    backend.agents.install(agent);
    true
}

/// A press for `agent` whose checks passed, with the gate held: kept ([`keep_press`]), or
/// started at once with `switch`, the lawyer's yes (`switch.rs`).
pub(super) fn keep_or_start(
    backend: &AgentsBackend,
    agent: &AgentId,
    press: Press,
    switch: bool,
) -> Result<(), HostError> {
    if keep_press(backend, agent, press, switch) {
        return Ok(());
    }
    call(backend, agent, press, switch)?;
    tracing::info!(%agent, method = ?press, "sign-in started");
    backend.replaced(agent);
    Ok(())
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
    let Some(Kept { press, .. }) = lock(&backend.pending).remove(agent) else {
        return false;
    };
    tracing::info!(%agent, method = ?press, why, "pending sign-in dropped");
    show_s18(backend, agent);
    true
}

/// `SignOut`, refused over the lawyer's own login: a press kept through a failed install goes
/// first, with the gate held, so a later Retry's `Ready` never starts a sign-in the lawyer has
/// since signed out of; then the sign-out.
pub(super) fn sign_out(backend: &AgentsBackend, agent: &AgentId) -> Result<(), HostError> {
    backend.refuse_reused(agent)?;
    {
        let _gate = lock(&backend.signin_gate);
        drop_press(backend, agent, "signed out");
    }
    backend
        .agents
        .sign_out(agent)
        .map_err(|error| signin_error(&error))?;
    backend.replaced(agent);
    Ok(())
}

/// The pump's install `state` for `agent` other than `Ready`: `Paused` and `Managed` drop a
/// pending press; any other keeps it, `Failed` too (a retry's `Ready` runs it).
pub(super) fn follow_press(backend: &AgentsBackend, agent: &AgentId, state: &InstallState) {
    let why = match state {
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
    let Some(kept) = lock(&backend.pending).remove(agent) else {
        return false;
    };
    let press = kept.press;
    match start_press(backend, agent, kept) {
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

/// `kept`'s checks and its S18 call, as the direct press's. Without the lawyer's yes, a login
/// that would replace Clauseo's own is refused here as S18 refuses it, so the fake `Agents` of
/// the tests needs no copy of S18's rule.
fn start_press(backend: &AgentsBackend, agent: &AgentId, kept: Kept) -> Result<(), HostError> {
    backend.refuse_reused(agent)?;
    backend.refuse_start(agent)?;
    if !kept.switch && backend.agents.replaces_own_login(agent) {
        return Err(signin_error(&SigninError::OwnLoginPresent(agent.clone())));
    }
    call(backend, agent, kept.press, kept.switch)
}

/// `press`'s S18 call, the confirmed form with `switch`.
fn call(
    backend: &AgentsBackend,
    agent: &AgentId,
    press: Press,
    switch: bool,
) -> Result<(), HostError> {
    match press {
        Press::SignIn(method) => switch::sign_in(&*backend.agents, agent, method, switch),
        Press::DifferentAccount => switch::use_different(&*backend.agents, agent, switch),
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
