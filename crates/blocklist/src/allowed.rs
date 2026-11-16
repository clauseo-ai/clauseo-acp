//! The sites the lawyer allowed although the list names them (ADR 0053, PR 2), as one launch's
//! proxy reads them: exact hosts, swapped whole by the host.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};

/// Told the hosts a [`Allowed::set`] removed, after the swap and outside every lock of
/// [`Allowed`]. The proxy closes its open tunnels to them with one.
pub type Watcher = dyn Fn(&BTreeSet<String>) + Send + Sync;

/// One launch's allowed hosts, shared between the host (which swaps the set) and the launch's
/// egress proxy (which reads it per request). A host is matched exactly, as the proxy's
/// `Name::parse` prints it (case folded, one trailing dot dropped, an IDN as punycode, an IPv6
/// address without brackets, as `2606:4700::1111`): an allow covers the host the lawyer's
/// notice named and nothing under it. Readers clone an `Arc` under a read lock held for the
/// clone only, as [`crate::Current`] does, so a swap never touches a request in flight. Each
/// swap bumps [`Allowed::generation`] and tells the watchers what it removed. Empty by default.
#[derive(Default)]
pub struct Allowed {
    hosts: RwLock<Arc<BTreeSet<String>>>,
    generation: AtomicU64,
    watchers: Mutex<Vec<Weak<Watcher>>>,
}

impl fmt::Debug for Allowed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Allowed")
            .field("hosts", &self.get())
            .field("generation", &self.generation())
            .field("watchers", &self.lock_watchers().len())
            .finish()
    }
}

impl Allowed {
    /// The hosts allowed now.
    pub fn get(&self) -> Arc<BTreeSet<String>> {
        // The lock guards only an `Arc` swap, valid whatever a panicking thread left.
        Arc::clone(&self.hosts.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Whether `host` is allowed now, compared exactly.
    pub fn contains(&self, host: &str) -> bool {
        self.get().contains(host)
    }

    /// How many swaps there have been. Read before [`Allowed::get`], an unchanged value later
    /// means the set read is still the one in place.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Calls `watcher` after each later [`Allowed::set`] that removes a host, with the hosts it
    /// removed, for as long as the caller keeps the `Arc`; a dropped one is forgotten at the
    /// next `watch` or `set` that removes a host. Debug prints how many are held.
    pub fn watch(&self, watcher: &Arc<Watcher>) {
        let mut watchers = self.lock_watchers();
        watchers.retain(|watcher| watcher.strong_count() > 0);
        watchers.push(Arc::downgrade(watcher));
    }

    fn lock_watchers(&self) -> std::sync::MutexGuard<'_, Vec<Weak<Watcher>>> {
        // A list of weak pointers, valid whatever a panicking thread left.
        self.watchers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Puts `hosts` in place of the set, then tells the watchers the hosts it removed. The old
    /// set is dropped, and the watchers run, after the lock is released.
    pub fn set(&self, hosts: BTreeSet<String>) {
        let new = Arc::new(hosts);
        let old = {
            let mut guard = self.hosts.write().unwrap_or_else(PoisonError::into_inner);
            self.generation.fetch_add(1, Ordering::SeqCst);
            std::mem::replace(&mut *guard, Arc::clone(&new))
        };
        let removed: BTreeSet<String> = old.difference(&new).cloned().collect();
        drop(old);
        if removed.is_empty() {
            return;
        }
        let live: Vec<Arc<Watcher>> = {
            let mut watchers = self.lock_watchers();
            watchers.retain(|watcher| watcher.strong_count() > 0);
            watchers.iter().filter_map(Weak::upgrade).collect()
        };
        for watcher in live {
            watcher(&removed);
        }
    }
}
