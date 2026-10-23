//! `DomainEvent`: what the log stores, one event per entry, append-only.
//!
//! The JSON is a one-key object, the kind as the key and the payload as its value:
//! `{"text_appended": {...}}`. Every format treats it as that one-entry map, so JSON and
//! `MessagePack` (`rmp_serde::to_vec_named`) agree. Serialize and Deserialize are written by hand
//! from one table, so a kind cannot be left out of [`DomainEvent::KINDS`], `kind()`, the encoder
//! or the decoder. An unknown kind decodes to [`DomainEvent::Unknown`] with its payload kept as
//! [`RawPayload`] and writes back under its own kind; a known kind with a payload that does not
//! decode is an error. Events do not deny unknown fields, so older readers accept new optional
//! fields. Each known kind's payload struct is in `event/payload/`.
//!
//! Each row of the table names its stream, `matter` or `thread`, and
//! [`DomainEvent::is_matter_level`] answers from it; 0002's Kinds table is the record. An unknown
//! kind's stream is not known here; its envelope's `stream` says it. A test checks the rows
//! against 0002's Kind and Stream columns.

mod de;
mod payload;

use serde::{Serialize, Serializer};

use crate::RawPayload;
use crate::open_enum::{UnknownKind, serialize_one_key};

pub use payload::{
    AgentSessionAttached, AgentSessionDetached, CheckpointRecorded, CommandsAvailable,
    CompactionChanged, ConfigOptionsReplaced, ContentAppended, ElicitationExpired,
    ElicitationRequested, ElicitationResolved, FilesChanged, MatterCreated, ModeChanged, Notice,
    OutsideEdits, PermissionExpired, PermissionRequested, PermissionResolved, PlanReplaced,
    QueuedPromptEdited, SteerResolved, StepUndone, TextAppended, ThreadArchived, ThreadCreated,
    ThreadRenamed, ToolCallStarted, ToolCallUpdated, TurnEnded, TurnFailed, TurnQueued,
    TurnStarted, TurnStoppedAtLimit, UsageUpdated, UserMessage, VersionRestored,
};

/// `true` for the `matter` stream, `false` for `thread`; any other word does not compile.
macro_rules! is_matter_stream {
    (matter) => {
        true
    };
    (thread) => {
        false
    };
}

