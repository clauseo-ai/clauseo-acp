//! `SetAgentKey` and `SetFirmEndpoint`: the checks, the confirm, the leave order and the write.

use clauseo_protocol::{AgentId, ErrorCode, HostError};

use super::super::KeyWrite;
use super::super::admit::key_stored;
use super::super::confirm::Ask;
use super::super::keys::KeyCache;
use super::super::leave;
use super::parts::checked_endpoint;
use super::{AgentInfo, AgentsBackend};

/// `SetAgentKey`: refused while policy blocks the agent, for a key the keychain would
/// refuse (`command.invalid`), and from `main` while a key is there; a reused login is left
/// first (the leave order), but only once the key is known to be taken: `main`'s first key
/// on a reused login reads the keychain first, and a read that fails refuses it, so no
/// refusal ever follows a leave (the security review's M5), and a computer with no keychain
/// refuses any key before it (review N1). The reads are held while the
/// key is written, then the agents restart with it and the limits read it.
pub(super) async fn set_key(
    backend: &AgentsBackend,
    agent: &AgentId,
    key: clauseo_protocol::Secret,
    key_write: KeyWrite,
) -> Result<(), HostError> {
    backend.refuse_blocked(agent)?;
    KeyCache::check(agent, &key)?;
    backend.keys.refuse_unavailable(agent)?;
    // `main`'s first key leaves no login for a key that is there already. One marked
    // unreadable goes on to the keychain's check, which reads it again.
    if key_write == KeyWrite::OnlyIfAbsent && backend.keys.in_memory(agent) {
        return Err(key_stored(agent));
    }
    let left = backend.reused(agent);
    if left
        && key_write == KeyWrite::OnlyIfAbsent
        && backend
            .keys
            .probe(agent, backend.key_trigger(agent))
            .await?
    {
        return Err(key_stored(agent));
    }
    if left {
        leave::leave_first(backend, agent, leave::After::Command).await?;
    }
    let hold = leave::hold_reads(backend, agent).await;
    let then = backend.key_trigger(agent);
    let stored = backend.keys.set(agent, key, key_write, then).await;
    drop(hold);
    if let Err(error) = stored {
        if left {
            leave::restart_if_usable(backend, agent).await;
        }
        return Err(error);
    }
    backend.compose.key_saved(agent);
    backend.end_sign_in(agent);
    leave::restart_for_login(backend, agent).await;
    Ok(())
}

/// `SetFirmEndpoint`: every check before the confirm, so a refused endpoint asks nothing and
/// a page cannot write its own words into the dialog: `signin.policy_unreadable` while
/// policy is unreadable for a firm plan, `settings.locked` for a plan policy sets, and
/// `command.invalid` for an address the settings crate would refuse. Then the confirm, then
/// the endpoint alone written, the plan's provider and variables kept.
pub(super) async fn set_endpoint(
    backend: &AgentsBackend,
    agent: &AgentId,
    info: &AgentInfo,
    url: Option<String>,
) -> Result<(), HostError> {
    backend.refuse_blocked(agent)?;
    if info.plan_locked {
        return Err(HostError::new(
            ErrorCode::SettingsLocked,
            format!("the firm's policy sets plans.{agent}"),
        ));
    }
    let url = url.as_deref().map(checked_endpoint).transpose()?;
    let name = info.name.clone();
    backend
        .ask(Ask::Endpoint {
            name,
            url: url.clone(),
        })
        .await?;
    let left = url.is_some() && backend.reused(agent);
    if left {
        leave::leave_first(backend, agent, leave::After::Command).await?;
    }
    let firm = url.is_some() || info.provider;
    let hold = leave::hold_reads(backend, agent).await;
    let set = backend.host.set_endpoint(agent.clone(), url).await;
    drop(hold);
    if let Err(error) = set {
        if left {
            leave::restart_if_usable(backend, agent).await;
        }
        return Err(error);
    }
    backend.compose.set_firm(agent, firm);
    if firm {
        backend.end_sign_in(agent);
    }
    leave::restart_for_login(backend, agent).await;
    Ok(())
}
