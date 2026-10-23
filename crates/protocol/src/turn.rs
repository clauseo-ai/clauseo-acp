//! Turns: how a prompt is delivered, where a steered message went, how a turn stops, and which
//! files it changed.

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};
use crate::{ContentId, Count, Error, MatterPath, TurnId};

open_unit_enum! {
    /// How the lawyer asked a prompt to be delivered while a turn runs in its thread.
    pub enum Delivery {
        /// Wait in the thread's queue (also when the field is missing).
        Queue,
        /// Cancel the running turn, then send this prompt next. Works on every agent.
        SendNow,
        /// Add it to the running turn where the agent can, else send it now.
        Steer,
    }
    test mod delivery;
}

impl Default for Delivery {
    /// [`Delivery::Queue`], what a missing field means.
    fn default() -> Self {
        Self::Queue
    }
}

impl Delivery {
    /// Whether `delivery` is queue, which every `delivery` field writes as a missing field, so
    /// queue has one wire form. For `skip_serializing_if`, which passes a reference; generic so
    /// the closed `DeliveryRequest` of a command uses the same test.
    pub(crate) fn is_queue<T: Copy + Into<Self>>(delivery: &T) -> bool {
        (*delivery).into() == Self::Queue
    }
}

open_data_enum! {
    /// Where a steered message went. `turn` is always the turn that answers the message.
    ///
    /// A one-key object; a kind from a newer Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum SteerOutcome {
        /// The agent took it into the running turn.
        Injected = "injected" {
            /// The running turn.
            turn: TurnId,
        },
        /// That turn had just ended; the message moved to a new turn, which starts next.
        Requeued = "requeued" {
            /// The new turn.
            turn: TurnId,
        },
        /// No real steer: the running turn was cancelled and a new turn starts next.
        SentNow = "sent_now" {
            /// The new turn.
            turn: TurnId,
        },
        _ =>
        /// An outcome from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod steer_outcome with [
        SteerOutcome::Injected {
            turn: TurnId::new(uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0007)).unwrap(),
        },
        SteerOutcome::Requeued {
            turn: TurnId::new(uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0007)).unwrap(),
        },
        SteerOutcome::SentNow {
            turn: TurnId::new(uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0007)).unwrap(),
        },
    ];
}

open_unit_enum! {
    /// Why a turn stopped: ACP v1's stop reasons plus Clauseo's `AtLimit` and `Failed`.
    pub enum StopReason {
        /// The agent finished. Only this ending moves the thread's queue on.
        EndTurn,
        /// The model hit its token limit.
        MaxTokens,
        /// The agent hit its limit of model requests in one turn.
        MaxTurnRequests,
        /// The model refused.
        Refusal,
        /// The lawyer or the host cancelled it.
        Cancelled,
        /// The account's usage limit stopped it; a `TurnStoppedAtLimit` explains it.
        AtLimit,
        /// It failed; a `TurnFailed` before it says why.
        Failed,
    }
    test mod stop_reason;
}

/// One file a turn (or an outside edit) changed.
///
/// Built through [`FileChange::new`], which checks `before` and `after` against `change`;
/// decoding accepts any combination, so an older or foreign entry still reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FileChange {
    /// The file, relative to the matter folder (its new path for a rename).
    path: MatterPath,
    /// What happened to it.
    change: ChangeKind,
    /// Its content before; `None` for an added file.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    before: Option<ContentId>,
    /// Its content after; `None` for a deleted file.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    after: Option<ContentId>,
    /// Its size in bytes after the change (before it, for a deleted file), at most
    /// [`MAX_SAFE`](crate::MAX_SAFE).
    size_bytes: Count,
    /// The watcher saw it change during the turn with no matching agent tool call, so it may
    /// hold the lawyer's own edits. Undo shows it unticked.
    may_include_lawyer_edits: bool,
}

impl FileChange {
    /// The change `change` to `path`, with `may_include_lawyer_edits` off (see
    /// [`FileChange::with_lawyer_edits`]). Fails with [`Error::Format`] unless an added file
    /// has only `after`, a deleted file only `before`, and a modified or renamed file both.
    pub fn new(
        path: MatterPath,
        change: ChangeKind,
        before: Option<ContentId>,
        after: Option<ContentId>,
        size_bytes: Count,
    ) -> Result<Self, Error> {
        let refused = match change {
            ChangeKind::Added => (before.is_some() || after.is_none())
                .then_some("an added file has content after and none before"),
            ChangeKind::Deleted => (before.is_none() || after.is_some())
                .then_some("a deleted file has content before and none after"),
            ChangeKind::Modified | ChangeKind::Renamed { .. } => (before.is_none()
                || after.is_none())
            .then_some("a modified or renamed file has content before and after"),
            ChangeKind::Unknown { .. } => None,
        };
        if let Some(rule) = refused {
            return Err(Error::Format {
                kind: "FileChange",
                rule,
            });
        }
        Ok(Self {
            path,
            change,
            before,
            after,
            size_bytes,
            may_include_lawyer_edits: false,
        })
    }

    /// The same change, marked as one the watcher saw with no matching agent tool call.
    #[must_use]
    pub const fn with_lawyer_edits(mut self) -> Self {
        self.may_include_lawyer_edits = true;
        self
    }

    /// The file, relative to the matter folder (its new path for a rename).
    pub const fn path(&self) -> &MatterPath {
        &self.path
    }

    /// What happened to it.
    pub const fn change(&self) -> &ChangeKind {
        &self.change
    }

    /// Its content before; `None` for an added file.
    pub const fn before(&self) -> Option<&ContentId> {
        self.before.as_ref()
    }

    /// Its content after; `None` for a deleted file.
    pub const fn after(&self) -> Option<&ContentId> {
        self.after.as_ref()
    }

    /// Its size in bytes after the change (before it, for a deleted file).
    pub const fn size_bytes(&self) -> Count {
        self.size_bytes
    }

    /// Whether it may hold the lawyer's own edits; undo shows it unticked.
    pub const fn may_include_lawyer_edits(&self) -> bool {
        self.may_include_lawyer_edits
    }
}

open_data_enum! {
    /// What happened to a file.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ChangeKind {
        /// Created.
        Added = "added",
        /// Edited.
        Modified = "modified",
        /// Deleted.
        Deleted = "deleted",
        /// Moved or renamed; `path` is the new path.
        Renamed = "renamed" {
            /// The old path.
            from: MatterPath,
        },
        _ =>
        /// A change from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod change_kind with [
        ChangeKind::Added,
        ChangeKind::Modified,
        ChangeKind::Deleted,
        ChangeKind::Renamed {
            from: MatterPath::new("old/a.docx").unwrap(),
        },
    ];
}

/// A file a checkpoint did not version. Office lock and temp files (`~$*`, `*.tmp`) are never
/// versioned and never listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SkippedFile {
    /// The file, relative to the matter folder.
    pub path: MatterPath,
    /// Why it was skipped.
    pub reason: SkippedFileReason,
}

open_unit_enum! {
    /// Why a file was not versioned.
    pub enum SkippedFileReason {
        /// Larger than the size cap.
        OverSizeCap,
        /// A cloud placeholder whose content is not on this computer.
        CloudOnly,
    }
    test mod skipped_file_reason;
}
