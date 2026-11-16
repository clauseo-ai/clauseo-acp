//! How the run before this one ended, for the room's notice after Clauseo closed unexpectedly:
//! a [`LastExit`] answers `HostQuery::LastExit`. The desktop knows whether the last run quit
//! through its own quit; the host knows which file actions had started and not finished then
//! (each is a row in the store from before it touches a file until its writer passes on).

use serde::{Deserialize, Serialize};

use crate::{Count, MatterId, ThreadId, Timestamp};

/// What `LastExit` answers, the same all through one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LastExit {
    /// The run before this one ended without its quit: a crash, a forced quit or a power cut.
    /// The desktop's flag alone: `interrupted` can hold actions after a clean quit too (an idle
    /// Quit or a logoff while one ran), and then this is false.
    pub unclean: bool,
    /// When this run started: a window keeps the lawyer's dismissal per value, so the notice
    /// shows once per unclean exit.
    pub run_started: Timestamp,
    /// The file actions that had started and not finished, one per matter at most.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interrupted: Vec<InterruptedAction>,
}

/// A file action the last run started and did not finish: some of its files may hold the old
/// version and some the new. Versions keeps the copies from before it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct InterruptedAction {
    /// What it was.
    pub kind: InterruptedKind,
    /// The matter whose files it changed.
    pub matter: MatterId,
    /// The conversation it was asked from, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadId>,
    /// How many files it set out to change; missing when that was not known before it began
    /// (a restore of a whole version).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Count>,
}

/// The kinds of file action that are recorded before they start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum InterruptedKind {
    /// `UndoStep`.
    Undo,
    /// `RestoreVersion`.
    Restore,
    /// `DecideRevisions` accepting tracked changes.
    AcceptChanges,
    /// `DecideRevisions` rejecting tracked changes.
    RejectChanges,
}

impl InterruptedKind {
    /// Every value, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::Undo,
        Self::Restore,
        Self::AcceptChanges,
        Self::RejectChanges,
    ];

    /// Its JSON name, as the store keeps it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::Restore => "restore",
            Self::AcceptChanges => "accept_changes",
            Self::RejectChanges => "reject_changes",
        }
    }

    /// The value named `name`, as [`InterruptedKind::name`] writes it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}
