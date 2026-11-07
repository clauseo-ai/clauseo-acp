//! Refresh-on-check for Codex (R17, decision record 0048): a Codex check's `account/read` may
//! ask Codex to refresh the login's tokens with `OpenAI` first (`refreshToken: true`), which
//! closes the token-only swap (a swapped access and refresh token under the old `id_token`
//! would otherwise read as the old account until Codex's next refresh).
//!
//! - **Force only when needed.** The last confirmed stamp of Codex's `auth.json` (its file id,
//!   last write, status change and size, from metadata only, never the contents) is kept per
//!   agent. A check forces a refresh only on its first check since Clauseo started, or when the
//!   stamp is not the confirmed one; with the stamp unchanged it reads as before, with no
//!   refresh. A forced check that confirms the recorded account keeps the stamp after the read
//!   (Codex's refresh rewrites the file in place), so Clauseo's own rewrite forces nothing.
//! - **The 60 s guard.** Forcing, a login file whose last write is under [`GUARD`] old (a
//!   session's own refresh just wrote it) is read without a refresh now, and that answer counts
//!   for this check; one forced re-check is due at its last write plus [`GUARD`]. If the file
//!   changed again by then, the re-check reads without a refresh and nothing more is due.
//! - **No answer is not a mismatch.** A forced read with no answer (offline, a timeout, a proxy
//!   down, a run that did not start) falls back to the read without a refresh, which decides as
//!   before; one forced re-check is due [`RETRY_AFTER`] later (a re-check after no answer is not
//!   retried again). Only a definite answer refuses: another account, or a refresh refused (an
//!   auth error, `refresh_token_reused`), which S18 reads as no account.
//! - **One forced refresh at a time in Clauseo.** Codex 0.162.1 takes no lock across processes
//!   for a `ChatGPT` login, so two refreshes at once would both send the same refresh token.
//!   One lock covers every forced refresh of the process (the app has one [`Refresh`], in its
//!   one `Identities`): a caller for the agent whose refresh is under way waits for it and takes
//!   its answer; a caller for another waits its turn. Sessions refreshing on their own are
//!   outside it; the guard and the lock narrow that race, they do not close it.
//!
//! Claude is unchanged: its checks never force anything.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use clauseo_agents::{AccountFuture, AccountRead};
use clauseo_protocol::AgentId;
use futures::future::{FutureExt as _, Shared};

use super::Stamp;
use crate::bridge::lock;

/// How old Codex's login file must be before a check forces a refresh.
pub(crate) const GUARD: Duration = Duration::from_secs(60);
/// How long after a forced read with no answer the one forced re-check is due.
pub(crate) const RETRY_AFTER: Duration = Duration::from_secs(60);

/// How a check reads the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Plan {
    /// As before: no refresh.
    Plain,
    /// With a forced refresh. `retry` says whether a forced read with no answer may make one
    /// re-check due (not when this one is that re-check).
    Forced { retry: bool },
}

/// A forced re-check due at `at` (the clock's monotonic time), for the login file at `stamp`.
#[derive(Debug, Clone, Copy)]
struct Due {
    at: Duration,
    stamp: Stamp,
    /// Made after a forced read with no answer: its own no answer makes nothing more due.
    after_no_answer: bool,
    /// A look found it come and asked for its check (the look's clock may run ahead of the
    /// check's).
    fired: bool,
}

#[derive(Default)]
struct Inner {
    /// The login file's stamp after the last forced check that confirmed the account.
    confirmed: BTreeMap<AgentId, Stamp>,
    /// The one forced re-check due per agent.
    due: BTreeMap<AgentId, Due>,
    /// The forced read under way per agent, which a second caller for it takes.
    flying: BTreeMap<AgentId, Shared<AccountFuture>>,
}

/// Refresh-on-check's state: see the module docs.
pub(crate) struct Refresh {
    /// Whether Codex's checks force refreshes (on in the app; tests of other parts turn it off).
    pub(crate) on: bool,
    inner: Arc<Mutex<Inner>>,
    /// Held through each forced read: one at a time in the process.
    one_at_a_time: Arc<futures::lock::Mutex<()>>,
}

impl Default for Refresh {
    fn default() -> Self {
        Self {
            on: true,
            inner: Arc::default(),
            one_at_a_time: Arc::default(),
        }
    }
}

