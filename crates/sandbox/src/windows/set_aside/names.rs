//! Names kept as the file system gives them (S15 `PR3b` round 4). NTFS allows any UTF-16 name,
//! a lone surrogate included, which isn't text; the scan, the record and the manifest keep
//! every name as it is, so such a name is recorded and compared like any other.
//!
//! In the JSON files, a name or path that is text is a string; one that isn't is
//! `{"utf16": [<unit>, ...]}`, its UTF-16 code units as numbers, which reads back to the same
//! name. On Windows every name is UTF-16, so every name can be written. Elsewhere (the tests'
//! stand-in) a name is bytes, read as WTF-8 (UTF-8 that also holds lone surrogates); bytes
//! that aren't WTF-8 can't be written, and the file's write fails.

#[cfg(any(windows, test))]
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[cfg(any(windows, test))]
use super::files::FileId;

/// An item's path below the matter's root, one name per folder and its own name last: its key
/// in the record.
#[cfg(any(windows, test))]
pub(super) type Key = Vec<OsString>;

/// One name as the JSON files hold it.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Raw {
    /// A name that is text.
    Text(String),
    /// A name that isn't: its UTF-16 code units.
    Units { utf16: Vec<u16> },
}

impl Raw {
    fn of(name: &OsStr) -> Option<Raw> {
        if let Some(text) = name.to_str() {
            return Some(Raw::Text(text.to_owned()));
        }
        #[cfg(windows)]
        let utf16 = Some(units(name));
        #[cfg(not(windows))]
        let utf16 = units(name);
        utf16.map(|utf16| Raw::Units { utf16 })
    }

    fn name(self) -> Option<OsString> {
        match self {
            Raw::Text(text) => Some(OsString::from(text)),
            Raw::Units { utf16 } => {
                #[cfg(windows)]
                let name = Some(from_units(&utf16));
                #[cfg(not(windows))]
                let name = from_units(&utf16);
                name
            }
        }
    }
}

/// `name` as UTF-16 code units: on Windows what it is.
#[cfg(windows)]
fn units(name: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;
    name.encode_wide().collect()
}

/// The name these UTF-16 code units spell: on Windows, any.
#[cfg(windows)]
fn from_units(units: &[u16]) -> OsString {
    use std::os::windows::ffi::OsStringExt as _;
    OsString::from_wide(units)
}

/// `name`'s bytes read as WTF-8, as UTF-16 code units; `None` when they aren't WTF-8.
#[cfg(not(windows))]
fn units(name: &OsStr) -> Option<Vec<u16>> {
    use std::os::unix::ffi::OsStrExt as _;
    let bytes = name.as_bytes();
    let mut out: Vec<u16> = Vec::new();
    let mut at = 0;
    while let Some(&first) = bytes.get(at) {
        let (len, lead) = match first {
            0x00..=0x7F => (1, u32::from(first)),
            0xC2..=0xDF => (2, u32::from(first & 0x1F)),
            0xE0..=0xEF => (3, u32::from(first & 0x0F)),
            0xF0..=0xF4 => (4, u32::from(first & 0x07)),
            _ => return None,
        };
        let mut point = lead;
        for &next in bytes.get(at + 1..at + len)? {
            if next & 0xC0 != 0x80 {
                return None;
            }
            point = (point << 6) | u32::from(next & 0x3F);
        }
        let least = match len {
            1 => 0,
            2 => 0x80,
            3 => 0x800,
            _ => 0x1_0000,
        };
        let trail = (0xDC00..=0xDFFF).contains(&point);
        let after_lead = out
            .last()
            .is_some_and(|unit| (0xD800..=0xDBFF).contains(unit));
        // A lead surrogate then a trail one is one 4-byte sequence in WTF-8, never two.
        if point < least || point > 0x10_FFFF || (trail && after_lead) {
            return None;
        }
        match char::from_u32(point) {
            Some(c) => out.extend(c.encode_utf16(&mut [0; 2]).iter()),
            None => out.push(u16::try_from(point).ok()?),
        }
        at += len;
    }
    Some(out)
}

