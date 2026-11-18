//! The confirm before a Codex sign-in that would replace Clauseo's own login: `codex login`
//! revokes and deletes the login it finds before it starts, so Clauseo never starts one over a
//! signed-in account without the lawyer's say. S18 refuses such a login itself
//! (`SigninError::OwnLoginPresent`) unless it is told the lawyer said yes.

use clauseo_agents::{Method, OwnLogin, SigninError, SigninState};
use clauseo_protocol::{AgentId, AgentKind};

use super::AgentsBackend;
use super::seam::SetupAgents;
use crate::setup::confirm::Ask;

/// Whether a sign-in may start for `agent` (named `name`), and with what: `Some(None)` when it
/// replaces nothing; `Some(Some(login))` when it would replace Clauseo's own Codex login
/// (`Agents::own_login`, a stat of the files, so it holds at start-up too) and the lawyer said
/// yes to [`Ask::SwitchAccount`] about `login`, stat'ed before the ask; `None` when they
/// declined. The ask names the plan (`ChatGPT`, `instructions::plan_name`), else `name`.
/// A decline starts
/// nothing and removes the starting mark (`SetupEvents::end_start`), which no sign-in state
/// would otherwise end; the caller answers `Done` then, as a Cancel should: there is no
/// refusal to show.
pub(super) async fn switch_first(
    backend: &AgentsBackend,
    agent: &AgentId,
    name: &str,
) -> Option<Option<OwnLogin>> {
    let Some(login) = backend.agents.own_login(agent) else {
        return Some(None);
    };
    let plan = AgentKind::of(agent).map(clauseo_profile::instructions::plan_name);
    let name = plan.unwrap_or(name).to_owned();
    if backend.confirm.confirm(Ask::SwitchAccount { name }).await {
        return Some(Some(login));
    }
    tracing::info!(%agent, "sign-in not started: the lawyer kept the account");
    backend.events.end_start(agent);
    None
}

/// Whether a `StartSignIn` or `UseDeviceCode` for `agent` is stale, and dropped unasked: Clauseo's
/// own Codex login is there and S18 is `Checking` it or `SignedIn`. A signed-in card offers
/// "Sign out", not "Sign in", so such a press is no request to sign out: it is "Add `ChatGPT`"'s
/// sign-in at the install's `Ready`, whichever of the window and the check is first, or a
/// second press. It answers `Done` as a decline does, its starting mark removed.
/// `UseDifferentAccount` is never stale: it asks (`switch_first`).
pub(super) fn stale_start(backend: &AgentsBackend, agent: &AgentId) -> bool {
    let signed = matches!(
        backend.agents.signin_state(agent),
        Some(SigninState::Checking | SigninState::SignedIn { .. })
    );
    if !signed || backend.agents.own_login(agent).is_none() {
        return false;
    }
    tracing::info!(%agent, "sign-in not started: Clauseo's own login is signed in or checked");
    backend.events.end_start(agent);
    true
}

/// `Agents::sign_in`, or with `switch` (the lawyer's yes) `sign_in_switching`.
pub(super) fn sign_in(
    agents: &dyn SetupAgents,
    agent: &AgentId,
    method: Method,
    switch: bool,
) -> Result<(), SigninError> {
    if switch {
        agents.sign_in_switching(agent, method)
    } else {
        agents.sign_in(agent, method)
    }
}

/// `Agents::use_different_account`, or with `switch` its `_switching` form.
pub(super) fn use_different(
    agents: &dyn SetupAgents,
    agent: &AgentId,
    switch: bool,
) -> Result<(), SigninError> {
    if switch {
        agents.use_different_account_switching(agent)
    } else {
        agents.use_different_account(agent)
    }
}