impl Refresh {
    /// How a check of `agent` reads, its Codex login file at `stamp` with last write
    /// `modified`, at wall time `wall` and monotonic time `now`. Takes a due re-check that has
    /// come; makes one due when the guard holds the refresh off. Logs by fixed words.
    pub(crate) fn plan(
        &self,
        agent: &AgentId,
        stamp: Stamp,
        modified: Option<SystemTime>,
        wall: SystemTime,
        now: Duration,
    ) -> Plan {
        let mut inner = lock(&self.inner);
        if let Some(due) = inner.due.get(agent).copied()
            && (due.fired || now >= due.at)
        {
            inner.due.remove(agent);
            if due.stamp == stamp {
                tracing::info!(%agent, "identity: forced re-check");
                return Plan::Forced {
                    retry: !due.after_no_answer,
                };
            }
            tracing::info!(%agent, "identity: refresh skipped, login changed again");
            return Plan::Plain;
        }
        if inner.confirmed.get(agent) == Some(&stamp) || !matches!(stamp, Stamp::File(_)) {
            return Plan::Plain;
        }
        // A last write in the future (a clock set back) counts as just now.
        let age = modified.map_or(Duration::ZERO, |modified| {
            wall.duration_since(modified).unwrap_or(Duration::ZERO)
        });
        if age < GUARD {
            if !inner.due.contains_key(agent) {
                let due = Due {
                    at: now.saturating_add(GUARD.saturating_sub(age)),
                    stamp,
                    after_no_answer: false,
                    fired: false,
                };
                inner.due.insert(agent.clone(), due);
            }
            tracing::info!(%agent, "identity: refresh skipped, login just changed");
            return Plan::Plain;
        }
        Plan::Forced { retry: true }
    }

    /// Whether a forced re-check of `agent` has come at `now` (a look's time); one that has
    /// is marked so, and the next check takes it.
    pub(crate) fn fire_if_due(&self, agent: &AgentId, now: Duration) -> bool {
        let mut inner = lock(&self.inner);
        let Some(due) = inner.due.get_mut(agent) else {
            return false;
        };
        if now >= due.at {
            due.fired = true;
        }
        due.fired
    }

    /// A forced read with no answer, its login file at `stamp`: one forced re-check is due at
    /// `at`, unless one is already.
    pub(crate) fn retry_at(&self, agent: &AgentId, stamp: Stamp, at: Duration) {
        let due = Due {
            at,
            stamp,
            after_no_answer: true,
            fired: false,
        };
        lock(&self.inner).due.entry(agent.clone()).or_insert(due);
    }

    /// The forced read of `agent`, `read` started only once the process's one forced refresh
    /// at a time is free. A forced read of `agent` under way is taken instead of a second.
    pub(crate) fn forced(
        &self,
        agent: &AgentId,
        read: impl FnOnce() -> AccountFuture + Send + 'static,
    ) -> Shared<AccountFuture> {
        let mut inner = lock(&self.inner);
        if let Some(flying) = inner.flying.get(agent) {
            return flying.clone();
        }
        let one = Arc::clone(&self.one_at_a_time);
        let state = Arc::clone(&self.inner);
        let who = agent.clone();
        let flight: AccountFuture = Box::pin(async move {
            let _one = one.lock().await;
            let answer = read().await;
            // Before the lock is let go: a caller from now on starts the next one.
            lock(&state).flying.remove(&who);
            answer
        });
        let flight = flight.shared();
        inner.flying.insert(agent.clone(), flight.clone());
        flight
    }

    /// A forced read of `agent` was given up at the check's bound: the next caller starts
    /// another, and once its last caller lets go, the read under way is dropped with the lock.
    pub(crate) fn abandon(&self, agent: &AgentId) {
        lock(&self.inner).flying.remove(agent);
    }

    /// A forced check of `agent` ended: when it `confirmed` the account, the login file's
    /// stamp after it (`stamp`) is the confirmed one; else none is, so the next check forces.
    pub(crate) fn forced_ended(&self, agent: &AgentId, confirmed: bool, stamp: Stamp) {
        let mut inner = lock(&self.inner);
        if confirmed {
            inner.confirmed.insert(agent.clone(), stamp);
        } else {
            inner.confirmed.remove(agent);
        }
    }

    /// Whether `read` is a forced read's no answer, which falls back to the plain read.
    pub(crate) fn no_answer(read: &AccountRead) -> bool {
        matches!(read, AccountRead::Failed)
    }
}

/// `path`'s last write, from metadata only, never following a link.
pub(crate) fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

#[cfg(test)]
mod tests;
