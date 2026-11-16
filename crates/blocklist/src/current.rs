//! The list in use, shared read-only between the host's updater and every proxy.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use crate::{BlockList, Generated, accept};

/// The list in use. Readers ([`Current::get`]) clone an `Arc` under a read lock held for the
/// clone only, then match without any lock; an update ([`Current::offer`]) swaps the `Arc`
/// under the write lock and drops the old list after releasing it. A request in flight keeps
/// the list it started with, and an update closes nothing.
#[derive(Debug, Default)]
pub struct Current {
    list: RwLock<Arc<BlockList>>,
    /// Whether "no list loaded" was already reported.
    noted: AtomicBool,
}

impl Current {
    /// Starts with `list` ([`BlockList::empty`] for none).
    pub fn new(list: BlockList) -> Current {
        Current {
            list: RwLock::new(Arc::new(list)),
            noted: AtomicBool::new(false),
        }
    }

    /// The list in use now.
    pub fn get(&self) -> Arc<BlockList> {
        // The lock guards only an `Arc` swap, valid whatever a panicking thread left.
        Arc::clone(&self.list.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// When the list in use was made; `None` when none is loaded.
    pub fn generated(&self) -> Option<Generated> {
        self.get().generated()
    }

    /// Puts `list` in use if the rollback rule ([`accept`]) lets it replace the list in use,
    /// given the bundled snapshot's time. True when it did. The check and the swap happen under
    /// one write lock, so two offers cannot both pass against the same old list.
    pub fn offer(&self, list: BlockList, bundled: Option<Generated>) -> bool {
        let Some(candidate) = list.generated() else {
            return false;
        };
        let new = Arc::new(list);
        let old = {
            let mut slot = self.list.write().unwrap_or_else(PoisonError::into_inner);
            if !accept(candidate, slot.generated(), bundled) {
                return false;
            }
            std::mem::replace(&mut *slot, new)
        };
        drop(old);
        true
    }

    /// True the first time it is called while no list is loaded, false after that and whenever
    /// a list is loaded: so "the block list is not loaded" is reported once per `Current`.
    pub fn first_unloaded_notice(&self) -> bool {
        self.get().is_unloaded() && !self.noted.swap(true, Ordering::Relaxed)
    }
}
