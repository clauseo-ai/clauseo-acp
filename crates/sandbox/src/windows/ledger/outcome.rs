//! What the ledger's changes and the way back report.

use super::{Entry, EntryId};

/// The entries [`Ledger::mark_applied`] was asked to mark that the file no longer lists; the
/// inner error of its `NotFound` (`io::Error::get_ref` and `downcast_ref`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the ACL ledger no longer lists {} of this launch's entries", .0.len())]
pub struct MissingEntries(pub Vec<EntryId>);

/// What [`reconcile`](super::apply::reconcile) did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// Pending entries whose permission entry was there, now applied.
    pub applied: usize,
    /// Pending entries whose permission entry wasn't there, dropped.
    pub dropped: usize,
    /// Pending entries the check failed on, kept pending.
    pub unchecked: usize,
    /// Pending entries kept pending on purpose: deny entries whose permission entry is there
    /// (their matter's next launch walks them strictly, N4), and entries of a matter whose
    /// hand-back is owed (a retried forget needs them, N5).
    pub deferred: usize,
    /// Entries a rollback or forget was removing when it stopped, whose removal is now finished
    /// and which are dropped (round 4). One of a matter whose hand-back is owed has its
    /// permission entry removed but is kept, and counts as deferred.
    pub removed: usize,
}

/// What [`Ledger::take_back`] does with an entry it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Take {
    /// Remove its permission entry, unless another entry names it too; then drop it.
    Remove,
    /// Drop it without a removal (its permission entry was there before, or never written).
    Drop,
}

/// What [`Ledger::take_back`] did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TakenBack {
    /// Entries whose permission entry was removed; still `removing` until their trees are walked.
    pub removed: Vec<Entry>,
    /// Entries dropped without a removal because another entry names the same permission entry.
    pub shared: usize,
    /// Entries whose removal failed, kept `removing` for the next try.
    pub kept: usize,
}

/// What [`forget_matter`](super::apply::forget_matter) did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Forgotten {
    /// Entries whose permission entry was removed, now dropped.
    pub removed: usize,
    /// Entries whose removal failed, kept for the next try.
    pub kept: usize,
    /// Entries dropped without a removal: another matter's entry names the same entry.
    pub shared: usize,
}
