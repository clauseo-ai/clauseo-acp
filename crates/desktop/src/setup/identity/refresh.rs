//! Refresh-on-check for Codex (R17, decision record 0048): a Codex check's `account/read` may
//! ask Codex to refresh the login's tokens with `OpenAI` first (`refreshToken: true`), which
//! closes the token-only swap (a swapped access and refresh token under the old `id_token`
//! would otherwise read as the old account until Codex's next refresh).
//!
//! - **Pending until forced.** The last confirmed stamp of Codex's `auth.json` (its file id,
//!   last write, status change and size, from metadata only, never the contents) is kept per
//!   agent. A stamp that is not it is pending, and only a forced read confirms it: a read
//!   without a refresh may let this launch go, but never confirms. With the stamp confirmed a
//!   check reads as before, with no refresh.
//! - **Confirmed takes a rewrite and the same account.** Codex 0.162.1 answers a forced
//!   `account/read` from the stored `id_token` whatever the refresh did, but every refresh that
//!   succeeds rewrites `auth.json` in place. So a forced read confirms only when the stamp
//!   advanced during it (another stamp just after the reply than just before the request) and
//!   is no later than the reply ([`confirms`]), and one plain read of the rewritten file then
//!   names the recorded account; that stamp is the confirmed one, so Clauseo's own rewrite
//!   forces nothing. Another account there is a change.
//! - **An answer with no rewrite is a refresh that failed with the network up.** It confirms
//!   nothing, its answer decides this launch as a plain read's would, and one forced retry is
//!   due [`UNWRITTEN_RETRY`] later (the 60 s rate waived for it). The same again is unknown:
//!   launches refused, and the card says sign in again. No answer in between changes nothing.
//! - **At most one forced refresh per [`GUARD`].** While pending, a check forces once
//!   [`GUARD`] has passed since Clauseo's last forced refresh (at once if it has), however
//!   recently or often the file was written: constant writes delay a force by [`GUARD`] at
//!   most. Inside it the check reads without a refresh, and a look asks for the forced check
//!   once it is due ([`Refresh::fire_if_due`]).
//! - **No answer is not a mismatch.** A forced read with no answer (offline, a timeout at its
//!   share of the check's bound, after which it is let go: its app-server's stdin is closed, and
//!   it drains and exits on its own, one per home at most; a run that did not
//!   start, or any JSON-RPC error but Codex's refused login) falls back to the plain read,
//!   which decides this launch. Such a read leaves the stamp pending and forcing goes on with
//!   [`backoff`] (30 s, 1 min, 2 min, 5 min, then every 5 min). Codex's refused login
//!   (`AccountRead::Refused`) is unknown, launches refused and the card saying sign in again,
//!   with the same backoff.
//! - **One forced refresh at a time in Clauseo.** Codex 0.162.1 takes no lock across processes
//!   for a `ChatGPT` login, so two refreshes at once would both send the same refresh token.
//!   One lock covers every forced refresh of the process (the app has one [`Refresh`], in its
//!   one `Identities`): a caller for the agent whose refresh is under way waits for it and takes
//!   its answer; a caller for another waits its turn. Sessions refreshing on their own are
//!   outside it; the rate and the lock narrow that race, they do not close it.
//!
//! Claude is unchanged: its checks never force anything.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use clauseo_agents::{AccountFuture, AccountRead, Clock, SystemClock};
use clauseo_protocol::AgentId;
use futures::future::{FutureExt as _, Shared};

use super::Stamp;
use super::watch::stamp;
use crate::bridge::lock;

/// The least time between two of Clauseo's forced refreshes of one login.
pub(crate) const GUARD: Duration = Duration::from_secs(60);
/// How long after a forced read answered with no rewrite its one retry is due.
pub(crate) const UNWRITTEN_RETRY: Duration = Duration::from_secs(30);

/// How long after the `misses`th forced read in a row that did not confirm (no answer, or a
/// refusal) the next one is due at the earliest: 30 s, 1 min, 2 min, 5 min, then every 5 min.
pub(crate) fn backoff(misses: u32) -> Duration {
    let secs = match misses {
        0 | 1 => 30,
        2 => 60,
        3 => 120,
        _ => 300,
    };
    Duration::from_secs(secs)
}

/// The share of a check's bound a forced read may take, the rest kept for its plain fallback:
/// five eighths (25 s of 40), and a plain read needs no network.
pub(crate) fn forced_share(bound: Duration) -> Duration {
    bound / 8 * 5
}

/// How a check reads the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Plan {
    /// As before: no refresh.
    Plain,
    /// With a forced refresh.
    Forced,
}

