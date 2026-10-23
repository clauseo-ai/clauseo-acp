//! Payloads about a turn, from the lawyer's message to its end, logged in the thread's stream.

use serde::{Deserialize, Deserializer, Serialize};

use crate::serde_util::BorrowedStr;
use crate::{
    CheckpointId, CompactString, ContentBlock, Delivery, Error, ErrorCode, FileChange,
    LimitStopReason, MatterPath, MessageId, SkippedFile, SteerOutcome, StopReason, TextKind,
    ThreadId, Timestamp, TurnId, TurnUsage,
};

/// The lawyer's message, logged before its turn is queued or started. `session/load` replays
/// of `user_message_chunk` map to it too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UserMessage {
    /// The turn that answers it.
    pub turn: TurnId,
    /// The message.
    pub message: MessageId,
    /// ACP's `messageId`, when the agent gave one; a foreign attribute.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub acp_message_id: Option<CompactString>,
    /// What it says.
    pub content: Vec<ContentBlock>,
    /// How the lawyer asked it to be delivered. Queue is written as a missing field, and a
    /// missing field reads as queue.
    #[serde(default, skip_serializing_if = "Delivery::is_queue")]
    pub delivery: Delivery,
}

/// A turn waits: behind its own thread's running turn, or behind another thread's writer.
///
/// Built through [`TurnQueued::new`], which checks `behind` against `behind_turn`; decoding
/// accepts any combination, so an older or foreign entry still reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnQueued {
    /// The waiting turn.
    turn: TurnId,
    /// The thread it waits for: this thread when `behind_turn` is set, or when start-up
    /// reconciliation holds a first turn.
    behind: ThreadId,
    /// The same thread's turn it waits for.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    behind_turn: Option<TurnId>,
}

impl TurnQueued {
    /// `turn`, queued in `thread` (the stream's thread, which the payload does not store)
    /// behind the thread `behind` or, when `behind_turn` is set, behind that turn of `thread`.
    /// Fails with [`Error::Format`] when `behind_turn` is set and `behind` is another thread,
    /// or when `behind_turn` is `turn` itself.
    pub fn new(
        thread: ThreadId,
        turn: TurnId,
        behind: ThreadId,
        behind_turn: Option<TurnId>,
    ) -> Result<Self, Error> {
        let refused = match behind_turn {
            Some(_) if behind != thread => {
                Some("a turn queued behind a turn waits in its own thread")
            }
            Some(other) if other == turn => Some("a turn never waits for itself"),
            Some(_) | None => None,
        };
        if let Some(rule) = refused {
            return Err(Error::Format {
                kind: "TurnQueued",
                rule,
            });
        }
        Ok(Self {
            turn,
            behind,
            behind_turn,
        })
    }

    /// The waiting turn.
    pub const fn turn(&self) -> TurnId {
        self.turn
    }

    /// The thread it waits for.
    pub const fn behind(&self) -> ThreadId {
        self.behind
    }

    /// The same thread's turn it waits for.
    pub const fn behind_turn(&self) -> Option<TurnId> {
        self.behind_turn
    }
}

/// A queued prompt's content was replaced; the turn keeps its id and its place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct QueuedPromptEdited {
    /// The queued turn.
    pub turn: TurnId,
    /// Its message.
    pub message: MessageId,
    /// The new content.
    pub content: Vec<ContentBlock>,
}

/// Where a steered message went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SteerResolved {
    /// The steered message.
    pub message: MessageId,
    /// Where it went.
    pub outcome: SteerOutcome,
}

/// A turn started, after its before-turn checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnStarted {
    /// The turn.
    pub turn: TurnId,
    /// The checkpoint taken before it; a turn never runs without one.
    pub checkpoint_before: CheckpointId,
}

/// Text appended to an agent's answer or thought.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TextAppended {
    /// The turn.
    pub turn: TurnId,
    /// The agent's message.
    pub message: MessageId,
    /// ACP's `messageId`, when the agent gave one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub acp_message_id: Option<CompactString>,
    /// Answer or thought.
    pub kind: TextKind,
    /// The text.
    pub text: String,
}

/// A non-text block in an agent's answer or thought (never `ContentBlock::Text`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ContentAppended {
    /// The turn.
    pub turn: TurnId,
    /// The agent's message.
    pub message: MessageId,
    /// ACP's `messageId`, when the agent gave one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub acp_message_id: Option<CompactString>,
    /// Answer or thought.
    pub kind: TextKind,
    /// The block.
    pub block: ContentBlock,
}

/// The files a turn changed. When present it comes before the turn's `TurnEnded`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FilesChanged {
    /// The turn.
    pub turn: TurnId,
    /// The files it changed.
    pub files: Vec<FileChange>,
    /// The files it changed that were not versioned.
    pub skipped: Vec<SkippedFile>,
}

/// Why a turn failed. Detail only: the turn's `TurnEnded` still follows.
///
/// A code this version does not know decodes to `ErrorCode::Unknown` and moves to the front of
/// `detail`, as `"<code>: <detail>"` or `"<code>"` alone, the way `HostError` keeps it in
/// `message`, so a re-sent copy still names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnFailed {
    /// The turn.
    pub turn: TurnId,
    /// The failure's code; the UI picks copy by it.
    pub code: ErrorCode,
    /// More detail for logs, redacted, never shown: at most 4 KiB as written; a decoded copy
    /// may carry an unknown code in front, so it can be longer.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
}

/// `TurnFailed` as read, before its code is looked up.
#[derive(Deserialize)]
struct TurnFailedWire<'a> {
    turn: TurnId,
    #[serde(borrow)]
    code: BorrowedStr<'a>,
    #[serde(default)]
    detail: Option<String>,
}

impl<'de> Deserialize<'de> for TurnFailed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = TurnFailedWire::deserialize(deserializer)?;
        let raw = wire.code.as_str();
        let (code, detail) = if let Ok(code) = raw.parse::<ErrorCode>() {
            (code, wire.detail)
        } else {
            let detail = match wire.detail {
                Some(detail) => format!("{raw}: {detail}"),
                None => raw.to_owned(),
            };
            (ErrorCode::Unknown, Some(detail))
        };
        Ok(Self {
            turn: wire.turn,
            code,
            detail,
        })
    }
}

/// A turn the account's usage limit stopped. It may come after the turn's `TurnEnded`, at most
/// once per turn. Its thread is the envelope's `stream`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnStoppedAtLimit {
    /// The turn.
    pub turn: TurnId,
    /// The id of the window it hit, when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub window: Option<String>,
    /// When the agent can continue, when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resets_at: Option<Timestamp>,
    /// Which limit it hit.
    pub reason: LimitStopReason,
}

/// A turn ended: exactly one per turn, including errors, cancels and a queued turn cancelled
/// before it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnEnded {
    /// The turn.
    pub turn: TurnId,
    /// Why it stopped.
    pub stop: StopReason,
    /// Its token counts, when the agent reported them.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage: Option<TurnUsage>,
    /// The checkpoint taken after it; `None` when that failed, which is shown, never silent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub checkpoint_after: Option<CheckpointId>,
}

/// A step (a turn's changes) was undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct StepUndone {
    /// The turn whose changes were undone.
    pub turn: TurnId,
    /// The checkpoint the undo wrote.
    pub checkpoint: CheckpointId,
    /// The files put back.
    pub restored: Vec<MatterPath>,
    /// The files the lawyer chose to keep.
    pub kept: Vec<MatterPath>,
}
