//! The small fakes: the usage counts, the confirms, the opener, and the account reads.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use clauseo_agents::{AccountFuture, AccountHash, AccountRead};
use clauseo_counts::schema::Event;
use clauseo_protocol::{AgentId, HostError};
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;

use super::super::super::SetupCounts;
use crate::bridge::lock;
use crate::dialogs::BoxFuture;
use crate::open::Opener;
use crate::setup::confirm::{Ask, SetupConfirm};

/// The usage counts, recording each event.
#[derive(Default)]
pub(in crate::setup::backend) struct FakeCounts(pub(in crate::setup::backend) Mutex<Vec<Event>>);

impl SetupCounts for FakeCounts {
    fn bump(&self, event: Event) {
        lock(&self.0).push(event);
    }
}

/// The confirms, answered `answer`, each ask recorded.
pub(in crate::setup::backend) struct FakeConfirm {
    pub(in crate::setup::backend) answer: bool,
    pub(in crate::setup::backend) asked: Mutex<Vec<Ask>>,
}

impl SetupConfirm for FakeConfirm {
    fn confirm(&self, ask: Ask) -> BoxFuture<'_, bool> {
        lock(&self.asked).push(ask);
        Box::pin(std::future::ready(self.answer))
    }
}

/// The opener, each URL recorded.
#[derive(Default)]
pub(in crate::setup::backend) struct FakeOpener(pub(in crate::setup::backend) Mutex<Vec<String>>);

impl Opener for FakeOpener {
    fn open_url(&self, url: &str) -> Result<(), HostError> {
        lock(&self.0).push(url.to_owned());
        Ok(())
    }
    fn open_path(&self, _path: &Path, _kind: &str) -> Result<(), HostError> {
        Ok(())
    }
    fn reveal(&self, _path: &Path) -> Result<(), HostError> {
        Ok(())
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

    /// Every agent's status answers nothing set (a failed read), and none is held.
    pub(in crate::setup::backend) fn clear(&self) {
        lock(&self.reads).clear();
        lock(&self.held).clear();
    }

    /// Whether reads never answer.
    pub(in crate::setup::backend) fn stall(&self, stalled: bool) {
        *lock(&self.stalled) = stalled;
    }

    pub(in crate::setup::backend) fn read(&self, agent: &AgentId) -> AccountFuture {
        if *lock(&self.stalled) {
            return Box::pin(futures::future::pending());
        }
        if let Some(opened) = lock(&self.held).get(agent).cloned() {
            let reads = Arc::clone(&self.reads);
            let agent = agent.clone();
            return Box::pin(async move {
                let _open = opened.await;
                let read = lock(&reads).get(&agent).cloned();
                read.unwrap_or(AccountRead::Failed)
            });
        }
        let read = lock(&self.reads).get(agent).cloned();
        Box::pin(std::future::ready(read.unwrap_or(AccountRead::Failed)))
    }
}
