//! Each agent's API key, as the setup backend holds it (decision record 0048, "API keys"): read
//! from the OS keychain once at start, on desktop's own `clauseo-setup` thread, and kept in
//! memory as `Zeroizing` text, so no keychain call blocks the host's actor and a launch's hook
//! (R20 P5b piece B) reads the key from here.
//!
//! The brief says "on the agents thread": that thread is internal to the `agents` crate, which
//! runs no caller code on it, so the load runs on desktop's own setup thread instead.
//!
//! The first load is bounded (decision record 0048, R20 P5b A4; `keys/reads.rs`): each agent's
//! key is read on a thread of its own, and the setup thread waits for them at most
//! [`KEYS_LOAD_BOUND`]. A read that fails (the keychain's error), or has not answered by then
//! for an agent the "key stored" marker marks (`keys/marker.rs`), marks the agent's key
//! **unreadable**: a key is stored, or may be, and it never reads as "no
//! key" ([`KeyCache::unreadable`]; [`KeyCache::has`] counts it). Desktop then refuses that
//! agent's launches with `agent.key_unreadable` (`setup/refusal.rs`) until a later read
//! succeeds: the slow read answering, or a read again ([`KeyCache::read_again`]) when a
//! launch meets the refusal. Each change of the mark goes out as a [`KeyNote::Read`].
//!
//! An agent the marker does not mark has no key Clauseo stored: one whose read has not
//! answered by the bound launches without a key. A save sets the marker before the keychain
//! write, a clear unsets it after, and each read that answers brings it in step.
//!
//! A write goes to the keychain first, on a thread of its own (a keychain call may block on a
//! prompt), and to memory only once it is stored, under one write order (see [`KeyCache`]). A
//! write that lands ends the unreadable mark; a `main` key refused because one is stored
//! takes the stored one into memory then, or marks it unreadable when that read fails.
//! `main`'s first key ([`KeyWrite::OnlyIfAbsent`]) goes through `Keychain::store_if_absent`,
//! whose check and store are one step in this process, and is refused with
//! `settings.key_stored` when a key is there. No key reaches a log, an error
//! or a `Debug`: [`KeyCache`]'s `Debug` names the agents that have one.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::keys::{KeyError, Keychain};
use clauseo_protocol::{AgentId, ErrorCode, HostError, Secret};
use futures::channel::oneshot;
use zeroize::Zeroizing;

use super::KeyWrite;
use super::admit::key_stored;
use crate::bridge::lock;

pub(super) mod marker;
mod notes;
mod reads;

pub(crate) use marker::KeyMarker;
pub(crate) use notes::{KeyNote, KeyNotes, NoteSender};
use reads::Marks;

/// How long the setup thread waits for the first load before an agent whose read has not
/// answered counts as unreadable (an unanswered keychain prompt, say).
pub(crate) const KEYS_LOAD_BOUND: Duration = Duration::from_secs(10);

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
    /// The agents whose stored key is unreadable or being read, and the notes of each change.
    marks: Marks,
    /// Which agents Clauseo stored a key for, outside the keychain (`keys/marker.rs`).
    marker: Arc<KeyMarker>,
}

impl fmt::Debug for KeyCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // One lock at a time, never the keys' while the marks' is taken: `Marks::finish` takes
        // them the other way round (review L5).
        let agents: Vec<AgentId> = lock(&self.keys).keys().cloned().collect();
        let unreadable = self.marks.unreadable_agents();
        f.debug_struct("KeyCache")
            .field("available", &self.available)
            .field("agents", &agents)
            .field("unreadable", &unreadable)
            .finish_non_exhaustive()
    }
}

impl KeyCache {
    /// No key in memory and none marked yet, over `keychain` and `marker`; each change of a
    /// mark goes to `notes`.
    fn empty(keychain: Keychain, notes: Option<NoteSender>, marker: KeyMarker) -> Self {
        Self {
            available: keychain.available(),
            keychain,
            keys: Arc::default(),
            order: Arc::default(),
            marks: Marks::new(notes),
            marker: Arc::new(marker),
        }
    }

