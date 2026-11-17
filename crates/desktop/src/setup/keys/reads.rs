//! The key cache's reads and its unreadable marks (decision record 0048, R20 P5b A4).
//!
//! - The first load ([`KeyCache::start_load`]) reads each agent's key on a
//!   `clauseo-keychain` thread of its own, so one prompt nobody answers holds up no other
//!   agent, and waits for them at most the bound on the setup thread's timer. A read that has
//!   not answered by then is marked unreadable and goes on; when it answers it is taken in as
//!   any read is.
//! - A read's answer: a key goes into memory and ends the mark; no item ends the mark (no key
//!   is stored: the agent launches without one); the keychain's error marks the key
//!   unreadable when the "key stored" marker marks the agent, logged by the agent, "key
//!   unreadable" and the error's kind, never "no key". For an agent it does not mark, the error
//!   is taken as a read that timed out is: no key Clauseo stored, so no mark, and the agent
//!   launches without one (M-d, 2026-11-15: a Mac with no login keychain fails every read, and
//!   the card showed "Sign-in didn't finish." in place of Sign in). Only with no firm endpoint
//!   or provider: under a firm plan such a failure refuses as a marked one does (the agent's
//!   `failed` entry, read with the plan by [`KeyCache::unreadable_for`]), so a lawyer's own
//!   sign-in token is never offered to a firm's gateway.
//!   A write since the read began (a key set or cleared) wins: the read's answer is dropped.
//! - [`KeyCache::read_again`]: a key marked unreadable, or whose read failed unmarked (refused
//!   under a firm plan), with no read running, is read again: a launch that met
//!   `agent.key_unreadable` asks it, and so does a sign-in press while the key is unreadable
//!   (`setup/backend.rs`), the card's Try again. A read that timed out still runs: its answer,
//!   once the lawyer answers macOS's prompt, is taken in as any read is.
//! - A read that has not answered by the bound, for an agent the marker does not mark, counts as
//!   a failed read (`failed`) as well: it launches without a key, unless a firm plan, which
//!   refuses it, as a failed read does, until the read answers.
//! - A failed read is of one of two kinds (`ReadState::denied`): one that has not answered by
//!   the bound (`agent.key_unreadable`), or one the keychain answered with its error, a denied
//!   or cancelled prompt say (`agent.key_denied`, [`KeyCache::unreadable_code`]). A read that
//!   timed out and then answers with an error turns into the second.
//! - Each change of a mark, or of its kind, sends a [`KeyNote::Read`].
//! - A read whose thread panics is a failed read (`READ_PANICKED`): `reading` ends with it.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::keys::{KeyError, Keychain};
use clauseo_process::Timer;
use clauseo_protocol::AgentId;
use futures::channel::oneshot;
use futures::future::{Either, join_all, select};
use zeroize::Zeroizing;

use super::marker::{KeyMarker, Marked};
use super::notes::{KeyNote, NoteSender, send};
use super::{KeyCache, Keys, error_kind};
use crate::bridge::lock;

/// The kind a read that panicked is taken in as: a failed read, by the usual rule.
const READ_PANICKED: &str = "a panic";

/// Which agents' keys are unreadable and which are being read, and each agent's writes.
#[derive(Debug, Default)]
struct ReadState {
    unreadable: BTreeSet<AgentId>,
    /// Agents whose read failed with no "key stored" mark: refused only under a firm plan
    /// ([`KeyCache::unreadable_for`]).
    failed: BTreeSet<AgentId>,
    /// Agents in `unreadable` or `failed` whose read the keychain answered with its error (a
    /// denied prompt, say), not one that has not answered: `agent.key_denied`.
    denied: BTreeSet<AgentId>,
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

    /// Whether `agent`'s last read failed with no "key stored" mark (see `ReadState::failed`).
    pub(super) fn is_failed(&self, agent: &AgentId) -> bool {
        lock(&self.state).failed.contains(agent)
    }

    /// Whether `agent`'s failed read was the keychain's error, not one with no answer yet.
    pub(super) fn is_denied(&self, agent: &AgentId) -> bool {
        lock(&self.state).denied.contains(agent)
    }

