//! The confirm before a Codex sign-in that would replace Clauseo's own login: `codex login`
//! revokes and deletes the login it finds before it starts, so Clauseo never starts one over a
//! signed-in account without the lawyer's say. S18 refuses such a login itself
//! (`SigninError::OwnLoginPresent`) unless it is told the lawyer said yes.

use clauseo_agents::{Method, SigninError};
use clauseo_protocol::AgentId;

use super::AgentsBackend;
use super::seam::SetupAgents;
use crate::setup::confirm::Ask;

/// Whether a sign-in may start for `agent` (named `name`), and with what: `Some(false)` when it
/// replaces nothing; `Some(true)` when it would replace Clauseo's own Codex login
/// (`Agents::replaces_own_login`, read from the files, so it holds at start-up too) and the
/// lawyer said yes to [`Ask::SwitchAccount`]; `None` when they declined. A decline starts
/// nothing and removes the starting mark (`SetupEvents::end_start`), which no sign-in state
/// would otherwise end; the caller answers `Done` then, as a Cancel should: there is no
/// refusal to show.
pub(super) async fn switch_first(
    backend: &AgentsBackend,
    agent: &AgentId,
    name: &str,
) -> Option<bool> {
    if !backend.agents.replaces_own_login(agent) {
        return Some(false);
    }
    let name = name.to_owned();
    if backend.confirm.confirm(Ask::SwitchAccount { name }).await {
        return Some(true);
    }
    tracing::info!(%agent, "sign-in not started: the lawyer kept the account");
    backend.events.end_start(agent);
    None
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
