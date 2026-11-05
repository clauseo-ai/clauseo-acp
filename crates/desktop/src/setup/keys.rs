//! Each agent's API key, as the setup backend holds it (decision record 0048, "API keys"): read
//! from the OS keychain once at start, on desktop's own `clauseo-setup` thread, and kept in
//! memory as `Zeroizing` text, so no keychain call blocks the host's actor and a launch's hook
//! (R20 P5b piece B) reads the key from here.
//!
//! The brief says "on the agents thread": that thread is internal to the `agents` crate, which
//! runs no caller code on it, so the load runs on desktop's own setup thread instead.
//!
//! A write goes to the keychain first, on a thread of its own (a keychain call may block on a
//! prompt), and to memory only once it is stored. `main`'s first key ([`KeyWrite::OnlyIfAbsent`])
//! goes through `Keychain::store_if_absent`, whose check and store are one step in this process,
//! and is refused with `settings.key_stored` when a key is there. No key reaches a log, an error
//! or a `Debug`: [`KeyCache`]'s `Debug` names the agents that have one.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

use clauseo_agents::keys::{KeyError, Keychain};
use clauseo_protocol::{AgentId, ErrorCode, HostError, Secret};
use futures::channel::oneshot;
use zeroize::Zeroizing;

use super::KeyWrite;
use super::admit::key_stored;
use crate::bridge::lock;

/// The keys in memory, and the keychain they come from.
pub(crate) struct KeyCache {
    keychain: Keychain,
    available: bool,
    keys: Mutex<BTreeMap<AgentId, Zeroizing<String>>>,
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
            keys: Mutex::new(keys),
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

    /// A copy of `agent`'s key, zeroed when dropped: the launch hook's (piece B).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "R20 P5b piece B's launch hook reads it")
    )]
    pub(crate) fn get(&self, agent: &AgentId) -> Option<Zeroizing<String>> {
        lock(&self.keys).get(agent).cloned()
    }

    /// Stores `key` as `agent`'s: in the keychain (off this thread), then in memory. With
    /// [`KeyWrite::OnlyIfAbsent`] only when no key is stored, else `settings.key_stored` with
    /// nothing written. A key the keychain refuses is `command.invalid`; a keychain that fails
    /// is `internal`. Neither error holds the key.
    pub(crate) async fn set(
        &self,
        agent: &AgentId,
        key: Secret,
        write: KeyWrite,
    ) -> Result<(), HostError> {
        let key = key.into_zeroizing();
        let keychain = self.keychain.clone();
        let for_thread = agent.clone();
        let stored = off_thread(move || {
            let key = key;
            match write {
                KeyWrite::Replace => keychain.store(&for_thread, &key).map(|()| Some(key)),
                KeyWrite::OnlyIfAbsent => keychain
                    .store_if_absent(&for_thread, &key)
                    .map(|stored| stored.then_some(key)),
            }
        })
        .await?
        .map_err(|error| key_error(&error))?;
        let Some(key) = stored else {
            return Err(key_stored(agent));
        };
        lock(&self.keys).insert(agent.clone(), key);
        Ok(())
    }

    /// Removes `agent`'s key: from the keychain (off this thread), then from memory.
    pub(crate) async fn clear(&self, agent: &AgentId) -> Result<(), HostError> {
        let keychain = self.keychain.clone();
        let for_thread = agent.clone();
        off_thread(move || keychain.clear(&for_thread))
            .await?
            .map_err(|error| key_error(&error))?;
        lock(&self.keys).remove(agent);
        Ok(())
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