    /// Whether a read of `agent` runs: the tests'.
    #[cfg(test)]
    pub(super) fn is_reading(&self, agent: &AgentId) -> bool {
        lock(&self.state).reading.contains(agent)
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
            let failed = state.failed.remove(agent);
            let denied = state.denied.remove(agent);
            state.unreadable.remove(agent) || failed || denied
        };
        self.noted(changed, agent);
    }

    /// A read of `agent`'s stored key failed with the error of `kind`: it is marked, denied.
    pub(super) fn unreadable(&self, agent: &AgentId, kind: &str) {
        tracing::warn!(%agent, kind, "key unreadable");
        let changed = {
            let mut state = lock(&self.state);
            *state.writes.entry(agent.clone()).or_default() += 1;
            let denied = state.denied.insert(agent.clone());
            state.unreadable.insert(agent.clone()) || denied
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
        // Read before the state's lock: the marker reads its file.
        let marked = read.is_err() && marker.marked().contains(agent);
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
                    let failed = state.failed.remove(agent) | state.denied.remove(agent);
                    (state.unreadable.remove(agent) || new || failed, Some(true))
                }
                Ok(None) => {
                    let gone = lock(keys).remove(agent).is_some();
                    let failed = state.failed.remove(agent) | state.denied.remove(agent);
                    (
                        state.unreadable.remove(agent) || gone || failed,
                        Some(false),
                    )
                }
                Err(error) if marked => {
                    tracing::warn!(%agent, kind = error_kind(&error), "key unreadable");
                    let failed = state.failed.remove(agent) | state.denied.insert(agent.clone());
                    (state.unreadable.insert(agent.clone()) || failed, None)
                }
                Err(error) => {
                    // As a timed-out read of an unmarked agent (`time_out`): no key Clauseo
                    // stored, so it launches without one.
                    tracing::warn!(
                        %agent,
                        kind = error_kind(&error),
                        "key read failed, no key marked stored: none used unless a firm plan"
                    );
                    let failed =
                        state.failed.insert(agent.clone()) | state.denied.insert(agent.clone());
                    (state.unreadable.remove(agent) || failed, None)
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
    /// launches without one, unless a firm plan: it counts as failed (`ReadState::failed`).
    /// Each read goes on and is taken in when it answers.
    fn time_out(&self, marked: &Marked) {
        let (late, unmarked): (Vec<AgentId>, Vec<AgentId>) = {
            let mut state = lock(&self.state);
            let (late, unmarked): (Vec<AgentId>, Vec<AgentId>) = state
                .reading
                .iter()
                .filter(|agent| {
                    !state.unreadable.contains(*agent) && !state.failed.contains(*agent)
                })
                .cloned()
                .partition(|agent| marked.contains(agent));
            state.unreadable.extend(late.iter().cloned());
            state.failed.extend(unmarked.iter().cloned());
            (late, unmarked)
        };
        for agent in late {
            tracing::warn!(%agent, kind = "timed_out", "key unreadable");
            self.noted(true, &agent);
        }
        for agent in unmarked {
            tracing::warn!(
                %agent,
                kind = "timed_out",
                "key read late, no key marked stored: none used unless a firm plan"
            );
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

/// The first load as one call, [`start_load`] then [`FirstLoad::wait`]: the tests'.
#[cfg(test)]
pub(super) fn load_bounded<'a>(
    keychain: Keychain,
    agents: impl IntoIterator<Item = &'a AgentId>,
    marker: KeyMarker,
    notes: NoteSender,
    timer: &impl Timer,
    bound: Duration,
) -> KeyCache {
    let (cache, load) = start_load(keychain, agents, marker, notes);
    let _outcome = load.wait(&cache, timer, bound);
    cache
}

/// The first load's reads, started by [`start_load`]: each of `agents`' keys read on a thread
/// of its own. Each change of a mark, then and later, goes to `notes`. The cache is usable at
/// once; it holds each key as its read answers.
pub(super) fn start_load<'a>(
    keychain: Keychain,
    agents: impl IntoIterator<Item = &'a AgentId>,
    marker: KeyMarker,
    notes: NoteSender,
) -> (KeyCache, FirstLoad) {
    let cache = KeyCache::empty(keychain, Some(notes), marker);
    let reads = if cache.available {
        agents.into_iter().map(|a| start_read(&cache, a)).collect()
    } else {
        Vec::new()
    };
    (cache, FirstLoad { reads })
}

/// The first load's reads in flight, until [`FirstLoad::wait`].
pub(crate) struct FirstLoad {
    reads: Vec<oneshot::Receiver<()>>,
}

/// How the first load ended, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadOutcome {
    /// This computer has no keychain: nothing was read.
    NoKeychain,
    /// Every read answered within the bound.
    Read,
    /// The bound passed with a read still waiting.
    BoundPassed,
}

impl LoadOutcome {
    /// A fixed word for the log.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NoKeychain => "no_keychain",
            Self::Read => "read",
            Self::BoundPassed => "bound_passed",
        }
    }
}

impl FirstLoad {
    /// Waits for the reads at most `bound` on `timer`, blocking; a read that has not answered
    /// by then is marked unreadable, when the marker marks its agent, and goes on.
    pub(crate) fn wait(self, cache: &KeyCache, timer: &impl Timer, bound: Duration) -> LoadOutcome {
        if !cache.available {
            return LoadOutcome::NoKeychain;
        }
        let all = Box::pin(join_all(self.reads));
        let outcome = match futures::executor::block_on(select(all, Box::pin(timer.after(bound)))) {
            Either::Left(_) => LoadOutcome::Read,
            Either::Right(_) => LoadOutcome::BoundPassed,
        };
        // Read once the bound has passed: the reads that answered have brought it in step.
        cache.marks.time_out(&cache.marker.marked());
        outcome
    }
}

/// Reads `agent`'s key again when it is marked unreadable, or its read failed unmarked, and
/// no read of it runs (a launch met `agent.key_unreadable`, or a sign-in press did, the card's
/// Try again); a read that succeeds ends the mark. The receiver resolves once the answer is in
/// (at once when no read starts); nobody needs to wait for it.
pub(super) fn read_again(cache: &KeyCache, agent: &AgentId) -> oneshot::Receiver<()> {
    let (reading, failed) = {
        let state = lock(&cache.marks.state);
        (state.reading.contains(agent), state.failed.contains(agent))
    };
    if !(cache.marks.is_unreadable(agent) || failed) || reading {
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
            // A read that panics is taken in as a failed one, so `reading` ends and Try again
            // reads again (a build that unwinds; a release build aborts on a panic).
            let read = catch_unwind(AssertUnwindSafe(|| keychain.load(&for_thread)))
                .unwrap_or_else(|_panic| {
                    Err(KeyError::Keychain {
                        agent: for_thread.clone(),
                        kind: READ_PANICKED,
                    })
                });
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
