//! Tracked changes in a matter's Word file: the list `HostQuery::ListRevisions` answers and
//! the accept or reject `HostCommand::DecideRevisions` asks for. Never logged and in no stream:
//! a query, its answer and a command carry these types. The host reads and edits the file with
//! the `stetdoc` program; the decision writes the file and takes a `revisions` checkpoint.
//!
//! - [`DocVersion`] names the exact bytes a list was read from, so a decision made from an old
//!   list is refused (`word.revisions_stale`) instead of acting on changes that moved.
//! - A revision's `ids` are Word's own change ids (`w:id`), as stetdoc reports them; one
//!   revision can carry several (a replacement is a deletion and an insertion).

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::ids::hex_value;
use crate::open_enum::open_unit_enum;
use crate::{Error, MatterId, MatterPath, Timestamp};

/// Most revisions one `ListRevisions` returns, and most ids one `DecideRevisions` names: one
/// reply and one command stay bounded however heavily a document was edited.
pub const MAX_REVISIONS: usize = 500;

/// Most characters of a revision's `text` and `author` in a `ListRevisions` answer; the host
/// cuts longer ones at a character boundary.
pub const MAX_REVISION_TEXT_CHARS: usize = 2_000;

/// The lowercase hex SHA-256 of the exact `.docx` bytes a revision list was read from. JSON: a
/// string of 64 lowercase hex characters, which is all it decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
pub struct DocVersion([u8; 32]);

impl DocVersion {
    /// Wraps a SHA-256 digest. Every 32 bytes are a valid version.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The digest's bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for DocVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl FromStr for DocVersion {
    type Err = Error;

    /// Parses exactly 64 lowercase hex characters.
    fn from_str(text: &str) -> Result<Self, Error> {
        let rule = Error::Format {
            kind: "DocVersion",
            rule: "expected 64 lowercase hex characters",
        };
        let (pairs, rest) = text.as_bytes().as_chunks::<2>();
        if pairs.len() != 32 || !rest.is_empty() {
            return Err(rule);
        }
        let mut bytes = [0u8; 32];
        for (out, &[high, low]) in bytes.iter_mut().zip(pairs) {
            match (hex_value(high), hex_value(low)) {
                (Some(high), Some(low)) => *out = (high << 4) | low,
                _ => return Err(rule),
            }
        }
        Ok(Self(bytes))
    }
}

impl Serialize for DocVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for DocVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

open_unit_enum! {
    /// What a tracked change does.
    pub enum RevisionKind {
        /// Text or a paragraph inserted.
        Insert,
        /// Text or a paragraph deleted.
        Delete,
        /// Text moved from one place to another.
        Move,
        /// Formatting, a style or a property changed; no text changed.
        Format,
    }
    test mod revision_kind;
}

/// Accept or reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RevisionDecision {
    /// Keep the change: an insertion stays as plain text, a deletion is gone.
    Accept,
    /// Undo the change: an insertion is gone, a deletion comes back.
    Reject,
}

/// One tracked change, as `ListRevisions` lists it. A struct, so later fields are additive; it
/// accepts fields it does not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Revision {
    /// Word's change ids this entry covers, ascending; `DecideRevisions` names them.
    pub ids: Vec<u32>,
    /// What it does.
    pub kind: RevisionKind,
    /// Who made it, as Word recorded it, at most [`MAX_REVISION_TEXT_CHARS`] characters.
    pub author: String,
    /// When, as Word recorded it; missing when the file records no date.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub at: Option<Timestamp>,
    /// The paragraph it is in, as stetdoc names blocks (`p3` is the third paragraph).
    pub block: String,
    /// The text inserted, deleted or moved, at most [`MAX_REVISION_TEXT_CHARS`] characters;
    /// missing for a change with no text (formatting).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text: Option<String>,
}

/// The tracked changes of one Word file, for `ListRevisions`. A struct, so later fields are
/// additive; it accepts fields it does not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RevisionList {
    /// The file.
    pub path: MatterPath,
    /// The bytes the list was read from: `DecideRevisions` sends it back.
    pub version: DocVersion,
    /// The changes in document order (by paragraph, then id). Empty when the file has none.
    pub revisions: Vec<Revision>,
    /// Whether the file holds more than [`MAX_REVISIONS`] changes: `revisions` is then its
    /// first [`MAX_REVISIONS`].
    pub truncated: bool,
    /// How many changes the file holds, `revisions` and any past the cut.
    pub total: u32,
}

/// `HostCommand::DecideRevisions`: accept or reject tracked changes of one Word file. JSON:
/// `{"decide_revisions": {"matter": "…", "path": "a.docx", "version": "…", "ids": [1, 2],
/// "decision": "accept"}}`, or with `"ids": [], "all": true` for every change of that version
/// (Accept all, Reject all); unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DecideRevisions {
    /// The matter.
    pub matter: MatterId,
    /// The Word file, which must end in `.docx` (any case): otherwise `word.not_docx`.
    pub path: MatterPath,
    /// The version `ListRevisions` answered; a file whose bytes differ now is
    /// `word.revisions_stale`, and nothing is written.
    pub version: DocVersion,
    /// The change ids to decide: 1 to [`MAX_REVISIONS`], none twice; none when `all`.
    pub ids: Vec<u32>,
    /// Accept or reject them all.
    pub decision: RevisionDecision,
    /// Every change the file holds at `version`, past [`MAX_REVISIONS`] too: the host reads
    /// them from the same bytes it checked `version` against, so they are the changes that
    /// list was read from. `ids` is then empty. Left out when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
}

impl DecideRevisions {
    /// The shape: 1 to [`MAX_REVISIONS`] ids, none repeated, or none with `all`. Whether the
    /// file is a `.docx` and holds the ids is the host's check.
    pub(crate) fn check(&self) -> Result<(), Error> {
        const KIND: &str = "DecideRevisions";
        if self.all {
            if !self.ids.is_empty() {
                return Err(Error::Format {
                    kind: KIND,
                    rule: "names no change ids when it decides all",
                });
            }
            return Ok(());
        }
        if self.ids.is_empty() {
            return Err(Error::Format {
                kind: KIND,
                rule: "names at least one change id",
            });
        }
        if self.ids.len() > MAX_REVISIONS {
            return Err(Error::TooLarge {
                kind: KIND,
                what: crate::Measure::ChangeIds,
                size: self.ids.len(),
                max: MAX_REVISIONS,
            });
        }
        let mut seen = BTreeSet::new();
        if !self.ids.iter().all(|id| seen.insert(*id)) {
            return Err(Error::Format {
                kind: KIND,
                rule: "names each change id once",
            });
        }
        Ok(())
    }
}
