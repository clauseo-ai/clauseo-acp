//! `DomainEvent`: what the log stores, one event per entry, append-only.
//!
//! The JSON is a one-key object, the kind as the key and the payload as its value:
//! `{"text_appended": {...}}`. Every format treats it as that one-entry map, so JSON and
//! `MessagePack` (`rmp_serde::to_vec_named`) agree. Serialize and Deserialize are written by hand
//! from one table, so a kind cannot be left out of [`DomainEvent::KINDS`], `kind()`, the encoder
//! or the decoder. An unknown kind decodes to [`DomainEvent::Unknown`] with its payload kept as
//! [`RawPayload`] and writes back under its own kind; a known kind with a payload that does not
//! decode is an error. Events do not deny unknown fields, so older readers accept new optional
//! fields.
//!
//! Matter-level kinds (`matter_created`, `checkpoint_recorded`, `outside_edits`,
//! `version_restored`) are logged in a matter's stream; every other kind in a thread's.

mod de;
mod payload;

use serde::{Serialize, Serializer};

use crate::open_enum::serialize_one_key;
use crate::{
    CheckpointRecorded, LimitStop, MatterCreated, OutsideEdits, RawPayload, ToolCallStarted,
    VersionRestored,
};

pub use payload::{
    AgentSessionAttached, AgentSessionDetached, CommandsAvailable, CompactionChanged,
    ConfigOptionsReplaced, ContentAppended, ElicitationExpired, ElicitationRequested,
    ElicitationResolved, FilesChanged, ModeChanged, Notice, PermissionExpired, PermissionRequested,
    PermissionResolved, PlanReplaced, QueuedPromptEdited, SteerResolved, StepUndone, TextAppended,
    ThreadArchived, ThreadCreated, ThreadRenamed, ToolCallUpdated, TurnEnded, TurnFailed,
    TurnQueued, TurnStarted, UsageUpdated, UserMessage,
};

