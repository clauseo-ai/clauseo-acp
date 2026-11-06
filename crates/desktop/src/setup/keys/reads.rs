//! The key cache's reads and its unreadable marks (decision record 0048, R20 P5b A4).
//!
//! - The first load ([`KeyCache::load_bounded`]) reads each agent's key on a
//!   `clauseo-keychain` thread of its own, so one prompt nobody answers holds up no other
//!   agent, and waits for them at most the bound on the setup thread's timer. A read that has
//!   not answered by then is marked unreadable and goes on; when it answers it is taken in as
//!   any read is.
//! - A read's answer: a key goes into memory and ends the mark; no item ends the mark (no key
//!   is stored: the agent launches without one); the keychain's error marks the key
//!   unreadable, logged by the agent, "key unreadable" and the error's kind, never "no key".
//!   A write since the read began (a key set or cleared) wins: the read's answer is dropped.
//! - [`KeyCache::read_again`]: a key marked unreadable, with no read running, is read again.
//! - Each change of a mark sends a [`KeyNote::Read`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::keys::{KeyError, Keychain};
use clauseo_process::Timer;
use clauseo_protocol::AgentId;
use futures::channel::oneshot;
use futures::future::{join_all, select};
use zeroize::Zeroizing;

use super::marker::{KeyMarker, Marked};
use super::notes::{KeyNote, NoteSender, send};
use super::{KeyCache, Keys, error_kind};
use crate::bridge::lock;

/// Which agents' keys are unreadable and which are being read, and each agent's writes.
#[derive(Debug, Default)]
struct ReadState {
    unreadable: BTreeSet<AgentId>,
    reading: BTreeSet<AgentId>,
    /// A count of each agent's writes and marks: a read that began before one is dropped.
    writes: BTreeMap<AgentId, u64>,
}

/// The marks, shared by the cache and its threads, and where their changes go.
#[derive(Clone)]
pub(super) struct Marks {
    state: Arc<Mutex<ReadState>>,
    notes: Option<NoteSender>,
}

impl Marks {
    /// No mark yet; changes go to `notes`.
    pub(super) fn new(notes: Option<NoteSender>) -> Self {
        Self {
            state: Arc::default(),
            notes,
        }
    }

    /// Whether `agent`'s key is marked unreadable.
    pub(super) fn is_unreadable(&self, agent: &AgentId) -> bool {
        lock(&self.state).unreadable.contains(agent)
    }

    /// The agents whose key is marked unreadable, for `Debug`.
    pub(super) fn unreadable_agents(&self) -> Vec<AgentId> {
        lock(&self.state).unreadable.iter().cloned().collect()
    }

    /// A write, or a read, found `agent`'s key (or that none is stored): its mark ends.
    pub(super) fn known(&self, agent: &AgentId) {
        let changed = {
            let mut state = lock(&self.state);
            *state.writes.entry(agent.clone()).or_default() += 1;
            state.unreadable.remove(agent)
        };
        self.noted(changed, agent);
    }

    /// A read of `agent`'s stored key failed with the error of `kind`: it is marked.
    pub(super) fn unreadable(&self, agent: &AgentId, kind: &str) {
        tracing::warn!(%agent, kind, "key unreadable");
        let changed = {
            let mut state = lock(&self.state);
            *state.writes.entry(agent.clone()).or_default() += 1;
            state.unreadable.insert(agent.clone())
        };
        self.noted(changed, agent);
    }

    /// A [`KeyNote::Read`] for `agent` when its mark `changed`.
    fn noted(&self, changed: bool, agent: &AgentId) {
        if let Some(notes) = self.notes.as_ref().filter(|_| changed) {
            send(notes, KeyNote::Read(agent.clone()));
        }
    }

    /// Starts a read of `agent`: its write count at the start, or `None` when one runs.
    pub(super) fn begin(&self, agent: &AgentId) -> Option<u64> {
        let mut state = lock(&self.state);
        if !state.reading.insert(agent.clone()) {
            return None;
        }
        Some(state.writes.get(agent).copied().unwrap_or_default())
    }

    /// Takes in a read of `agent` begun at write count `began` that answered `read`; an answer
    /// brings `marker` in step. A change of the mark, or of the key in memory, is noted.
    pub(super) fn finish(
        &self,
        keys: &Keys,
        marker: &KeyMarker,
        agent: &AgentId,
        began: u64,
        read: Result<Option<Zeroizing<String>>, KeyError>,
    ) {
        let (changed, stored) = {
            let mut state = lock(&self.state);
            state.reading.remove(agent);
            if state.writes.get(agent).copied().unwrap_or_default() != began {
                // A write since the read began: its answer stands.
                return;
            }
            match read {
                Ok(Some(key)) => {
                    let new = lock(keys).insert(agent.clone(), key).is_none();
                    (state.unreadable.remove(agent) || new, Some(true))
                }
                Ok(None) => {
                    let gone = lock(keys).remove(agent).is_some();
                    (state.unreadable.remove(agent) || gone, Some(false))
                }
                Err(error) => {
                    tracing::warn!(%agent, kind = error_kind(&error), "key unreadable");
                    (state.unreadable.insert(agent.clone()), None)
                }
            }
        };
        if let Some(stored) = stored {
            marker.reconcile(agent, stored);
        }
        self.noted(changed, agent);
    }