/// The name these UTF-16 code units spell, as WTF-8 bytes: each lone surrogate as its three
/// bytes.
#[cfg(not(windows))]
fn from_units(units: &[u16]) -> Option<OsString> {
    use std::os::unix::ffi::OsStringExt as _;
    let mut bytes = Vec::new();
    for unit in char::decode_utf16(units.iter().copied()) {
        match unit {
            Ok(c) => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
            Err(lone) => {
                let s = lone.unpaired_surrogate();
                for (shift, mark) in [(12, 0xE0), (6, 0x80), (0, 0x80)] {
                    let bits = if shift == 12 {
                        s >> 12
                    } else {
                        (s >> shift) & 0x3F
                    };
                    bytes.push(mark | u8::try_from(bits).ok()?);
                }
            }
        }
    }
    Some(OsString::from_vec(bytes))
}

fn unwritable<E: serde::ser::Error>() -> E {
    E::custom("a name that can't be written")
}

fn unreadable<E: serde::de::Error>() -> E {
    E::custom("a name that can't be read")
}

/// One name, for `#[serde(with)]`.
pub(super) mod one {
    use super::{Deserialize as _, Deserializer, OsStr, OsString, Raw, Serialize as _, Serializer};
    use super::{unreadable, unwritable};

    pub(in super::super) fn serialize<S: Serializer>(
        name: &OsStr,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Raw::of(name).ok_or_else(unwritable)?.serialize(serializer)
    }

    pub(in super::super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<OsString, D::Error> {
        Raw::deserialize(deserializer)?
            .name()
            .ok_or_else(unreadable)
    }
}

/// A list of names, for `#[serde(with)]`.
pub(super) mod many {
    use super::{Deserialize as _, Deserializer, OsString, Raw, Serialize as _, Serializer};
    use super::{unreadable, unwritable};

    pub(in super::super) fn serialize<S: Serializer>(
        names: &[OsString],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        names
            .iter()
            .map(|name| Raw::of(name))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(unwritable)?
            .serialize(serializer)
    }

    pub(in super::super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<OsString>, D::Error> {
        Vec::<Raw>::deserialize(deserializer)?
            .into_iter()
            .map(Raw::name)
            .collect::<Option<_>>()
            .ok_or_else(unreadable)
    }
}

/// A path, as one name, for `#[serde(with)]`.
pub(super) mod path {
    use super::{Deserializer, PathBuf, Serializer, one};

    pub(in super::super) fn serialize<S: Serializer>(
        path: &std::path::Path,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        one::serialize(path.as_os_str(), serializer)
    }

    pub(in super::super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PathBuf, D::Error> {
        one::deserialize(deserializer).map(PathBuf::from)
    }
}

/// One of the record's items: its path below the root and its file ID.
#[cfg(any(windows, test))]
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept<T> {
    #[serde(with = "many")]
    path: Key,
    id: T,
}

/// The record's names or links, for `#[serde(with)]`: a list of `{"path": [<name>, ...],
/// "id": ...}`, so a key needs no joining and no escaping.
#[cfg(any(windows, test))]
pub(super) mod keyed {
    use super::{BTreeMap, Deserialize, Deserializer, Kept, Key, Serialize, Serializer};

    pub(in super::super) fn serialize<S: Serializer, T: Serialize + Clone>(
        map: &BTreeMap<Key, T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter()
            .map(|(path, id)| Kept {
                path: path.clone(),
                id: id.clone(),
            })
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub(in super::super) fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<Key, T>, D::Error> {
        let list = Vec::<Kept<T>>::deserialize(deserializer)?;
        let count = list.len();
        let map: BTreeMap<Key, T> = list.into_iter().map(|kept| (kept.path, kept.id)).collect();
        if map.len() == count {
            Ok(map)
        } else {
            Err(serde::de::Error::custom("an item recorded twice"))
        }
    }
}

/// Only so the record's types are named here, where its JSON is.
#[cfg(any(windows, test))]
pub(super) type Names = BTreeMap<Key, Option<FileId>>;
/// See [`Names`].
#[cfg(any(windows, test))]
pub(super) type Links = BTreeMap<Key, FileId>;
