//! Each agent's launch refusal, in one place (decision record 0048, ruling 4 and R20 P5b A4).
//! The refusal in force for an agent ([`wanted`]):
//!
//! - `signin.policy_unreadable` while an unreadable policy blocks it;
//! - else `agent.key_unreadable` while its stored key is marked unreadable (the read failed,
//!   or did not answer within the first load's bound: `setup/keys/reads.rs`);
//! - else, while it launches on the shared login (its launch env carries no key and no provider
//!   switch: `identity::exempt`), its identity check's (R17, `setup/identity.rs`):
//!   `signin.account_changed`, `agent.identity_checking` or `agent.identity_unknown`;
//! - else none: no key is stored (it launches without one), or its key is in memory.
//!
//! `agent.keys_loading` is the host's from its start until the first load is over; [`sync`]
//! replaces it, once the keys have loaded or the bound has passed, with the refusal in force.
//! [`follow_keys`] keeps it in step later: each [`KeyNote::Read`] sets the agent's refusal
//! again (a slow read that answered, a read again that succeeded, a key set or cleared), and
//! each [`KeyNote::Retry`] dispatches by the code the launch met (the host's launch retry):
//! `agent.key_unreadable` reads the key again, `agent.identity_checking` asks the identity
//! follower for its check, and any other code does nothing. Retries already queued together
//! are taken as one: several launches waiting at once ask for one check.
//!
//! A refusal the host cannot be told (the call fails twice) fails closed: logged as an error,
//! the agent is kept to be told again at each look of the identity follower (every 2 s) until
//! the call succeeds, whatever it was told before.

use std::collections::BTreeSet;

use clauseo_protocol::{AgentId, ErrorCode};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;

use super::backend::AgentsBackend;
use super::host_calls::SetupHost;
use super::identity;
use super::keys::{KeyCache, KeyNote};
use crate::bridge::lock;

/// The refusal in force for `agent`, given whether a policy `blocked` it, its `keys`, and
/// its identity check's refusal, which holds only while no key is stored.
pub(crate) fn in_force(
    blocked: bool,
    keys: &KeyCache,
    identity: Option<ErrorCode>,
    agent: &AgentId,
) -> Option<ErrorCode> {
    if blocked {
        Some(ErrorCode::SigninPolicyUnreadable)
    } else if keys.unreadable(agent) {
        Some(ErrorCode::AgentKeyUnreadable)
    } else if keys.has(agent) {
        None
    } else {
        identity
    }
}

/// The refusal in force for `agent` as `backend` holds the settings, the keys and the
/// identity check; a launch env that replaces the shared login (`identity::exempt`: a key or
/// a provider, never an endpoint alone) takes the identity check's out of it.
pub(crate) fn wanted(backend: &AgentsBackend, agent: &AgentId) -> Option<ErrorCode> {
    let blocked = lock(&backend.wiring.applied).blocked.contains(agent);
    let identity = backend
        .wiring
        .identity
        .refusal(agent)
        .filter(|_| !identity::exempt(backend, agent));
    in_force(blocked, &backend.keys, identity, agent)
}

/// Tells the host `agent`'s refusal in force when it is not what the host was last told, or
/// the last telling failed. A call that fails is tried once more (review L4); failing again,
/// it fails closed: an error log with the agent and the error's code, and the agent kept in
/// `unsynced`, told again at the identity follower's next look.
pub(crate) async fn sync(backend: &AgentsBackend, agent: &AgentId) {
    let _gate = backend.wiring.refusal_gate.lock().await;
    let code = wanted(backend, agent);
    let unsynced = lock(&backend.wiring.unsynced).contains(agent);
    if !unsynced && lock(&backend.wiring.refused).get(agent).copied() == code {
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
                lock(&backend.wiring.unsynced).remove(agent);
                return;
            }
            Err(error) if !again => {
                let code = error.code().as_str();
                tracing::info!(%agent, code, "launch refusal not set; again");
            }
            Err(error) => {
                let code = error.code().as_str();
                tracing::error!(%agent, code, "launch refusal not set; told again at the next look");
                lock(&backend.wiring.unsynced).insert(agent.clone());
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
        let code = in_force(false, keys, None, agent);
        if let Err(error) = host.set_launch_refused(agent.clone(), code).await {
            let code = error.code().as_str();
            tracing::warn!(%agent, code, "the keys' launch refusal was not set");
        }
    }
}

/// Each note, until the channel closes: a [`KeyNote::Read`] sets that agent's refusal again
/// (and what the windows show of its key); a [`KeyNote::Retry`] goes by the code the launch
/// met, as the module docs say.
pub(crate) async fn follow_keys(backend: &AgentsBackend, notes: &mut UnboundedReceiver<KeyNote>) {
    while let Some(first) = notes.next().await {
        for note in coalesce(first, notes) {
            follow_note(backend, note).await;
        }
    }
}

/// `first` and the notes queued behind it, each identity retry once per agent: several
/// launches meeting `agent.identity_checking` together ask for one check.
fn coalesce(first: KeyNote, notes: &mut UnboundedReceiver<KeyNote>) -> Vec<KeyNote> {
    let mut batch = vec![first];
    while let Ok(note) = notes.try_recv() {
        let identity = matches!(note, KeyNote::Retry(_, ErrorCode::AgentIdentityChecking));
        if !(identity && batch.contains(&note)) {
            batch.push(note);
        }
    }
    batch
}

/// One note, as [`follow_keys`] says.
async fn follow_note(backend: &AgentsBackend, note: KeyNote) {
    match note {
        KeyNote::Retry(agent, ErrorCode::AgentKeyUnreadable) => {
            // Its answer comes back as a `KeyNote::Read` when the mark changes.
            drop(backend.keys.read_again(&agent));
        }
        // The follower runs it when it is due, and the refusal follows its verdict.
        KeyNote::Retry(agent, ErrorCode::AgentIdentityChecking) => {
            backend.wiring.identity.ask(&agent);
        }
        KeyNote::Retry(..) => {}
        KeyNote::Read(agent) => {
            if !backend.known.contains_key(&agent) {
                return;
            }
            backend.compose.show_key(&agent, &backend.keys);
            // A key set, cleared or read changes the launch env's exemption (R17); the
            // refusal follows.
            identity::exemption_changed(backend, &agent).await;
        }
    }
}