/// How a forced check ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    /// The login file was rewritten to this stamp, and its plain read named the recorded
    /// account.
    Confirmed(Stamp),
    /// It was rewritten, but its plain read did not confirm (another account, an unknown one,
    /// no answer): still pending, forced again once [`GUARD`] has passed.
    Unconfirmed,
    /// No answer, or a refused login: still pending, nothing else changed, forced again after
    /// [`backoff`].
    Missed,
    /// An answer with no rewrite: still pending; the first is retried after
    /// [`UNWRITTEN_RETRY`], and a second (unknown) goes on with [`backoff`].
    Unwritten,
}

/// A forced read's answer, the login file's stamps just before its request and just after its
/// reply, and the files' clock when the reply came.
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    pub(crate) read: AccountRead,
    pub(crate) before: Stamp,
    pub(crate) after: Stamp,
    pub(crate) replied: SystemTime,
}

impl Reply {
    /// Whether Codex rewrote the login file during the read: see [`confirms`].
    pub(crate) fn rewrote(&self) -> bool {
        self.after != self.before && confirms(self.after, self.replied)
    }
}

type Flight = Pin<Box<dyn Future<Output = Reply> + Send>>;

/// One agent's refresh state.
#[derive(Debug, Default)]
struct State {
    /// The login file's stamp after the last forced check that confirmed the account.
    confirmed: Option<Stamp>,
    /// The last check saw a stamp that is not the confirmed one.
    pending: bool,
    /// When Clauseo's last forced read of it started (the clock's monotonic time).
    last_forced: Option<Duration>,
    /// Forced reads in a row that did not answer or were refused.
    misses: u32,
    /// The earliest the next forced read may run after the last miss.
    retry: Option<Retry>,
    /// The last forced read that answered did so with no rewrite.
    unwritten: bool,
    /// A look found the forced check due and asked for it (a look's clock may run ahead of
    /// the check's).
    fired: bool,
}

/// The next forced read's retry.
#[derive(Debug, Clone, Copy)]
struct Retry {
    at: Duration,
    /// The retry after an answer with no rewrite: due at `at`, the rate waived.
    waives_rate: bool,
}

impl State {
    /// When the next forced read is due: [`GUARD`] after the last one, and not before the
    /// backoff's retry.
    fn due(&self) -> Duration {
        let retry = self.retry.map_or(Duration::ZERO, |retry| retry.at);
        if self.retry.is_some_and(|retry| retry.waives_rate) {
            return retry;
        }
        let rate = self
            .last_forced
            .map_or(Duration::ZERO, |at| at.saturating_add(GUARD));
        rate.max(retry)
    }
}

#[derive(Default)]
struct Inner {
    /// The latest monotonic time any look or check gave: a look's clock may run ahead of the
    /// check's (the tests'), and a forced read ends after it starts.
    seen: Duration,
    agents: BTreeMap<AgentId, State>,
    /// The forced read under way per agent, which a second caller for it takes.
    flying: BTreeMap<AgentId, Shared<Flight>>,
}

/// Refresh-on-check's state: see the module docs.
pub(crate) struct Refresh {
    /// Whether Codex's checks force refreshes (on in the app; tests of other parts turn it off).
    pub(crate) on: bool,
    /// The clock file times are stamped by, read when a forced reply comes (the system's;
    /// tests set one behind the files).
    pub(crate) files_clock: Arc<dyn Clock>,
    inner: Arc<Mutex<Inner>>,
    /// Held through each forced read: one at a time in the process.
    one_at_a_time: Arc<futures::lock::Mutex<()>>,
}

impl Default for Refresh {
    fn default() -> Self {
        Self {
            on: true,
            files_clock: Arc::new(SystemClock),
            inner: Arc::default(),
            one_at_a_time: Arc::default(),
        }
    }
}

impl Inner {
    /// `now`, or a later time already seen; kept as seen.
    fn at(&mut self, now: Duration) -> Duration {
        self.seen = self.seen.max(now);
        self.seen
    }
}

impl Refresh {
    /// A look at `now`: refresh-on-check's times are never earlier from here on.
    pub(crate) fn saw(&self, now: Duration) {
        lock(&self.inner).at(now);
    }

    /// How a check of `agent` reads, its Codex login file at `stamp`, at the clock's monotonic
    /// time `now`: forced when the stamp is pending and a forced read is due, which then counts
    /// as Clauseo's last. Logs by fixed words.
    pub(crate) fn plan(&self, agent: &AgentId, stamp: Stamp, now: Duration) -> Plan {
        let mut inner = lock(&self.inner);
        let now = inner.at(now);
        let state = inner.agents.entry(agent.clone()).or_default();
        // No login file to refresh, or the confirmed one: nothing pending.
        if !matches!(stamp, Stamp::File(_)) || state.confirmed == Some(stamp) {
            state.pending = false;
            state.fired = false;
            return Plan::Plain;
        }
        state.pending = true;
        if state.fired || now >= state.due() {
            state.fired = false;
            if let Some(retry) = &mut state.retry {
                retry.waives_rate = false;
            }
            state.last_forced = Some(now);
            return Plan::Forced;
        }
        tracing::info!(%agent, "identity: refresh held; login pending a forced check");
        Plan::Plain
    }

