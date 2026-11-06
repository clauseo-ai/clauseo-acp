//! The backend's `agents.<id>` entry writes: from an install, for the login's source.

use clauseo_agents::{SigninState, Source};
use clauseo_protocol::{AgentId, AgentKind, SignInSource};
use clauseo_settings::EntryWrite;

use super::AgentsBackend;
use super::launch;

/// Writes `agent`'s `agents.<id>` entry from its install, for the login's source as S18
/// last said ([`AgentsBackend`]'s).
pub(crate) async fn write_entry(backend: &AgentsBackend, agent: &AgentId) {
    let source = match backend.compose.s18(agent) {
        Some(SigninState::SignedIn {
            source: Source::Reused,
            ..
        }) => SignInSource::Reused,
        _ => SignInSource::Own,
    };
    write_entry_as(backend, agent, source).await;
}

/// Writes `agent`'s `agents.<id>` entry from its install for a login of `source`, logging
/// the outcome by code; a written entry is what the limits readers read from then on.
pub(crate) async fn write_entry_as(backend: &AgentsBackend, agent: &AgentId, source: SignInSource) {
    let (Some(info), Some(installed)) = (backend.known.get(agent), backend.agents.installed(agent))
    else {
        return;
    };
    let Some(launch) = launch(agent, &installed, &info.home, source) else {
        tracing::warn!(%agent, "the install's paths are not UTF-8: no entry written");
        return;
    };
    match backend
        .host
        .set_agent_launch(agent.clone(), launch.clone())
        .await
    {
        Ok(EntryWrite::Written) => {
            tracing::info!(%agent, "agent entry written");
            if let Some(kind) = AgentKind::of(agent) {
                backend.wiring.readers.set_entry(kind, Some(launch));
            }
        }
        Ok(EntryWrite::LeftAlone) => {
            tracing::info!(%agent, "agent entry left alone: it is the lawyer's or the firm's");
        }
        Err(error) => {
            tracing::warn!(%agent, code = error.code().as_str(), "agent entry not written");
        }
    }
}
