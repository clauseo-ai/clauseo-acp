//! Each agent's launch refusal, in one place (decision record 0048, ruling 4 and R20 P5b A4).
//! The refusal in force for an agent ([`wanted`]):
//!
//! - `signin.policy_unreadable` while an unreadable policy blocks it;
//! - else `agent.key_unreadable` while its stored key is marked unreadable (the read failed,
//!   or did not answer within the first load's bound: `setup/keys/reads.rs`);
//! - else none: no key is stored (it launches without one), or its key is in memory.
//!
//! `agent.keys_loading` is the host's from its start until the first load is over; [`sync`]
//! replaces it, once the keys have loaded or the bound has passed, with the refusal in force.
//! [`follow_keys`] keeps it in step later: each [`KeyNote::Read`] sets the agent's refusal
//! again (a slow read that answered, a read again that succeeded, a key set or cleared), and
//! each [`KeyNote::Retry`] (a launch met `agent.key_unreadable`) reads the key again.

use std::collections::BTreeSet;

use clauseo_protocol::{AgentId, ErrorCode};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;

use super::backend::AgentsBackend;
use super::host_calls::SetupHost;
use super::keys::{KeyCache, KeyNote};
use crate::bridge::lock;

/// The refusal in force for `agent`, given whether a policy `blocked` it and its `keys`.
pub(crate) fn in_force(blocked: bool, keys: &KeyCache, agent: &AgentId) -> Option<ErrorCode> {
    if blocked {
        Some(ErrorCode::SigninPolicyUnreadable)
    } else if keys.unreadable(agent) {
        Some(ErrorCode::AgentKeyUnreadable)
    } else {
        None
    }
}

/// The refusal in force for `agent` as `backend` holds the settings and keys.
pub(crate) fn wanted(backend: &AgentsBackend, agent: &AgentId) -> Option<ErrorCode> {
    let blocked = lock(&backend.wiring.applied).blocked.contains(agent);
    in_force(blocked, &backend.keys, agent)
}

/// Tells the host `agent`'s refusal in force when it is not what the host was last told. A
/// call that fails is logged by the agent and its code; the host keeps what it had.
pub(crate) async fn sync(backend: &AgentsBackend, agent: &AgentId) {
    let _gate = backend.wiring.refusal_gate.lock().await;
    let code = wanted(backend, agent);
    if lock(&backend.wiring.refused).get(agent).copied() == code {
        return;
    }
    match backend.host.set_launch_refused(agent.clone(), code).await {
        Ok(()) => {
            let mut refused = lock(&backend.wiring.refused);
            match code {
                Some(code) => refused.insert(agent.clone(), code),
                None => refused.remove(agent),
            };
        }
        Err(error) => {
            tracing::warn!(%agent, code = error.code().as_str(), "launch refusal not set");
        }
    }
}

/// [`sync`] for each of `backend`'s agents.
pub(crate) async fn sync_all(backend: &AgentsBackend) {
    for agent in backend.known.keys() {
        sync(backend, agent).await;
    }
}

/// The first load is over and no backend follows (no platform): each of `known` but those in
/// `blocked` (whose refusal is the policy's) gets its refusal in force from `keys`. A call
/// that fails is logged by the agent and its code.
pub(crate) async fn settle_after_load<'a>(
    host: &dyn SetupHost,
    keys: &KeyCache,
    known: impl IntoIterator<Item = &'a AgentId>,
    blocked: &BTreeSet<AgentId>,
) {
    for agent in known.into_iter().filter(|agent| !blocked.contains(*agent)) {
        let code = in_force(false, keys, agent);
        if let Err(error) = host.set_launch_refused(agent.clone(), code).await {
            let code = error.code().as_str();
            tracing::warn!(%agent, code, "the keys' launch refusal was not set");
        }
    }
}

/// Each note about a stored key, until the channel closes: a [`KeyNote::Read`] sets that
/// agent's refusal again (and what the windows show of its key); a [`KeyNote::Retry`] reads
/// its key again when it is marked unreadable.
pub(crate) async fn follow_keys(backend: &AgentsBackend, mut notes: UnboundedReceiver<KeyNote>) {
    while let Some(note) = notes.next().await {
        match note {
            KeyNote::Retry(agent) => {
                // Its answer comes back as a `KeyNote::Read` when the mark changes.
                drop(backend.keys.read_again(&agent));
            }
            KeyNote::Read(agent) => {
                if !backend.known.contains_key(&agent) {
                    continue;
                }
                backend.compose.set_key(&agent, backend.keys.has(&agent));
                sync(backend, &agent).await;
            }
        }
    }
}
