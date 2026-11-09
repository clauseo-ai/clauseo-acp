//! Whether the data fence compares by names only (decision record 0030, "Network locations"):
//! the data folder's own identity, or one of its parents', can't be read. One per host, shared
//! by every fence check (on the matter threads, which read the disk) and the actor (which only
//! loads the flag, `ProtectionView::data_fence_names_only`, and never touches the disk).
//!
//! - Cached success, re-verified: once the data folder's identity and its parents' have all been
//!   read, they are kept, and each fence check reads the data folder's own identity again (one
//!   read). When it differs (a data folder removed and made again has a new identity) or can't be
//!   read, every one is read again, so a recreated data folder is never compared on a stale
//!   identity while the flag says healthy.
//! - Retry on failure: while one can't be read, each fence check reads that one again (one
//!   identity read per check). When it reads, every one is read again; until then the check
//!   compares by names, with the identities that did read.
//! - Only the data folder's own path and its parents (the host's configured data folder,
//!   resolved) are ever kept here: nothing a request names.
//! - Each result is recorded with the sequence number its check took as it started
//!   ([`DataFenceHealth::begin`]): an older check finishing late never overwrites a newer one's
//!   result, so the flag doesn't flicker.
//! - One `warn` when the names-only state starts, one when it clears, never one per request,
//!   and never a path.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// One file's identity (device or volume, and file number).
pub(super) type Id = (u64, u128);

/// The identities read for the data folder at `real_data`.
#[derive(Debug, Clone, Default)]
pub(super) struct Captured {
    /// The data folder's resolved path the identities belong to.
    pub(super) real_data: PathBuf,
    /// The data folder's own identity; `None` when it is missing or can't be read.
    pub(super) data: Option<Id>,
    /// The identities of the data folder and its folders above it that could be read.
    pub(super) line: Vec<Id>,
    /// The first of them (the data folder, then its parents nearest first) whose identity
    /// can't be read; `None` when every one read.
    pub(super) failing: Option<PathBuf>,
    /// Whether every one read and the data folder exists: a capture to keep.
    pub(super) complete: bool,
}

#[derive(Debug, Default)]
struct Shared {
    /// The last capture, kept while it is complete or failing, and the sequence number of the
    /// check that recorded it; read and replaced without the lock held across any disk read.
    captured: Mutex<(u64, Option<Captured>)>,
    /// The next check's sequence number ([`DataFenceHealth::begin`]).
    next: AtomicU64,
    /// Whether the fence compares by names only now.
    names_only: AtomicBool,
}

/// The host's data-fence health: cheap to clone, every clone the same state.
#[derive(Debug, Clone, Default)]
pub(crate) struct DataFenceHealth(Arc<Shared>);

impl DataFenceHealth {
    /// Whether the data fence compares by names only now (no disk access: the actor calls it).
    pub(crate) fn names_only(&self) -> bool {
        self.0.names_only.load(Ordering::Relaxed)
    }

    /// The kept capture for `real_data`, if any.
    pub(super) fn kept(&self, real_data: &Path) -> Option<Captured> {
        let captured = self
            .0
            .captured
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        captured
            .1
            .as_ref()
            .filter(|captured| captured.real_data == real_data)
            .cloned()
    }

    /// The sequence number of a check starting now, for [`Self::record`]: each one larger than the
    /// last (numbers start at 1).
    pub(super) fn begin(&self) -> u64 {
        self.0
            .next
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }

    /// Keeps `captured` (when complete or failing) and sets the flag, logging when it flips,
    /// unless a check that started later (`sequence` from [`Self::begin`]) has recorded already:
    /// an older result finishing late is dropped.
    pub(super) fn record(&self, sequence: u64, captured: Captured) {
        let failing = captured.failing.is_some();
        let keep = captured.complete || failing;
        let mut slot = self
            .0
            .captured
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if sequence < slot.0 {
            return;
        }
        *slot = (sequence, keep.then_some(captured));
        // Flipped under the lock, so two records can't leave the flag out of step with the kept
        // capture.
        let was = self.0.names_only.swap(failing, Ordering::Relaxed);
        drop(slot);
        if failing && !was {
            tracing::warn!(
                "the data folder's identity, or a parent's, can't be read; the data fence \
                 compares by names only until it reads again"
            );
        } else if was && !failing {
            tracing::warn!(
                "the data folder's identity reads again; the data fence compares by identity"
            );
        }
    }

    /// Puts the health in the names-only state, or out of it, as a fence check would.
    #[cfg(test)]
    pub(crate) fn set_names_only(&self, names_only: bool) {
        self.record(
            self.begin(),
            Captured {
                failing: names_only.then(|| PathBuf::from("/")),
                complete: !names_only,
                data: Some((1, 1)),
                ..Captured::default()
            },
        );
    }
}
