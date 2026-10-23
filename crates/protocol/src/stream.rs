//! `StreamId` and `EntryId`: enums with data that travel as one plain string, `<kind>:<uuid>`.
//!
//! The same text serves the wire, SQL and the command line. `Serialize` writes it with
//! `collect_str`, straight into the output; `Deserialize` parses the borrowed `&str`. `EntryId`'s
//! variants and `FromStr` come from the `view_entries!` table in `view.rs`, with `ViewEntry`'s.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::serde_util::FromStrVisitor;
use crate::{EntryId, Error, MatterId, ThreadId};

/// Splits `<prefix>:<rest>`, or names the missing separator as a format error.
pub(crate) fn split_kind<'a>(
    text: &'a str,
    kind: &'static str,
) -> Result<(&'a str, &'a str), Error> {
    text.split_once(':').ok_or(Error::Format {
        kind,
        rule: "expected <kind>:<uuid>",
    })
}

pub(crate) fn unknown_prefix(kind: &'static str, prefix: &str) -> Error {
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
    /// A matter's stream: the matter-level kinds, those whose
    /// [`DomainEvent::is_matter_level`](crate::DomainEvent::is_matter_level) is `Some(true)`
    /// (M in 0002's Kinds table).
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
