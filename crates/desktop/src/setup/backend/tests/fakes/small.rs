//! The small fakes: the usage counts and the account reads.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use clauseo_agents::{AccountFuture, AccountHash, AccountRead};
use clauseo_counts::schema::Event;
use clauseo_protocol::AgentId;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;

use super::super::super::SetupCounts;
use crate::bridge::lock;

/// The usage counts, recording each event.
#[derive(Default)]
pub(in crate::setup::backend) struct FakeCounts(pub(in crate::setup::backend) Mutex<Vec<Event>>);

impl SetupCounts for FakeCounts {
    fn bump(&self, event: Event) {
        lock(&self.0).push(event);
    }
}

/// What S18's `read_account` answers, by agent: `Failed` unless a test set an account; a read
/// that never answers while `stalled`.
#[derive(Default)]
pub(in crate::setup::backend) struct FakeAccounts {
    reads: Arc<Mutex<BTreeMap<AgentId, AccountRead>>>,
    stalled: Mutex<bool>,
    /// The agents whose reads wait for a test to open them ([`FakeAccounts::hold`]).
    held: Mutex<BTreeMap<AgentId, Shared<oneshot::Receiver<()>>>>,
    /// Whether each read so far was forced (`read_account_refreshed`), in order.
    pub(in crate::setup::backend) forced: Mutex<Vec<(AgentId, bool)>>,
    /// What a forced read answers, by agent, when it differs from the plain read's.
    forced_reads: Mutex<BTreeMap<AgentId, AccountRead>>,
    /// Forced reads running now, and the most that ran at once.
    pub(in crate::setup::backend) running: Arc<Mutex<(usize, usize)>>,
    /// What a forced read of each agent does as it starts, as Codex's refresh rewrites
    /// `auth.json` before it answers.
    on_forced: Mutex<BTreeMap<AgentId, Arc<dyn Fn() + Send + Sync>>>,
    /// The agents whose forced reads never answer (a stalled network).
    hung: Mutex<BTreeSet<AgentId>>,
    /// Hung forced reads dropped unanswered: what kills the real read's app-server.
    pub(in crate::setup::backend) dropped: Arc<AtomicUsize>,
}

/// Counts its drop.
struct Dropped(Arc<AtomicUsize>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl FakeAccounts {
    /// `agent`'s status names `email` from now on.
    pub(in crate::setup::backend) fn names(&self, agent: &AgentId, email: &str) {
        let read = AccountRead::Account(AccountHash::of(email).unwrap());
        lock(&self.reads).insert(agent.clone(), read);
    }

    /// `agent`'s status answers `read` from now on.
    pub(in crate::setup::backend) fn answers(&self, agent: &AgentId, read: AccountRead) {
        lock(&self.reads).insert(agent.clone(), read);
    }

    /// `agent`'s reads from now on wait until the sender this gives is used (or dropped),
    /// then answer what `agent`'s status names then.
    pub(in crate::setup::backend) fn hold(&self, agent: &AgentId) -> oneshot::Sender<()> {
        let (open, opened) = oneshot::channel();
        lock(&self.held).insert(agent.clone(), opened.shared());
        open
    }

    /// `agent`'s forced reads answer `read` from now on (its plain ones are unchanged).
    pub(in crate::setup::backend) fn forced_answers(&self, agent: &AgentId, read: AccountRead) {
        lock(&self.forced_reads).insert(agent.clone(), read);
    }

    /// `agent`'s forced reads run `then` as they start, before they answer.
    pub(in crate::setup::backend) fn on_forced(
        &self,
        agent: &AgentId,
        then: impl Fn() + Send + Sync + 'static,
    ) {
        lock(&self.on_forced).insert(agent.clone(), Arc::new(then));
    }

    /// `agent`'s forced reads never answer from now on.
    pub(in crate::setup::backend) fn hang_forced(&self, agent: &AgentId) {
        lock(&self.hung).insert(agent.clone());
    }

    /// How many of `agent`'s reads so far were forced, and how many were plain.
    pub(in crate::setup::backend) fn forced_and_plain(&self, agent: &AgentId) -> (usize, usize) {
        let forced = lock(&self.forced);
        let of = forced.iter().filter(|(who, _)| who == agent);
        let count = of.clone().filter(|(_, forced)| *forced).count();
        (count, of.count() - count)
    }

    /// Every agent's status answers nothing set (a failed read), and none is held.
    pub(in crate::setup::backend) fn clear(&self) {
        lock(&self.reads).clear();
        lock(&self.held).clear();
        lock(&self.forced_reads).clear();
        lock(&self.forced).clear();
        lock(&self.on_forced).clear();
        lock(&self.hung).clear();
    }

    /// Whether reads never answer.
    pub(in crate::setup::backend) fn stall(&self, stalled: bool) {
        *lock(&self.stalled) = stalled;
    }

    pub(in crate::setup::backend) fn read(&self, agent: &AgentId, forced: bool) -> AccountFuture {
        lock(&self.forced).push((agent.clone(), forced));
        if *lock(&self.stalled) {
            return Box::pin(futures::future::pending());
        }
        if forced {
            let then = lock(&self.on_forced).get(agent).cloned();
            if let Some(then) = then {
                then();
            }
            if lock(&self.hung).contains(agent) {
                let dropped = Dropped(Arc::clone(&self.dropped));
                return Box::pin(async move {
                    let _dropped = dropped;
                    futures::future::pending().await
                });
            }
        }
        let special = forced
            .then(|| lock(&self.forced_reads).get(agent).cloned())
            .flatten();
        let running = Arc::clone(&self.running);
        if forced {
            let mut now = lock(&running);
            now.0 += 1;
            now.1 = now.1.max(now.0);
        }
        let reads = Arc::clone(&self.reads);
        let opened = lock(&self.held).get(agent).cloned();
        let agent = agent.clone();
        Box::pin(async move {
            if let Some(opened) = opened {
                let _open = opened.await;
            }
            let read = special.or_else(|| lock(&reads).get(&agent).cloned());
            if forced {
                lock(&running).0 -= 1;
            }
            read.unwrap_or(AccountRead::Failed)
        })
    }
}
