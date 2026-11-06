//! The small fakes: the usage counts, the confirms, the opener, and the account reads.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use clauseo_agents::{AccountFuture, AccountHash, AccountRead};
use clauseo_counts::schema::Event;
use clauseo_protocol::{AgentId, HostError};

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
    reads: Mutex<BTreeMap<AgentId, AccountRead>>,
    stalled: Mutex<bool>,
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

    /// Whether reads never answer.
    pub(in crate::setup::backend) fn stall(&self, stalled: bool) {
        *lock(&self.stalled) = stalled;
    }

    pub(in crate::setup::backend) fn read(&self, agent: &AgentId) -> AccountFuture {
        if *lock(&self.stalled) {
            return Box::pin(futures::future::pending());
        }
        let read = lock(&self.reads).get(agent).cloned();
        Box::pin(std::future::ready(read.unwrap_or(AccountRead::Failed)))
    }
}
