//! Fast later launches (S15 `PR3d`): the clean-close record and the launch check.
//!
//! A later launch's cost is the strict walk of the matter, state and notes trees (`apply`), and
//! the set-aside's scan. Both are deterministic in the plan's walk inputs and the trees'
//! names, file IDs and descriptors. So when a session ends clean, a record of the trees' file
//! IDs and the volume's change-journal cursor is kept ([`write_clean_close`]), and the next
//! launch reads the journal from that cursor ([`check_launch`]):
//!
//! - nothing in the trees changed: the own-tree walks and the set-aside scan are skipped
//!   ([`LaunchPath::Skipped`]);
//! - a few items changed: only their folders are walked and re-probed
//!   ([`LaunchPath::Targeted`]);
//! - anything doubtful: today's full walk and full scan ([`LaunchPath::Full`], with the reason).
//!
//! The record lives in the sandbox folder, which no pool user can write, and the journal is the
//! kernel's: nothing the pool user controls can make a launch skip. Every launch removes the
//! record before the agent starts, whatever the check decides (`check_launch` under the
//! matter's set-aside lock, and the set-aside's launch pass again when it adds the session), so
//! a record never stands while an agent runs. `docs/decisions/0049-windows-sandbox-tier-b.md`
//! gives each condition and why each input can't be forged.
//!
//! The order PR6 follows: at launch, `check_launch`, then `set_aside::before_launch_checked`,
//! then `apply_checked`, all with the check; at session end, [`session_cursor`] before
//! `repair_after_session`, then `after_session`, then [`write_clean_close`] with both results.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::windows::admit::PathKind;
use crate::windows::writer::Reach;

#[cfg(any(windows, test))]
mod changes;
#[cfg(any(windows, test))]
mod check;
#[cfg(any(windows, test))]
mod close;
#[cfg(test)]
pub(crate) mod fake;
mod inputs;
mod journal;
#[cfg(windows)]
mod ntfs;
// Two attributes, not `all(windows, test)`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(test)]
#[cfg(windows)]
mod ntfs_timing;
#[cfg(any(windows, test))]
mod record;

#[cfg(any(windows, test))]
pub use check::check_launch;
#[cfg(any(windows, test))]
pub use close::{Closed, Cursor, NotWritten, session_cursor, write_clean_close};
pub(crate) use inputs::WalkInputs;
pub use journal::{
    ATTRIBUTE_DIRECTORY, ATTRIBUTE_REPARSE_POINT, Change, Flow, Found, Journal, JournalError,
    JournalState, Listed, RootFacts, TreeError, Usn,
};
#[cfg(windows)]
pub use ntfs::NtfsJournal;
#[cfg(test)]
pub(crate) use record::record_path as fake_record_path;
#[cfg(any(windows, test))]
pub(crate) use record::remove as remove_record;

/// The most changed items a targeted launch handles; see [`changes`]' docs for the choice.
pub const MOST_TOUCHED: usize = 2_000;

/// Which path a launch took, for logs and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchPath {
    /// Nothing in the own trees changed since a clean close: no own-tree walk, no scan.
    Skipped,
    /// This many folders are walked (and, in the matter, re-probed); nothing else.
    Targeted(usize),
    /// Today's full walk and scan, for this reason.
    Full(FullReason),
}

/// Why a launch walks in full. New reasons are appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullReason {
    /// No clean-close record: the last session didn't end clean, or this is the first.
    NoRecord,
    /// The record can't be read, or is another matter's.
    Unreadable,
    /// The record was written by another version.
    Version,
    /// A session of the matter is open (running, or ended without its after-session pass).
    SessionOpen,
    /// The plan's walk inputs (own roots, their grants, the strict sets) differ.
    PlanChanged,
    /// A ledger entry on, in or above an own root is `pending` or `removing`.
    LedgerBusy,
    /// A hand-back is owed for the matter.
    HandbackOwed,
    /// An own root couldn't be opened or read.
    RootUnreadable,
    /// An own root is another folder now (another file ID or volume).
    RootChanged,
    /// An own root is on a removable drive.
    Removable,
    /// An own root's volume isn't NTFS, or its file IDs don't fit 64 bits.
    NotNtfs,
    /// The volume has no active journal.
    NoJournal,
    /// The unprivileged journal read isn't supported here.
    Unsupported,
    /// The journal was deleted and made again (another journal ID).
    JournalChanged,
    /// The journal wrapped: records past the cursor are gone.
    Wrapped,
    /// The cursor is past the journal's next USN: the volume went back (a restore, a rollback).
    RolledBack,
    /// The journal read failed.
    ReadFailed,
    /// More changed items than a targeted launch handles.
    TooMany,
    /// An own root itself changed (its descriptor, its name).
    RootTouched,
    /// A changed folder's place couldn't be found.
    Unresolved,
    /// The check was made for another plan than the one applied.
    OtherPlan,
    /// The journal's record at the cursor isn't the one the record saw there (another USN,
    /// time or file), or there is none now: the volume went back and has written past the
    /// cursor since (a restored image).
    FirstRecordDiffers,
}

/// A folder a targeted launch walks: below which own root, its parts below it, and how far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The own root it is in.
    pub root: PathBuf,
    /// That root's kind.
    pub kind: PathKind,
    /// Its parts below the root, one name per part.
    pub parts: Vec<OsString>,
    /// [`Reach::Children`] for a folder whose names changed, [`Reach::Tree`] for one whose
    /// subtree is new or changed.
    pub reach: Reach,
}

impl Target {
    /// Its path: the root joined with each part.
    pub fn path(&self) -> PathBuf {
        self.parts
            .iter()
            .fold(self.root.clone(), |p, part| p.join(part))
    }
}

/// What the launch check decided, for `apply_checked` and the set-aside's launch pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCheck {
    path: LaunchPath,
    targets: Vec<Target>,
    inputs: Option<WalkInputs>,
}

impl LaunchCheck {
    /// The full walk, for `reason`.
    pub fn full(reason: FullReason) -> LaunchCheck {
        LaunchCheck {
            path: LaunchPath::Full(reason),
            targets: Vec::new(),
            inputs: None,
        }
    }

    #[cfg(any(windows, test))]
    pub(crate) fn fast(inputs: WalkInputs, targets: Vec<Target>) -> LaunchCheck {
        let path = if targets.is_empty() {
            LaunchPath::Skipped
        } else {
            LaunchPath::Targeted(targets.len())
        };
        LaunchCheck {
            path,
            targets,
            inputs: Some(inputs),
        }
    }

    /// Which path the launch takes.
    pub fn path(&self) -> LaunchPath {
        self.path
    }

    /// The folders a targeted launch walks; empty otherwise.
    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    /// The walk inputs the check was made for; `None` for the full walk.
    pub(crate) fn inputs(&self) -> Option<&WalkInputs> {
        self.inputs.as_ref()
    }

    /// For the set-aside: the matter-tree folders to re-probe (empty when skipped), or `None`
    /// for the full scan.
    #[cfg(any(windows, test))]
    pub(crate) fn touched_in_matter(&self) -> Option<Vec<(Vec<OsString>, bool)>> {
        match self.path {
            LaunchPath::Full(_) => None,
            LaunchPath::Skipped | LaunchPath::Targeted(_) => Some(
                self.targets
                    .iter()
                    .filter(|t| t.kind == PathKind::Matter)
                    .map(|t| (t.parts.clone(), t.reach == Reach::Tree))
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests;
