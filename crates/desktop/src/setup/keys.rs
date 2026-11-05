//! Each agent's API key, as the setup backend holds it (decision record 0048, "API keys"): read
//! from the OS keychain once at start, on desktop's own `clauseo-setup` thread, and kept in
//! memory as `Zeroizing` text, so no keychain call blocks the host's actor and a launch's hook
//! (R20 P5b piece B) reads the key from here.
//!
//! The brief says "on the agents thread": that thread is internal to the `agents` crate, which
//! runs no caller code on it, so the load runs on desktop's own setup thread instead.
//!
//! A write goes to the keychain first, on a thread of its own (a keychain call may block on a
//! prompt), and to memory only once it is stored, under one write order (see [`KeyCache`]). A
//! read that failed at start leaves the key out; a `main` key refused because one is stored
//! takes the stored one into memory then. `main`'s first key ([`KeyWrite::OnlyIfAbsent`])
//! goes through `Keychain::store_if_absent`, whose check and store are one step in this process,
//! and is refused with `settings.key_stored` when a key is there. No key reaches a log, an error
//! or a `Debug`: [`KeyCache`]'s `Debug` names the agents that have one.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use clauseo_agents::keys::{KeyError, Keychain};
use clauseo_protocol::{AgentId, ErrorCode, HostError, Secret};
use futures::channel::oneshot;
use zeroize::Zeroizing;

use super::KeyWrite;
use super::admit::key_stored;
use crate::bridge::lock;

/// Where the [`KeyCache`] lands once the setup thread has loaded it: the launch hook and the
/// limits readers are built before, on the host's start thread, and find no key until then.
pub(crate) type KeySlot = Arc<std::sync::OnceLock<Arc<KeyCache>>>;

/// The keys in memory, by agent.
type Keys = Arc<Mutex<BTreeMap<AgentId, Zeroizing<String>>>>;

/// The keys in memory, and the keychain they come from.
///
/// `order` is held from each write's keychain call through its memory update and its `then`
/// (the compose trigger), so memory, and what the windows are told, change in the order the
/// keychain did: a set and a clear at once never leave a cleared key in memory. It is never held
/// across anything that waits on a window, nor taken by a read ([`KeyCache::has`]).
pub(crate) struct KeyCache {
    keychain: Keychain,
    available: bool,
    keys: Keys,
    order: Arc<Mutex<()>>,
}

