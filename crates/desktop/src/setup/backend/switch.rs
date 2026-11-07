//! The confirm before a Codex sign-in that would replace Clauseo's own login: `codex login`
//! revokes and deletes the login it finds before it starts, so Clauseo never starts one over a
//! signed-in account without the lawyer's say.

use clauseo_agents::{SigninState, Source};
use clauseo_protocol::{AgentId, AgentKind};

use super::AgentsBackend;
use crate::setup::confirm::Ask;

/// Whether a sign-in may start for `agent` (named `name`): always, unless it is Codex and
/// signed in with Clauseo's own login. Then the lawyer is asked first ([`Ask::SwitchAccount`]);
/// a decline starts nothing and removes the starting mark (`SetupEvents::end_start`), which no
/// sign-in state would otherwise end. The caller answers `Done` then, as a Cancel should: there
/// is no refusal to show.
pub(super) async fn switch_first(backend: &AgentsBackend, agent: &AgentId, name: &str) -> bool {
    let signed_in = matches!(
        backend.agents.signin_state(agent),
        Some(SigninState::SignedIn {
            source: Source::Own,
            ..
        })
    );
    if AgentKind::of(agent) != Some(AgentKind::Codex) || !signed_in {
        return true;
    }
    let name = name.to_owned();
    if backend.confirm.confirm(Ask::SwitchAccount { name }).await {
        return true;
    }
    tracing::info!(%agent, "sign-in not started: the lawyer kept the account");
    backend.events.end_start(agent);
    false
}