/// Writes `DomainEvent` and everything that must list every kind, from one table of
/// `Variant(PayloadType) = "kind"` rows.
macro_rules! domain_events {
    ($($(#[$doc:meta])* $variant:ident($ty:ty) = $kind:literal,)+) => {
        /// One event of a stream's log. Each known variant holds the payload struct of the same
        /// name; see the module docs for the wire form.
        #[derive(Debug, Clone, PartialEq)]
        pub enum DomainEvent {
            $($(#[$doc])* $variant($ty),)+
            /// A kind this version does not know, from a newer Clauseo. It still advances
            /// `seq`, and writes back under its own kind with its payload as read. This version
            /// never produces it.
            Unknown {
                /// The kind as read.
                kind: String,
                /// The payload as read.
                payload: RawPayload,
            },
        }

        impl DomainEvent {
            /// Every known kind, in declaration order.
            pub const KINDS: &'static [&'static str] = &[$($kind),+];

            /// The event's kind, the key of its JSON object, such as `"text_appended"`; for
            /// `Unknown`, the kind it was read with. `store` fills its `kind` column with it.
            pub fn kind(&self) -> &str {
                match self {
                    $(Self::$variant(_) => $kind,)+
                    Self::Unknown { kind, .. } => kind,
                }
            }

            /// True for the kinds logged in a matter's stream (`matter_created`,
            /// `checkpoint_recorded`, `outside_edits`, `version_restored`); every other kind,
            /// `Unknown` included, belongs to a thread's stream (QR5.1). The host checks it
            /// against the envelope's `StreamId` before committing.
            pub const fn is_matter_level(&self) -> bool {
                matches!(
                    self,
                    Self::MatterCreated(_)
                        | Self::CheckpointRecorded(_)
                        | Self::OutsideEdits(_)
                        | Self::VersionRestored(_)
                )
            }
        }

        $(
            impl From<$ty> for DomainEvent {
                fn from(payload: $ty) -> Self {
                    Self::$variant(payload)
                }
            }
        )+

        impl Serialize for DomainEvent {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match self {
                    $(Self::$variant(payload) => serialize_one_key(serializer, $kind, payload),)+
                    Self::Unknown { kind, payload } => {
                        if Kind::from_key(kind).is_some() {
                            return Err(serde::ser::Error::custom(format_args!(
                                "DomainEvent::Unknown under the known kind {kind:?}"
                            )));
                        }
                        serialize_one_key(serializer, kind, payload)
                    }
                }
            }
        }

        /// A known kind, matched from the borrowed key before its payload is read.
        #[derive(Debug, Clone, Copy)]
        enum Kind {
            $($variant,)+
        }

        impl Kind {
            /// The known kind named `key`.
            fn from_key(key: &str) -> Option<Self> {
                match key {
                    $($kind => Some(Self::$variant),)+
                    _ => None,
                }
            }

            /// Decodes this kind's payload straight into its type.
            fn decode<'de, D: serde::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<DomainEvent, D::Error> {
                match self {
                    $(Self::$variant => {
                        <$ty as serde::Deserialize>::deserialize(deserializer)
                            .map(DomainEvent::$variant)
                    })+
                }
            }
        }

        // specta: `DomainEvent`'s type is `shape::DomainEvent`'s, the known kinds as one-key
        // objects and no `Unknown`, so the TypeScript matches the JSON. Delegating (rather than
        // `specta(type = ...)` with an inlined stand-in) leaves one entry of that name, not two.
        #[cfg(feature = "specta")]
        impl specta::Type for DomainEvent {
            fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
                <shape::DomainEvent as specta::Type>::definition(types)
            }
        }

        #[cfg(feature = "specta")]
        mod shape {
            use super::*;

            /// One event of a stream's log. Each known variant holds the payload struct of the
            /// same name; see the module docs for the wire form.
            #[derive(serde::Serialize, specta::Type)]
            #[expect(dead_code, reason = "only its specta::Type impl is used")]
            pub(super) enum DomainEvent {
                $($(#[$doc])* #[serde(rename = $kind)] $variant($ty),)+
            }
        }
    };
}

domain_events! {
    /// A thread was created (thread stream; host).
    ThreadCreated(ThreadCreated) = "thread_created",
    /// The thread got a new title (host, acp).
    ThreadRenamed(ThreadRenamed) = "thread_renamed",
    /// The thread was archived (host).
    ThreadArchived(ThreadArchived) = "thread_archived",
    /// An agent session started or was loaded (acp).
    AgentSessionAttached(AgentSessionAttached) = "agent_session_attached",
    /// The agent session ended (acp, host).
    AgentSessionDetached(AgentSessionDetached) = "agent_session_detached",
    /// The lawyer's message (host; acp on a `session/load` replay).
    UserMessage(UserMessage) = "user_message",
    /// A turn waits (host).
    TurnQueued(TurnQueued) = "turn_queued",
    /// A queued prompt was edited (host).
    QueuedPromptEdited(QueuedPromptEdited) = "queued_prompt_edited",
    /// Where a steered message went (host).
    SteerResolved(SteerResolved) = "steer_resolved",
    /// A turn started (host).
    TurnStarted(TurnStarted) = "turn_started",
    /// Text appended to an answer or thought (acp).
    TextAppended(TextAppended) = "text_appended",
    /// A non-text block in an answer or thought (acp).
    ContentAppended(Box<ContentAppended>) = "content_appended",
    /// A tool call started (acp).
    ToolCallStarted(Box<ToolCallStarted>) = "tool_call_started",
    /// A tool call changed (acp; host marks in-flight calls cancelled).
    ToolCallUpdated(Box<ToolCallUpdated>) = "tool_call_updated",
    /// The agent asked for permission (acp).
    PermissionRequested(PermissionRequested) = "permission_requested",
    /// A permission request was answered (host).
    PermissionResolved(PermissionResolved) = "permission_resolved",
    /// A permission request expired (host).
    PermissionExpired(PermissionExpired) = "permission_expired",
    /// The agent asked for input (acp).
    ElicitationRequested(ElicitationRequested) = "elicitation_requested",
    /// An elicitation was answered (host).
    ElicitationResolved(ElicitationResolved) = "elicitation_resolved",
    /// An elicitation expired (host).
    ElicitationExpired(ElicitationExpired) = "elicitation_expired",
    /// The agent's plan, replaced whole (acp).
    PlanReplaced(PlanReplaced) = "plan_replaced",
    /// The session's mode changed (acp).
    ModeChanged(ModeChanged) = "mode_changed",
    /// The session's config options, replaced whole (acp).
    ConfigOptionsReplaced(ConfigOptionsReplaced) = "config_options_replaced",
    /// The agent's slash commands, replaced whole (acp).
    CommandsAvailable(CommandsAvailable) = "commands_available",
    /// Context use and cost (acp).
    UsageUpdated(UsageUpdated) = "usage_updated",
    /// A notice for the lawyer (acp, host).
    Notice(Notice) = "notice",
    /// Context compaction started or finished (acp).
    CompactionChanged(CompactionChanged) = "compaction_changed",
    /// The files a turn changed (matters, through host).
    FilesChanged(FilesChanged) = "files_changed",
    /// Why a turn failed (host, acp).
    TurnFailed(TurnFailed) = "turn_failed",
    /// A usage limit stopped a turn (limits, through host).
    TurnStoppedAtLimit(LimitStop) = "turn_stopped_at_limit",
    /// A turn ended, exactly once per turn (host).
    TurnEnded(TurnEnded) = "turn_ended",
    /// A step was undone (host, matters).
    StepUndone(StepUndone) = "step_undone",
    /// A matter was created (matter stream; host).
    MatterCreated(MatterCreated) = "matter_created",
    /// A checkpoint was recorded (matter stream; matters, through host).
    CheckpointRecorded(CheckpointRecorded) = "checkpoint_recorded",
    /// Files changed outside Clauseo (matter stream; matters, through host).
    OutsideEdits(OutsideEdits) = "outside_edits",
    /// An earlier version was restored (matter stream; host, matters).
    VersionRestored(VersionRestored) = "version_restored",
}

// The boxed variants also convert from the bare payload, so a caller never boxes by hand.
macro_rules! from_unboxed {
    ($($variant:ident),+) => {
        $(
            impl From<$variant> for DomainEvent {
                fn from(payload: $variant) -> Self {
                    Self::$variant(Box::new(payload))
                }
            }
        )+
    };
}

from_unboxed!(ContentAppended, ToolCallStarted, ToolCallUpdated);