impl fmt::Debug for KeyCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyCache")
            .field("available", &self.available)
            .field("agents", &lock(&self.keys).keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl KeyCache {
    /// Reads each of `agents`' keys from `keychain`, blocking: once, at start. A key that does
    /// not load (no access, an item that is not a key) is left out and logged by its agent and
    /// the error's kind; the lawyer enters it again.
    pub(crate) fn load_all<'a>(
        keychain: Keychain,
        agents: impl IntoIterator<Item = &'a AgentId>,
    ) -> Self {
        let available = keychain.available();
        let mut keys = BTreeMap::new();
        if available {
            for agent in agents {
                match keychain.load(agent) {
                    Ok(Some(key)) => {
                        keys.insert(agent.clone(), key);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(%agent, kind = error_kind(&error), "an API key did not load");
                    }
                }
            }
        }
        Self {
            keychain,
            available,
            keys: Arc::new(Mutex::new(keys)),
            order: Arc::default(),
        }
    }

    /// Whether this computer's keychain can hold a key: `SetupInfo`'s `keys_available`.
    pub(crate) fn available(&self) -> bool {
        self.available
    }

    /// Whether `agent` has a key.
    pub(crate) fn has(&self, agent: &AgentId) -> bool {
        lock(&self.keys).contains_key(agent)
    }

    /// Whether the keychain would take `key` as `agent`'s: `command.invalid` (naming the agent
    /// and the reason, never the key) when it would refuse it. No keychain call.
    pub(crate) fn check(agent: &AgentId, key: &Secret) -> Result<(), HostError> {
        clauseo_agents::keys::check(agent, key.expose()).map_err(|error| key_error(&error))
    }

    /// Reads `agent`'s entry from the keychain, under the write order on a thread of its own:
    /// `true` when one is stored (memory takes it, then `then(true)`), `false` when none is,
    /// and the keychain's error (`internal`) when the read fails, memory left as it was.
    /// `main`'s first key asks it before it leaves a reused login.
    pub(crate) async fn probe(
        &self,
        agent: &AgentId,
        then: impl FnOnce(bool) + Send + 'static,
    ) -> Result<bool, HostError> {
        let (keychain, keys, order) = self.parts();
        let agent = agent.clone();
        off_thread(move || -> Result<bool, KeyError> {
            let _order = lock(&order);
            let Some(key) = keychain.load(&agent)? else {
                return Ok(false);
            };
            lock(&keys).insert(agent, key);
            then(true);
            Ok(true)
        })
        .await?
        .map_err(|error| key_error(&error))
    }

    /// A copy of `agent`'s key, zeroed when dropped: the launch hook's and the limits reads'.
    pub(crate) fn get(&self, agent: &AgentId) -> Option<Zeroizing<String>> {
        lock(&self.keys).get(agent).cloned()
    }

    /// Stores `key` as `agent`'s: in the keychain, then in memory, then `then(true)`. With
    /// [`KeyWrite::OnlyIfAbsent`] only when no key is stored, else `settings.key_stored` with
    /// nothing written; memory then takes the keychain's entry again (a read that failed at start
    /// left it out), and `then` gets whether it holds one. A key the keychain refuses is
    /// `command.invalid`; a keychain that fails is `internal`. Neither error holds the key.
    ///
    /// All of it runs on a thread of its own (a keychain call may block on a prompt) under the
    /// one write order ([`KeyCache`]), so memory and `then` follow the keychain's order, and a
    /// caller that stops waiting leaves them in step all the same.
    pub(crate) async fn set(
        &self,
        agent: &AgentId,
        key: Secret,
        write: KeyWrite,
        then: impl FnOnce(bool) + Send + 'static,
    ) -> Result<(), HostError> {
        let key = key.into_zeroizing();
        let (keychain, keys, order) = self.parts();
        let for_thread = agent.clone();
        let stored = off_thread(move || -> Result<bool, KeyError> {
            let agent = for_thread;
            let _order = lock(&order);
            let stored = match write {
                KeyWrite::Replace => keychain.store(&agent, &key).map(|()| true),
                KeyWrite::OnlyIfAbsent => keychain.store_if_absent(&agent, &key),
            }?;
            if stored {
                lock(&keys).insert(agent, key);
                then(true);
            } else {
                then(reload(&keychain, &keys, &agent));
            }
            Ok(stored)
        })
        .await?
        .map_err(|error| key_error(&error))?;
        if stored {
            Ok(())
        } else {
            Err(key_stored(agent))
        }
    }

    /// Removes `agent`'s key: from the keychain, then from memory, then `then(false)`; on a
    /// thread of its own under the write order, as [`KeyCache::set`] runs.
    pub(crate) async fn clear(
        &self,
        agent: &AgentId,
        then: impl FnOnce(bool) + Send + 'static,
    ) -> Result<(), HostError> {
        let (keychain, keys, order) = self.parts();
        let agent = agent.clone();
        off_thread(move || -> Result<(), KeyError> {
            let _order = lock(&order);
            keychain.clear(&agent)?;
            lock(&keys).remove(&agent);
            then(false);
            Ok(())
        })
        .await?
        .map_err(|error| key_error(&error))
    }

    /// What a write's thread needs: the keychain, memory and the write order.
    fn parts(&self) -> (Keychain, Keys, Arc<Mutex<()>>) {
        (
            self.keychain.clone(),
            Arc::clone(&self.keys),
            Arc::clone(&self.order),
        )
    }
}

/// Takes `agent`'s entry from `keychain` into `keys` again, under the write order, and answers
/// whether memory holds a key after: a read that fails leaves memory as it was, logged.
fn reload(keychain: &Keychain, keys: &Keys, agent: &AgentId) -> bool {
    match keychain.load(agent) {
        Ok(Some(key)) => {
            lock(keys).insert(agent.clone(), key);
            true
        }
        Ok(None) => {
            lock(keys).remove(agent);
            false
        }
        Err(error) => {
            tracing::warn!(%agent, kind = error_kind(&error), "an API key did not load");
            lock(keys).contains_key(agent)
        }
    }
}

/// Runs `work` on a thread of its own and waits for its answer: `internal` when no thread
/// starts or it ends without one.
async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, HostError> {
    let (tx, rx) = oneshot::channel();
    std::thread::Builder::new()
        .name("clauseo-keychain".to_owned())
        .spawn(move || {
            // The caller may have gone; then nobody reads the answer.
            let _gone = tx.send(work());
        })
        .map_err(|_no_thread| HostError::new(ErrorCode::Internal, "no thread for the keychain"))?;
    rx.await
        .map_err(|_ended| HostError::new(ErrorCode::Internal, "the keychain call ended early"))
}

/// The answer to a keychain error: its text names the agent and the kind, never the key.
fn key_error(error: &KeyError) -> HostError {
    let code = match error {
        KeyError::Invalid { .. } => ErrorCode::CommandInvalid,
        _ => ErrorCode::Internal,
    };
    HostError::new(code, error.to_string())
}

/// A fixed word for the kind of a keychain error, for the log.
fn error_kind(error: &KeyError) -> &'static str {
    match error {
        KeyError::NoKeychain { .. } => "no_keychain",
        KeyError::Invalid { .. } => "invalid",
        KeyError::StoredInvalid { .. } => "stored_invalid",
        KeyError::NoAccess { .. } => "no_access",
        KeyError::Keychain { kind, .. } => kind,
    }
}
