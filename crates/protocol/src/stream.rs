//! `StreamId` and `EntryId`: enums with data that travel as one plain string, `<kind>:<uuid>`.
//!
//! The same text serves the wire, SQL and the command line. `Serialize` writes it with
//! `collect_str`, straight into the output; `Deserialize` parses the borrowed `&str`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ids::FromStrVisitor;
use crate::{
    CheckpointId, ElicitationId, Error, EventId, MatterId, MessageId, PermissionRequestId,
    ThreadId, ToolCallId, TurnId,
};

/// Splits `<prefix>:<rest>`, or names the missing separator as a format error.
fn split_kind<'a>(text: &'a str, kind: &'static str) -> Result<(&'a str, &'a str), Error> {
    text.split_once(':').ok_or(Error::Format {
        kind,
        rule: "expected <kind>:<uuid>",
    })
}

fn unknown_prefix(kind: &'static str, prefix: &str) -> Error {
    Error::UnknownPrefix {
        kind,
        prefix: prefix.to_owned(),
    }
}

/// The log an event belongs to. Each stream has its own gapless `seq` and its own view cursor.
///
/// Text form: `thread:<uuid>` or `matter:<uuid>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
pub enum StreamId {
    /// A thread's stream: every event except the matter-level ones.
    Thread(ThreadId),
    /// A matter's stream: `MatterCreated`, `CheckpointRecorded`, `OutsideEdits`,
    /// `VersionRestored`.
    Matter(MatterId),
}

impl StreamId {
    /// The text before the `:`: `"thread"` or `"matter"`.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Thread(_) => "thread",
            Self::Matter(_) => "matter",
        }
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Thread(id) => write!(f, "thread:{id}"),
            Self::Matter(id) => write!(f, "matter:{id}"),
        }
    }
}

impl FromStr for StreamId {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        let (prefix, id) = split_kind(text, "StreamId")?;
        match prefix {
            "thread" => id.parse().map(Self::Thread),
            "matter" => id.parse().map(Self::Matter),
            other => Err(unknown_prefix("StreamId", other)),
        }
    }
}

impl Serialize for StreamId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for StreamId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::<Self>::new("a stream id, kind:uuid"))
    }
}

/// Defines `EntryId` from one table, so `Display`, `FromStr` and `KINDS` cannot disagree.
macro_rules! entry_ids {
    ($($(#[$meta:meta])* $variant:ident($id:ident) = $text:literal,)+) => {
        /// The key of a view entry: the entry's kind and the domain id it is built from.
        ///
        /// Text form: `<snake_case kind>:<uuid>`, for example `tool_call:<uuid>`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
        pub enum EntryId {
            $($(#[$meta])* $variant($id),)+
        }

        impl EntryId {
            /// Every kind's text, in declaration order.
            pub const KINDS: &'static [&'static str] = &[$($text),+];

            /// The text before the `:`, such as `"tool_call"`.
            pub const fn kind(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => $text,)+
                }
            }

            /// The domain id's UUID.
            pub const fn uuid(&self) -> uuid::Uuid {
                match self {
                    $(Self::$variant(id) => id.as_uuid(),)+
                }
            }
        }

        impl FromStr for EntryId {
            type Err = Error;

            fn from_str(text: &str) -> Result<Self, Error> {
                let (prefix, id) = split_kind(text, "EntryId")?;
                match prefix {
                    $($text => id.parse().map(Self::$variant),)+
                    other => Err(unknown_prefix("EntryId", other)),
                }
            }
        }
    };
}

entry_ids! {
    /// A thread's header, built from the thread.
    Header(ThreadId) = "header",
    /// The lawyer's message.
    UserMessage(MessageId) = "user_message",
    /// The agent's answer or thought text.
    AssistantMessage(MessageId) = "assistant_message",
    /// One non-text block of the agent's, built from its `ContentAppended` event.
    AgentContent(EventId) = "agent_content",
    /// A tool call.
    ToolCall(ToolCallId) = "tool_call",
    /// A permission request.
    Permission(PermissionRequestId) = "permission",
    /// An elicitation.
    Elicitation(ElicitationId) = "elicitation",
    /// One ended turn.
    Step(TurnId) = "step",
    /// A turn stopped at the usage limit.
    LimitStop(TurnId) = "limit_stop",
    /// A notice, built from its event.
    Notice(EventId) = "notice",
    /// An undo, restore or compaction marker, built from its event.
    Marker(EventId) = "marker",
    /// A matter's header.
    MatterHeader(MatterId) = "matter_header",
    /// One version in a matter's timeline.
    Version(CheckpointId) = "version",
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind(), self.uuid().hyphenated())
    }
}

impl Serialize for EntryId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for EntryId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::<Self>::new("an entry id, kind:uuid"))
    }
}

#[cfg(test)]
mod tests;
