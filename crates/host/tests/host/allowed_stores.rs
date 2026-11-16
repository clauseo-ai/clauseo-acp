//! Support for the allowed sites' storage tests (decision record 0053, PR 2): secret-store
//! fakes (denied, gated, flaky) and helpers over a running host.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};

use clauseo_connectors::{MemoryStore, SecretStore, StoreError};
use clauseo_host::AllowedSites;
use clauseo_protocol::{
    AllowScope, CommandOutcome, ErrorCode, HostCommand, HostError, HostQuery, MatterId, QueryResult,
};
use sha2::{Digest as _, Sha256};

use crate::allowed_sites::{allow, notice, start, start_with_secrets, wait_loaded};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

pub(crate) const AGENTS: &[(&str, Scenario)] = &[(FAKE, Scenario::Bundled("prompt-stream"))];
pub(crate) const ACCOUNT: &str = "blocklist-allowed-sha256";

/// A store whose every call fails as a denied Keychain prompt does, counting the calls.
#[derive(Default)]
pub(crate) struct Denied {
    pub(crate) calls: AtomicUsize,
}

impl SecretStore for Denied {
    fn get(&self, _account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(StoreError::NoAccess("denied".to_owned()))
    }

    fn set(&self, _account: &str, _secret: &[u8]) -> Result<(), StoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(StoreError::NoAccess("denied".to_owned()))
    }

    fn delete(&self, _account: &str) -> Result<(), StoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(StoreError::NoAccess("denied".to_owned()))
    }
}

/// A store whose `get` (or `set`) waits until the test releases it, as a Keychain prompt
/// waits on the lawyer; then it answers as `inner` does.
pub(crate) struct Gated {
    inner: Arc<MemoryStore>,
    gate_set: bool,
    stuck: AtomicBool,
    released: Mutex<bool>,
    wake: Condvar,
}

impl Gated {
    /// `inner` behind a gate on `get`, or on `set` when `gate_set`.
    pub(crate) fn new(inner: &Arc<MemoryStore>, gate_set: bool) -> Arc<Gated> {
        Arc::new(Gated {
            inner: Arc::clone(inner),
            gate_set,
            stuck: AtomicBool::new(false),
            released: Mutex::new(false),
            wake: Condvar::new(),
        })
    }

    /// Whether a call is waiting at the gate.
    pub(crate) fn stuck(&self) -> bool {
        self.stuck.load(Ordering::SeqCst)
    }

    fn wait(&self) {
        self.stuck.store(true, Ordering::SeqCst);
        let mut released = self.released.lock().unwrap_or_else(PoisonError::into_inner);
        while !*released {
            released = self
                .wake
                .wait(released)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    pub(crate) fn release(&self) {
        *self.released.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.wake.notify_all();
    }
}

impl SecretStore for Gated {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        if !self.gate_set {
            self.wait();
        }
        self.inner.get(account)
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        if self.gate_set {
            self.wait();
        }
        self.inner.set(account, secret)
    }

    fn delete(&self, account: &str) -> Result<(), StoreError> {
        self.inner.delete(account)
    }
}

pub(crate) fn file(dir: &Dir) -> PathBuf {
    dir.data().join("blocklist/allowed.json")
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _infallible = write!(hex, "{byte:02x}");
            hex
        })
}

/// The set-aside files' names in the block list's folder, sorted.
pub(crate) fn rejected(dir: &Dir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.data().join("blocklist"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("allowed.json.rejected-"))
        .collect();
    names.sort();
    names
}

/// Each listed allow's `saved`, in the list's order.
pub(crate) fn saved(host: &Running) -> Vec<bool> {
    match host.query(HostQuery::AllowedSites).unwrap() {
        QueryResult::AllowedSites(sites) => sites.into_iter().map(|site| site.saved).collect(),
        other => panic!("{other:?}"),
    }
}

pub(crate) fn always(host: &str) -> (String, AllowScope, Option<MatterId>) {
    (host.to_owned(), AllowScope::Always, None)
}

pub(crate) fn revoke(host: &str) -> HostCommand {
    HostCommand::RevokeAllowedSite {
        host: host.to_owned(),
        scope: AllowScope::Always,
        matter: None,
    }
}

/// Allows `site` always, from a fresh notice in a new matter `name`, which must succeed.
pub(crate) fn allow_in(dir: &Dir, host: &Running, name: &str, site: &str) -> MatterId {
    let (matter, answer) = try_allow_in(dir, host, name, site);
    assert_eq!(answer.unwrap(), CommandOutcome::Done);
    matter
}

/// [`allow_in`], answering whatever the host answers.
pub(crate) fn try_allow_in(
    dir: &Dir,
    host: &Running,
    name: &str,
    site: &str,
) -> (MatterId, Result<CommandOutcome, HostError>) {
    let (matter, thread, _) = matter_with_thread(dir, host, name, &[], FAKE);
    let id = notice(host, (matter, thread), site);
    (matter, host.dispatch(allow(id, AllowScope::Always)))
}

/// Allows `site` always from a fresh notice in a new matter `name`, which must answer
/// `blocklist.allow_not_saved`.
pub(crate) fn allow_unsaved(dir: &Dir, host: &Running, name: &str, site: &str) -> MatterId {
    let (matter, answer) = try_allow_in(dir, host, name, site);
    assert_eq!(
        answer.unwrap_err().code(),
        ErrorCode::BlocklistAllowNotSaved
    );
    matter
}

/// A store whose `set` or `delete` fails while asked, else answers as `inner` does.
pub(crate) struct Flaky {
    pub(crate) inner: Arc<MemoryStore>,
    pub(crate) fail_set: AtomicBool,
    pub(crate) fail_delete: AtomicBool,
}

impl Flaky {
    pub(crate) fn new(inner: &Arc<MemoryStore>) -> Arc<Flaky> {
        Arc::new(Flaky {
            inner: Arc::clone(inner),
            fail_set: AtomicBool::new(false),
            fail_delete: AtomicBool::new(false),
        })
    }
}

impl SecretStore for Flaky {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get(account)
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), StoreError> {
        if self.fail_set.load(Ordering::SeqCst) {
            return Err(StoreError::Platform("set fails".to_owned()));
        }
        self.inner.set(account, secret)
    }

    fn delete(&self, account: &str) -> Result<(), StoreError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(StoreError::Platform("delete fails".to_owned()));
        }
        self.inner.delete(account)
    }
}

/// Starts run `run` with `store` as the secret store, waiting for the read.
pub(crate) fn start_on(
    dir: &Dir,
    run: u16,
    store: Arc<dyn SecretStore>,
) -> (Running, Arc<AllowedSites>) {
    let sites = Arc::new(AllowedSites::default());
    let host = start_with_secrets(dir, run, &sites, Some(store));
    wait_loaded(&sites);
    (host, sites)
}

pub(crate) fn restart(
    dir: &Dir,
    run: u16,
    keychain: &Arc<MemoryStore>,
) -> (Running, Arc<AllowedSites>) {
    let sites = Arc::new(AllowedSites::default());
    (start(dir, run, &sites, keychain), sites)
}

pub(crate) fn replace(path: &Path, bytes: &[u8]) {
    crate::write::write(path, bytes);
}
