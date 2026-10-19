//! Turns: how a prompt is delivered, where a steered message went, how a turn stops, and which
//! files it changed.

use serde::de::MapAccess;
use serde::{Deserialize, Deserializer, Serialize};

use crate::open_enum::{
    OpenDataEnum, deserialize_open, enum_with_all, open_unit_enum, serialize_kind,
};
use crate::{ContentId, MatterPath, TurnId};

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
}

enum_with_all! {
    /// Where a steered message went. `turn` is always the turn that answers the message.
    ///
    /// A one-key object; a kind from a newer Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "specta", derive(specta::Type))]
    pub enum SteerOutcome {
        /// The agent took it into the running turn.
        Injected {
            /// The running turn.
            turn: TurnId = TurnId::NIL,
        },
        /// That turn had just ended; the message moved to a new turn, which starts next.
        Requeued {
            /// The new turn.
            turn: TurnId = TurnId::NIL,
        },
        /// No real steer: the running turn was cancelled and a new turn starts next.
        SentNow {
            /// The new turn.
            turn: TurnId = TurnId::NIL,
        },
        /// An outcome from a newer Clauseo; written back as the plain string `kind`.
        #[serde(untagged, serialize_with = "serialize_kind::<SteerOutcome, _>")]
        #[cfg_attr(feature = "specta", specta(type = String))]
        Unknown {
            /// The outcome's key as read.
            kind: String = String::new(),
        },
    }
}

#[derive(Deserialize)]
struct TurnData {
    turn: TurnId,
}

impl OpenDataEnum for SteerOutcome {
    const EXPECTING: &'static str = "a steer outcome";
    const DATA_KINDS: &'static [&'static str] = &["injected", "requeued", "sent_now"];

    fn unit(_kind: &str) -> Option<Self> {
        None
    }

    fn data<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Option<Self>, A::Error> {
        let make: fn(TurnId) -> Self = match kind {
            "injected" => |turn| Self::Injected { turn },
            "requeued" => |turn| Self::Requeued { turn },
            "sent_now" => |turn| Self::SentNow { turn },
            _ => return Ok(None),
        };
        let TurnData { turn } = map.next_value()?;
        Ok(Some(make(turn)))
    }

    fn unknown(kind: &str) -> Self {
        Self::Unknown {
            kind: kind.to_owned(),
        }
    }
}

impl<'de> Deserialize<'de> for SteerOutcome {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_open(deserializer)
    }
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
}

/// One file a turn (or an outside edit) changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FileChange {
    /// The file, relative to the matter folder (its new path for a rename).
    pub path: MatterPath,
    /// What happened to it.
    pub change: ChangeKind,
    /// Its content before; `None` for an added file.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub before: Option<ContentId>,
    /// Its content after; `None` for a deleted file.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub after: Option<ContentId>,
    /// Its size in bytes after the change (before it, for a deleted file), at most
    /// [`MAX_SAFE`](crate::MAX_SAFE).
    #[cfg_attr(feature = "specta", specta(type = u32))]
    #[serde(with = "crate::number::safe")]
    pub size_bytes: u64,
    /// The watcher saw it change during the turn with no matching agent tool call, so it may
    /// hold the lawyer's own edits. Undo shows it unticked.
    pub may_include_lawyer_edits: bool,
}

/// What happened to a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ChangeKind {
    /// Created.
    Added,
    /// Edited.
    Modified,
    /// Deleted.
    Deleted,
    /// Moved or renamed; `path` is the new path.
    Renamed {
        /// The old path.
        from: MatterPath,
    },
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
}