    /// Marks each read still running unreadable when `marked` marks its agent: the first
    /// load's bound has passed. One the marker does not mark has no key Clauseo stored, so it
    /// launches without one; its read goes on and is taken in when it answers.
    fn time_out(&self, marked: &Marked) {
        let late: Vec<AgentId> = {
            let mut state = lock(&self.state);
            let late: Vec<AgentId> = state
                .reading
                .iter()
                .filter(|agent| !state.unreadable.contains(*agent) && marked.contains(agent))
                .cloned()
                .collect();
            state.unreadable.extend(late.iter().cloned());
            late
        };
        for agent in late {
            tracing::warn!(%agent, kind = "timed_out", "key unreadable");
            self.noted(true, &agent);
        }
    }
}

/// Reads each of `agents`' keys from `keychain`, each on a thread of its own, and waits
/// for all of them, blocking: the tests' load, with no bound and no notes.
#[cfg(test)]
pub(super) fn load_all<'a>(
    keychain: Keychain,
    agents: impl IntoIterator<Item = &'a AgentId>,
) -> KeyCache {
    let cache = KeyCache::empty(keychain, None, KeyMarker::unkept());
    if cache.available {
        let reads: Vec<_> = agents.into_iter().map(|a| start_read(&cache, a)).collect();
        futures::executor::block_on(join_all(reads));
    }
    cache
}

/// The first load, on the setup thread: each of `agents`' keys read on a thread of its
/// own, waited for at most `bound` on `timer`; a read that has not answered by then is
/// marked unreadable, when `marker` marks its agent, and goes on. Each change of a mark, then
/// and later, goes to `notes`.
pub(super) fn load_bounded<'a>(
    keychain: Keychain,
    agents: impl IntoIterator<Item = &'a AgentId>,
    marker: KeyMarker,
    notes: NoteSender,
    timer: &impl Timer,
    bound: Duration,
) -> KeyCache {
    let cache = KeyCache::empty(keychain, Some(notes), marker);
    if !cache.available {
        return cache;
    }
    let reads: Vec<_> = agents.into_iter().map(|a| start_read(&cache, a)).collect();
    let all = Box::pin(join_all(reads));
    futures::executor::block_on(select(all, Box::pin(timer.after(bound))));
    // Read once the bound has passed: the reads that answered have brought it in step.
    cache.marks.time_out(&cache.marker.marked());
    cache
}

/// Reads `agent`'s key again when it is marked unreadable and no read of it runs (a launch
/// met `agent.key_unreadable`); a read that succeeds ends the mark. The receiver resolves
/// once the answer is in (at once when no read starts); nobody needs to wait for it.
pub(super) fn read_again(cache: &KeyCache, agent: &AgentId) -> oneshot::Receiver<()> {
    let reading = lock(&cache.marks.state).reading.contains(agent);
    if !cache.marks.is_unreadable(agent) || reading {
        // Nothing to read again, or the read that timed out still waits: its answer is taken
        // in when it comes.
        return oneshot::channel().1;
    }
    tracing::info!(%agent, "a stored API key is read again");
    start_read(cache, agent)
}

/// Starts a read of `agent`'s key on a thread of its own; the receiver resolves once its
/// answer is in the cache (or at once, when one runs already or no thread starts).
fn start_read(cache: &KeyCache, agent: &AgentId) -> oneshot::Receiver<()> {
    let (done, finished) = oneshot::channel();
    let Some(began) = cache.marks.begin(agent) else {
        return finished;
    };
    let (keychain, keys, order) = cache.parts();
    let marks = cache.marks.clone();
    let marker = Arc::clone(&cache.marker);
    let for_thread = agent.clone();
    let spawned = std::thread::Builder::new()
        .name("clauseo-keychain".to_owned())
        .spawn(move || {
            let read = keychain.load(&for_thread);
            {
                let _order = lock(&order);
                marks.finish(&keys, &marker, &for_thread, began, read);
            }
            // The load may have stopped waiting.
            let _gone = done.send(());
        });
    if spawned.is_err() {
        lock(&cache.marks.state).reading.remove(agent);
        cache.marks.unreadable(agent, "no_thread");
    }
    finished
}
