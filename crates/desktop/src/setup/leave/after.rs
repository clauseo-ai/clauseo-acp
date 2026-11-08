//! What follows a leave that finished (the orchestrator's ruling on 0048's leave flow), one rule
//! for every path: an automatic leave's first try or a try again, a lawyer's leave, and a
//! lawyer's leave S18's stop job ended after it gave up.
//!
//! When something usable remains for the agent ([`usable_after_leave`]): a stored API key, an
//! endpoint or a provider (the lawyer's or the firm's policy's), or Clauseo's own login signed
//! in, the hold lifts at once (`restart_agents`), and the held questions run on it. Otherwise
//! the hold stays for the next login, whose `SignedIn` the pump restarts on, or for a key or an
//! endpoint, whose command restarts once it is stored. Never a silent hold: with nothing usable
//! [`compose`](super::super::compose::compose) has no `Firm` or `ApiKey` line to show, so the
//! window gets S18's state after the leave, `SignedOut`.
//!
//! A lawyer's leave that is the first step of a key or an endpoint ([`After::Command`]) leaves
//! the restart to its command, which runs it once the key or the endpoint is in place, or by
//! this rule should the store fail: one restart, never one before the new key or endpoint.

use clauseo_agents::{SigninState, Source};
use clauseo_protocol::AgentId;

use super::super::backend::AgentsBackend;
use super::hold::HostHold;
use super::restart;

/// Who restarts the agents after a lawyer's leave that finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum After {
    /// The leave, by [`usable_after_leave`]: `StopUsingLogin`, `UseDifferentAccount`.
    Rule,
    /// The command it is the first step of, once its key or endpoint is stored; by
    /// [`usable_after_leave`] should the store fail ([`restart_if_usable`]), or the leave end
    /// with an error, since the command then stops there.
    Command,
}

/// Whether `agent` has a login the agents may restart on once its reused login was left: a
/// stored API key, an endpoint or a provider (what bars a sign-in, `admit::sign_in_bar`, and
/// shows `ApiKey` or `Firm` over S18's state), or Clauseo's own login, as the pump showed it.
pub(crate) fn usable_after_leave(backend: &AgentsBackend, agent: &AgentId) -> bool {
    backend.keys.has(agent)
        || backend.compose.sign_in_bar(agent).is_some()
        || own_login_shown(backend, agent)
}

/// A leave of `agent` finished: its `hold` lifts when something usable remains, else stays for
/// the next login.
pub(crate) async fn finish(backend: &AgentsBackend, agent: &AgentId, hold: HostHold) {
    if usable_after_leave(backend, agent) {
        tracing::info!(%agent, "a leave finished: restarting on the login that remains");
        hold.lift(backend).await;
    } else {
        tracing::info!(%agent, "a leave finished: held for the next login");
        hold.keep();
    }
}

/// A command that left `agent`'s reused login did not store its key or endpoint: the hold
/// lifts by the rule, as a leave's own would ([`finish`]).
pub(crate) async fn restart_if_usable(backend: &AgentsBackend, agent: &AgentId) {
    if usable_after_leave(backend, agent) {
        restart(backend, agent).await;
    } else {
        tracing::info!(%agent, "nothing stored after a leave: held for the next login");
    }
}

/// Whether the pump has shown Clauseo's own login for `agent`: it has handled its `SignedIn`.
pub(crate) fn own_login_shown(backend: &AgentsBackend, agent: &AgentId) -> bool {
    matches!(
        backend.compose.s18(agent),
        Some(SigninState::SignedIn {
            source: Source::Own,
            ..
        })
    )
}
