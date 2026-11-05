//! A skill pack's id and a matter's list of them (decision record 0040): the one id rule, and a
//! list that refuses duplicates and more than [`PACK_LIST_MAX`] entries when it decodes.

use std::fmt;
use std::str::FromStr;

use compact_str::CompactString;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::{Error, is_windows_reserved_name};

/// The fewest and most bytes a pack id has.
const PACK_ID_BYTES: std::ops::RangeInclusive<usize> = 2..=30;

/// Ids no pack may take: Clauseo's own pack and skill names, and the names of the built-in
/// skills (`skills::builtin::BUILTIN` in `profile`, which tests that this list holds them). Any
/// id under `clauseo-` is reserved too.
const RESERVED_IDS: [&str; 6] = [
    "clauseo",
    "builtin",
    "stetdoc",
    "legal-drafting",
    "legal-research",
    "matter-brief",
];

/// The rule a pack id breaks, as one sentence.
const PACK_ID_RULE: &str = "a pack id is 2 to 30 lowercase letters or digits, with single hyphens \
    between them, not a Windows device name, and not clauseo, builtin, stetdoc, a built-in \
    skill's name, or anything starting clauseo-";

/// The most ids a matter's `skill_packs` list holds.
pub const PACK_LIST_MAX: usize = 32;

/// The rule a pack list breaks, as one sentence, for each of its two limits.
const PACK_LIST_RULES: [&str; 2] = [
    "a matter's skill packs hold at most 32 pack ids",
    "a matter's skill packs hold no pack id twice",
];

/// The id of a skill pack, as the lawyer's skills settings and a matter's list name it: the
/// text `^[a-z0-9]+(-[a-z0-9]+)*$`, 2 to 30 bytes, not a Windows device name, and not a reserved
/// name. [`PackId::new`], `FromStr` and `Deserialize` check the same rule; serialised as a plain
/// string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
pub struct PackId(CompactString);

impl PackId {
    /// Checks `value` with the id rule.
    ///
    /// # Errors
    ///
    /// [`Error::Format`] (`kind` `"PackId"`) when `value` breaks the rule.
    pub fn new(value: &str) -> Result<Self, Error> {
        if is_pack_id(value) {
            Ok(Self(CompactString::from(value)))
        } else {
            Err(Error::Format {
                kind: "PackId",
                rule: PACK_ID_RULE,
            })
        }
    }

    /// The text inside.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether `id` keeps the id rule ([`PackId`]).
fn is_pack_id(id: &str) -> bool {
    PACK_ID_BYTES.contains(&id.len())
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !is_windows_reserved_name(id)
        && !RESERVED_IDS.contains(&id)
        && !id.starts_with("clauseo-")
}

impl fmt::Display for PackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for PackId {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        Self::new(text)
    }
}

impl Serialize for PackId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PackId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = CompactString::deserialize(deserializer)?;
        Self::new(&value).map_err(de::Error::custom)
    }
}

/// A matter's skill packs: the pack ids it may load, in the order the lawyer gave. At most
/// [`PACK_LIST_MAX`] ids and no id twice; an empty list is valid and means no extra pack for the
/// matter. [`PackList::new`] and `Deserialize` check the same rules; serialised as an array of
/// strings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = Vec<String>))]
pub struct PackList(Vec<PackId>);

impl PackList {
    /// The list of `ids`.
    ///
    /// # Errors
    ///
    /// [`Error::Format`] (`kind` `"PackList"`) for more than [`PACK_LIST_MAX`] ids, or an id
    /// given twice.
    pub fn new(ids: Vec<PackId>) -> Result<Self, Error> {
        if ids.len() > PACK_LIST_MAX {
            return Err(Error::Format {
                kind: "PackList",
                rule: PACK_LIST_RULES[0],
            });
        }
        let twice = ids
            .iter()
            .enumerate()
            .any(|(at, id)| ids.iter().take(at).any(|earlier| earlier == id));
        if twice {
            return Err(Error::Format {
                kind: "PackList",
                rule: PACK_LIST_RULES[1],
            });
        }
        Ok(Self(ids))
    }

    /// The ids, in the order they were given.
    pub fn as_slice(&self) -> &[PackId] {
        &self.0
    }
}

impl Serialize for PackList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(&self.0)
    }
}

impl<'de> Deserialize<'de> for PackList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let ids = Vec::<PackId>::deserialize(deserializer)?;
        Self::new(ids).map_err(de::Error::custom)
    }
}