/// Writes `DomainEvent` and everything that must list every kind, from one table of
/// `Variant(PayloadType) = "kind", stream;` rows.
macro_rules! domain_events {
    ($($(#[doc = $doc:expr])* $variant:ident($ty:ty) = $kind:literal, $stream:ident;)+) => {
        /// One event of a stream's log. Each known variant holds the payload struct of the same
        /// name; see the module docs for the wire form.
        #[derive(Debug, Clone, PartialEq)]
        pub enum DomainEvent {
            $($(#[doc = $doc])* $variant($ty),)+
            /// A kind this version does not know, from a newer Clauseo. It still advances
            /// `seq`, and writes back under its own kind with its payload as read. Its `kind` is
            /// an [`UnknownKind`], which has no public constructor, so no crate builds one by
            /// accident; any crate still gets one by decoding a kind this version does not know,
            /// or by cloning a decoded one.
            Unknown {
                /// The kind as read.
                kind: UnknownKind<DomainEvent>,
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

            /// `Some(true)` for the kinds logged in a matter's stream, the rows of the kind
            /// table marked `matter`; `Some(false)` for every other known kind, which belongs to
            /// a thread's stream. `None` for `Unknown`: this version does not know the kind's
            /// stream, so a reader trusts the envelope's `stream`. The host checks a known kind
            /// against the envelope's `StreamId` before committing.
            pub const fn is_matter_level(&self) -> Option<bool> {
                match self {
                    $(Self::$variant(_) => Some(is_matter_stream!($stream)),)+
                    Self::Unknown { .. } => None,
                }
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
                        serialize_one_key(serializer, kind, &crate::raw::AsDecoded(payload))
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
        // objects and an unknown arm, so the TypeScript matches the JSON and says the set is
        // open. Delegating (rather than `specta(type = ...)` with an inlined stand-in) leaves one
        // entry of that name, not two. Registering it registers `KnownDomainEvent` too, the
        // same union without the unknown arm, for the webview to narrow to (see its doc).
        #[cfg(feature = "specta")]
        impl specta::Type for DomainEvent {
            fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
                <shape::KnownDomainEvent as specta::Type>::definition(types);
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
                $($(#[doc = $doc])* #[serde(rename = $kind)] $variant($ty),)+
                /// A kind from a newer Clauseo, which this version does not know: one key, the
                /// kind, holding its payload as read. After a downgrade the host can still send
                /// one in a `HostEvent::Events` batch. The webview shows nothing for it and
                /// reads on, as docs/ui-states.md asks of every `Unknown` (a neutral fallback),
                /// so a `switch` over the kinds needs a default branch.
                #[serde(untagged)]
                Unknown(std::collections::BTreeMap<String, RawPayload>),
            }

            /// `DomainEvent` without its unknown arm: only the kinds this version knows. With
            /// the unknown arm in the union, `e.turn_started` stays `TurnStarted | RawPayload`
            /// whatever the webview checks, so it narrows to this type first: it checks that the
            /// event's one key is in `KNOWN_DOMAIN_EVENT_KINDS` (written from `DomainEvent::KINDS`,
            /// the keys of this union), then treats the event as `KnownDomainEvent`. There
            /// `e.turn_started !== undefined` narrows the payload to `TurnStarted` (`in` alone
            /// leaves `| undefined`, since each arm marks the other kinds `?: never`). An event
            /// whose key is not known is the unknown arm.
            #[derive(serde::Serialize, specta::Type)]
            #[expect(dead_code, reason = "only its specta::Type impl is used")]
            pub(super) enum KnownDomainEvent {
                $($(#[doc = $doc])* #[serde(rename = $kind)] $variant($ty),)+
            }
        }
    };
}

domain_events! {
    /// A thread was created (thread stream; host).
    ThreadCreated(ThreadCreated) = "thread_created", thread;
    /// The thread got a new title (host, acp).
    ThreadRenamed(ThreadRenamed) = "thread_renamed", thread;
    /// The thread was archived (host).
    ThreadArchived(ThreadArchived) = "thread_archived", thread;
    /// An agent session started or was loaded (acp).
    AgentSessionAttached(AgentSessionAttached) = "agent_session_attached", thread;
    /// The agent session ended (acp, host).
    AgentSessionDetached(AgentSessionDetached) = "agent_session_detached", thread;
    /// The lawyer's message (host; acp on a `session/load` replay).
    UserMessage(UserMessage) = "user_message", thread;
    /// A turn waits (host).
    TurnQueued(TurnQueued) = "turn_queued", thread;
    /// A queued prompt was edited (host).
    QueuedPromptEdited(QueuedPromptEdited) = "queued_prompt_edited", thread;
    /// Where a steered message went (host).
    SteerResolved(SteerResolved) = "steer_resolved", thread;
    /// A turn started (host).
    TurnStarted(TurnStarted) = "turn_started", thread;
    /// Text appended to an answer or thought (acp).
    TextAppended(TextAppended) = "text_appended", thread;
    /// A non-text block in an answer or thought (acp).
    ContentAppended(Box<ContentAppended>) = "content_appended", thread;
    /// A tool call started (acp).
    ToolCallStarted(Box<ToolCallStarted>) = "tool_call_started", thread;
    /// A tool call changed (acp; host marks in-flight calls cancelled).
    ToolCallUpdated(Box<ToolCallUpdated>) = "tool_call_updated", thread;
    /// The agent asked for permission (acp).
    PermissionRequested(PermissionRequested) = "permission_requested", thread;
    /// A permission request was answered (host).
    PermissionResolved(PermissionResolved) = "permission_resolved", thread;
    /// A permission request expired (host).
    PermissionExpired(PermissionExpired) = "permission_expired", thread;
    /// The agent asked for input (acp).
    ElicitationRequested(ElicitationRequested) = "elicitation_requested", thread;
    /// An elicitation was answered (host).
    ElicitationResolved(ElicitationResolved) = "elicitation_resolved", thread;
    /// An elicitation expired (host).
    ElicitationExpired(ElicitationExpired) = "elicitation_expired", thread;
    /// The agent's plan, replaced whole (acp).
    PlanReplaced(PlanReplaced) = "plan_replaced", thread;
    /// The session's mode changed (acp).
    ModeChanged(ModeChanged) = "mode_changed", thread;
    /// The session's config options, replaced whole (acp).
    ConfigOptionsReplaced(ConfigOptionsReplaced) = "config_options_replaced", thread;
    /// The agent's slash commands, replaced whole (acp).
    CommandsAvailable(CommandsAvailable) = "commands_available", thread;
    /// Context use and cost (acp).
    UsageUpdated(UsageUpdated) = "usage_updated", thread;
    /// A notice for the lawyer (acp, host).
    Notice(Notice) = "notice", thread;
    /// Context compaction started or finished (acp).
    CompactionChanged(CompactionChanged) = "compaction_changed", thread;
    /// The files a turn changed (matters, through host).
    FilesChanged(FilesChanged) = "files_changed", thread;
    /// Why a turn failed (host, acp).
    TurnFailed(TurnFailed) = "turn_failed", thread;
    /// A usage limit stopped a turn (limits, through host).
    TurnStoppedAtLimit(TurnStoppedAtLimit) = "turn_stopped_at_limit", thread;
    /// A turn ended, exactly once per turn (host).
    TurnEnded(TurnEnded) = "turn_ended", thread;
    /// A step was undone (host, matters).
    StepUndone(StepUndone) = "step_undone", thread;
    /// A matter was created (matter stream; host).
    MatterCreated(MatterCreated) = "matter_created", matter;
    /// A checkpoint was recorded (matter stream; matters, through host).
    CheckpointRecorded(CheckpointRecorded) = "checkpoint_recorded", matter;
    /// Files changed outside Clauseo (matter stream; matters, through host).
    OutsideEdits(OutsideEdits) = "outside_edits", matter;
    /// An earlier version was restored (matter stream; host, matters).
    VersionRestored(VersionRestored) = "version_restored", matter;
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