    /// Reads each of `agents`' keys from `keychain` and waits for all: the tests' load.
    #[cfg(test)]
    pub(crate) fn load_all<'a>(
        keychain: Keychain,
        agents: impl IntoIterator<Item = &'a AgentId>,
    ) -> Self {
        reads::load_all(keychain, agents)
    }

    /// The first load, on the setup thread, waited for at most `bound` on `timer`
    /// (`keys/reads.rs`); a read that has not answered by then marks the key unreadable only
    /// when `marker` marks the agent. Each change of a mark, then and later, goes to `notes`.
    pub(crate) fn load_bounded<'a>(
        keychain: Keychain,
        agents: impl IntoIterator<Item = &'a AgentId>,
        marker: KeyMarker,
        notes: NoteSender,
        timer: &impl clauseo_process::Timer,
        bound: Duration,
    ) -> Self {
        reads::load_bounded(keychain, agents, marker, notes, timer, bound)
    }

    /// Reads `agent`'s key again when it is marked unreadable and no read of it runs
    /// (`keys/reads.rs`); the receiver resolves once the answer is in.
    pub(crate) fn read_again(&self, agent: &AgentId) -> oneshot::Receiver<()> {
        reads::read_again(self, agent)
    }

    /// Whether this computer's keychain can hold a key: `SetupInfo`'s `keys_available`.
    pub(crate) fn available(&self) -> bool {
        self.available
    }

    /// `internal`, as a store would answer it, when this computer's keychain can hold no key:
    /// asked before a key leaves a reused login, so no refusal follows a leave (review N1).
    pub(crate) fn refuse_unavailable(&self, agent: &AgentId) -> Result<(), HostError> {
        if self.available {
            return Ok(());
        }
        Err(key_error(&KeyError::NoKeychain {
            agent: agent.clone(),
        }))
    }

    /// Whether `agent` has a key: in memory, or stored and unreadable (never "no key").
    pub(crate) fn has(&self, agent: &AgentId) -> bool {
        let in_memory = lock(&self.keys).contains_key(agent);
        in_memory || self.marks.is_unreadable(agent)
    }

    /// Whether memory holds `agent`'s key.
    pub(crate) fn in_memory(&self, agent: &AgentId) -> bool {
        lock(&self.keys).contains_key(agent)
    }

    /// Whether `agent`'s stored key could not be read: the read failed, or has not answered.
    pub(crate) fn unreadable(&self, agent: &AgentId) -> bool {
        self.marks.is_unreadable(agent)
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
        let marks = self.marks.clone();
        let marker = Arc::clone(&self.marker);
        let agent = agent.clone();
        off_thread(move || -> Result<bool, KeyError> {
            let _order = lock(&order);
            let Some(key) = keychain.load(&agent)? else {
                marks.known(&agent);
                marker.reconcile(&agent, false);
                return Ok(false);
            };
            lock(&keys).insert(agent.clone(), key);
            marks.known(&agent);
            marker.reconcile(&agent, true);
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

    /// Stores `key` as `agent`'s: the marker first (`keys/marker.rs`; one that cannot be
    /// written refuses the save with `internal`, nothing stored), then the keychain (a write
    /// that fails leaves the marker on, which fails closed), then memory, then `then(true)`. With
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
        let marks = self.marks.clone();
        let marker = Arc::clone(&self.marker);
        let for_thread = agent.clone();
        let stored = off_thread(move || -> Result<bool, HostError> {
            let agent = for_thread;
            let _order = lock(&order);
            marker
                .set(&agent)
                .map_err(|error| marker_error(&agent, &error))?;
            let stored = match write {
                KeyWrite::Replace => keychain.store(&agent, &key).map(|()| true),
                KeyWrite::OnlyIfAbsent => keychain.store_if_absent(&agent, &key),
            }
            .map_err(|error| key_error(&error))?;
            if stored {
                lock(&keys).insert(agent.clone(), key);
                marks.known(&agent);
                then(true);
            } else {
                then(reload(&keychain, &keys, &marks, &marker, &agent));
            }
            Ok(stored)
        })
        .await??;
        if stored {
            Ok(())
        } else {
            Err(key_stored(agent))
        }
    }

    /// Removes `agent`'s key: from the keychain, then from memory, then the marker (best effort:
    /// one that cannot be cleared stays on, which fails closed), then `then(false)`; on a
    /// thread of its own under the write order, as [`KeyCache::set`] runs.
    pub(crate) async fn clear(
        &self,
        agent: &AgentId,
        then: impl FnOnce(bool) + Send + 'static,
    ) -> Result<(), HostError> {
        let (keychain, keys, order) = self.parts();
        let marks = self.marks.clone();
        let marker = Arc::clone(&self.marker);
        let agent = agent.clone();
        off_thread(move || -> Result<(), KeyError> {
            let _order = lock(&order);
            keychain.clear(&agent)?;
            lock(&keys).remove(&agent);
            marks.known(&agent);
            marker.clear(&agent);
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
/// whether a key is stored: a read that fails marks it unreadable (a key is there, the store
/// said so) and answers `true`, never "no key". A read that answers brings the marker in step.
fn reload(
    keychain: &Keychain,
    keys: &Keys,
    marks: &Marks,
    marker: &KeyMarker,
    agent: &AgentId,
) -> bool {
    match keychain.load(agent) {
        Ok(Some(key)) => {
            lock(keys).insert(agent.clone(), key);
            marks.known(agent);
            marker.reconcile(agent, true);
            true
        }
        Ok(None) => {
            lock(keys).remove(agent);
            marks.known(agent);
            marker.reconcile(agent, false);
            false
        }
        Err(error) => {
            marks.unreadable(agent, error_kind(&error));
            true
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

/// The answer to a save whose marker could not be written: `internal`, naming the agent and
/// the error's kind, never a path.
fn marker_error(agent: &AgentId, error: &std::io::Error) -> HostError {
    let kind = error.kind();
    tracing::warn!(%agent, ?kind, "the key-stored marker was not written: the key is not stored");
    HostError::new(
        ErrorCode::Internal,
        format!("{agent}'s key was not stored: its marker could not be written ({kind})"),
    )
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