    /// Whether a forced check of `agent` is due at `now` (a look's time) while its stamp is
    /// pending; one that is is marked so, and the next check forces.
    pub(crate) fn fire_if_due(&self, agent: &AgentId, now: Duration) -> bool {
        let mut inner = lock(&self.inner);
        let now = inner.at(now);
        let Some(state) = inner.agents.get_mut(agent) else {
            return false;
        };
        if state.pending && now >= state.due() {
            state.fired = true;
        }
        state.pending && state.fired
    }

    /// A forced check of `agent` ended at `now`: see [`Ended`].
    pub(crate) fn forced_ended(&self, agent: &AgentId, ended: Ended, now: Duration) {
        let mut inner = lock(&self.inner);
        let now = inner.at(now);
        let state = inner.agents.entry(agent.clone()).or_default();
        match ended {
            Ended::Confirmed(stamp) => {
                *state = State {
                    confirmed: Some(stamp),
                    last_forced: state.last_forced,
                    ..State::default()
                };
            }
            Ended::Unconfirmed => {
                state.confirmed = None;
                state.pending = true;
                state.unwritten = false;
            }
            Ended::Unwritten if !state.unwritten => {
                state.pending = true;
                state.unwritten = true;
                state.retry = Some(Retry {
                    at: now.saturating_add(UNWRITTEN_RETRY),
                    waives_rate: true,
                });
                tracing::info!(%agent, "identity: forced refresh wrote nothing; retried once");
            }
            Ended::Unwritten => {
                state.pending = true;
                state.misses = state.misses.saturating_add(1);
                state.retry = Some(Retry {
                    at: now.saturating_add(backoff(state.misses)),
                    waives_rate: false,
                });
            }
            Ended::Missed => {
                state.pending = true;
                state.misses = state.misses.saturating_add(1);
                state.retry = Some(Retry {
                    at: now.saturating_add(backoff(state.misses)),
                    waives_rate: false,
                });
                tracing::info!(%agent, "identity: forced refresh to be tried again");
            }
        }
    }

    /// Whether `agent`'s last forced read that answered did so with no rewrite: another such
    /// answer now is unknown.
    pub(crate) fn unwritten_before(&self, agent: &AgentId) -> bool {
        lock(&self.inner)
            .agents
            .get(agent)
            .is_some_and(|state| state.unwritten)
    }

    /// A sign-in through Clauseo recorded `agent`'s account: what refresh-on-check held against
    /// the login before it (misses, an answer with no rewrite) goes; the rate stays.
    pub(crate) fn signed_in(&self, agent: &AgentId) {
        let mut inner = lock(&self.inner);
        if let Some(state) = inner.agents.get_mut(agent) {
            *state = State {
                confirmed: state.confirmed,
                pending: state.pending,
                last_forced: state.last_forced,
                ..State::default()
            };
        }
    }

    /// The forced read of `agent`, `read` started only once the process's one forced refresh
    /// at a time is free, with the stamps of its login file at `login` around it ([`Reply`]).
    /// A forced read of `agent` under way is taken instead of a second.
    pub(crate) fn forced(
        &self,
        agent: &AgentId,
        login: PathBuf,
        read: impl FnOnce() -> AccountFuture + Send + 'static,
    ) -> Shared<Flight> {
        let mut inner = lock(&self.inner);
        if let Some(flying) = inner.flying.get(agent) {
            return flying.clone();
        }
        let one = Arc::clone(&self.one_at_a_time);
        let state = Arc::clone(&self.inner);
        let clock = Arc::clone(&self.files_clock);
        let who = agent.clone();
        let flight: Flight = Box::pin(async move {
            let _one = one.lock().await;
            let before = stamp(&login);
            let answer = read().await;
            let replied = clock.now();
            let after = stamp(&login);
            // Before the lock is let go: a caller from now on starts the next one.
            lock(&state).flying.remove(&who);
            Reply {
                read: answer,
                before,
                after,
                replied,
            }
        });
        let flight = flight.shared();
        inner.flying.insert(agent.clone(), flight.clone());
        flight
    }

    /// A forced read of `agent` was given up at its share of the check's bound: the next
    /// caller starts another, and once its last caller lets go, the read under way is dropped
    /// with the lock.
    pub(crate) fn abandon(&self, agent: &AgentId) {
        lock(&self.inner).flying.remove(agent);
    }
}

/// Whether the login file at `stamp`, another than before a forced read that replied at
/// `replied` (the files' clock), can be that read's rewrite: its last write and status change
/// are no later than the reply, so no write after the reply is taken for it. Fails closed when
/// either is unknown.
pub(crate) fn confirms(stamp: Stamp, replied: SystemTime) -> bool {
    let Stamp::File(file) = stamp else {
        return false;
    };
    file.last_change().is_some_and(|at| at <= replied)
}

#[cfg(test)]
mod tests;
