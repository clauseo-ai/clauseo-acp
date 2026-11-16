//! Each agent's launch refusal, in one place (decision record 0048, ruling 4 and R20 P5b A4).
//! The refusal in force for an agent ([`wanted`]):
//!
//! - `signin.policy_unreadable` while an unreadable policy blocks it;
//! - else `agent.key_unreadable` while its stored key is marked unreadable (the read failed,
//!   or did not answer within the first load's bound: `setup/keys/reads.rs`);
//! - else `agent.firm_key_needed` while its plan sends it to an endpoint with Claude Code's own
//!   sign-in still on (`hook::sends_own_signin`) and no key is in memory: Claude Code 2.1.293
//!   would send the lawyer's claude.ai token to that endpoint as a Bearer;
//! - else `agent.endpoint_key_needed` while its launches get such an endpoint from the app's own
//!   environment or its entry, with no plan setting one (`inherited.rs`), and no key is in memory;
//! - else none: no key is stored (it launches without one), or its key is in memory.
//!
//! `agent.keys_loading` is the host's from its start until the first load is over and the
//! first network route is known (S17: a launch at start must not go direct behind a proxy set
//! only in the OS); [`lift_at_start`] replaces it, once the keys have loaded or the bound has
//! passed and the route has landed or `FIRST_ROUTE_WAIT` has passed since the setup's start (the
//! keys' load included), with the refusal in force.
//! [`follow_keys`] keeps it in step later: each [`KeyNote::Read`] sets the agent's refusal
//! again (a slow read that answered, a read again that succeeded, a key set or cleared), and
//! each [`KeyNote::Retry`] (a launch met `agent.key_unreadable`) reads the key again.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use clauseo_protocol::{AgentId, ErrorCode};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;

use super::backend::AgentsBackend;
use super::host_calls::SetupHost;
use super::inherited::Inherited;
use super::keys::{KeyCache, KeyNote};
use crate::bridge::lock;

/// The refusal in force for `agent`, given whether a policy `blocked` it, whether a firm plan
/// (an endpoint or a provider) is set for it, whether that plan keeps Claude Code's own sign-in
/// on toward its endpoint (`own_signin`), whether its launches get such an endpoint that no
/// plan set (`inherited`), and its `keys`.
pub(crate) fn in_force(
    blocked: bool,
    firm: bool,
    (own_signin, inherited): (bool, bool),
    keys: &KeyCache,
    agent: &AgentId,
) -> Option<ErrorCode> {
    if blocked {
        Some(ErrorCode::SigninPolicyUnreadable)
    } else if keys.unreadable_for(agent, firm) {
        Some(ErrorCode::AgentKeyUnreadable)
    } else if own_signin && !keys.in_memory(agent) {
        Some(ErrorCode::AgentFirmKeyNeeded)
    } else if inherited && !keys.in_memory(agent) {
        Some(ErrorCode::AgentEndpointKeyNeeded)
    } else {
        None
    }
}

/// The refusal in force for `agent` as `backend` holds the settings and keys.
pub(crate) fn wanted(backend: &AgentsBackend, agent: &AgentId) -> Option<ErrorCode> {
    let (blocked, firm, endpoint) = {
        let applied = lock(&backend.wiring.applied);
        (
            applied.blocked.contains(agent),
            applied.firm.contains(agent),
            (
                applied.own_signin.contains(agent),
                applied.inherited.contains_key(agent),
            ),
        )
    };
    in_force(blocked, firm, endpoint, &backend.keys, agent)
}

/// Tells the host `agent`'s refusal in force when it is not what the host was last told. A
/// call that fails is tried once more (review L4), then logged by the agent and the error's
/// code; the host keeps what it had.
pub(crate) async fn sync(backend: &AgentsBackend, agent: &AgentId) {
    let _gate = backend.wiring.refusal_gate.lock().await;
    let code = wanted(backend, agent);
    if lock(&backend.wiring.refused).get(agent).copied() == code {
        return;
    }
    for again in [false, true] {
        match backend.host.set_launch_refused(agent.clone(), code).await {
            Ok(()) => {
                let mut refused = lock(&backend.wiring.refused);
                match code {
                    Some(code) => refused.insert(agent.clone(), code),
                    None => refused.remove(agent),
                };
                return;
            }
            Err(error) if !again => {
                let code = error.code().as_str();
                tracing::info!(%agent, code, "launch refusal not set; again");
            }
            Err(error) => {
                tracing::warn!(%agent, code = error.code().as_str(), "launch refusal not set");
            }
        }
    }
}

/// [`sync`] for each of `backend`'s agents.
pub(crate) async fn sync_all(backend: &AgentsBackend) {
    for agent in backend.known.keys() {
        sync(backend, agent).await;
    }
}

/// The start's first [`sync_all`], once the keys' first load is over (the caller's): it waits,
/// at most until `wait` after the setup's start (the keys' load counts), for the first network
/// route to reach `Agents` and the launch hook, so the `agent.keys_loading` the host started
/// with holds every launch until its proxy is known.
pub(crate) async fn lift_at_start(backend: &AgentsBackend, wait: Duration) {
    backend.wiring.network.ready_within(wait).await;
    sync_all(backend).await;
}

/// The first load is over and no backend follows (no platform): each of `known` but those in
/// `blocked` (whose refusal is the policy's) gets its refusal in force from `keys`, those in
/// `firm` under their firm plan, those in `own_signin` under its endpoint
/// (`SettingsRead::own_signin`) and those in `inherited` under one no plan set
/// (`SettingsRead::inherited`). A call that fails is logged by the agent and its code.
pub(crate) async fn settle_after_load<'a>(
    host: &dyn SetupHost,
    keys: &KeyCache,
    known: impl IntoIterator<Item = &'a AgentId>,
    blocked: &BTreeSet<AgentId>,
    firm: &BTreeSet<AgentId>,
    (own_signin, inherited): (&BTreeSet<AgentId>, &BTreeMap<AgentId, Inherited>),
) {
    for agent in known.into_iter().filter(|agent| !blocked.contains(*agent)) {
        let code = in_force(
            false,
            firm.contains(agent),
            (own_signin.contains(agent), inherited.contains_key(agent)),
            keys,
            agent,
        );
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
                backend.compose.show_key(&agent, &backend.keys);
                sync(backend, &agent).await;
            }
        }
    }
}
