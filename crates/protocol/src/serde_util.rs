//! Deserialize helpers the id, stream, error and open enum types share: a uuid read strictly, a
//! value parsed from a borrowed `&str`, and a string read without a copy.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};
use uuid::Uuid;

use crate::Error;
use crate::ids::parse_uuid_text;

/// A uuid read strictly: in a human-readable format only the 36-character hyphenated text
/// (uuid's own `Deserialize` also takes the simple, braced and urn forms), otherwise exactly 16
/// bytes. The ids decode through it with `try_from`.
pub(crate) struct StrictUuid(pub(crate) Uuid);

impl<'de> Deserialize<'de> for StrictUuid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = if deserializer.is_human_readable() {
            deserializer.deserialize_str(UuidVisitor { text: true })?
        } else {
            deserializer.deserialize_bytes(UuidVisitor { text: false })?
        };
        Ok(Self(id))
    }
}

struct UuidVisitor {
    /// Text in a human-readable format, bytes otherwise; each refuses the other, because a
    /// format may hand either to any visitor (rmp-serde's `deserialize_bytes` reads a str too).
    text: bool,
}

impl de::Visitor<'_> for UuidVisitor {
    type Value = Uuid;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(if self.text {
            "a hyphenated UUID"
        } else {
            "16 bytes"
        })
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Uuid, E> {
        if !self.text {
            return Err(E::invalid_type(de::Unexpected::Str(v), &self));
        }
        parse_uuid_text(v, "Uuid").map_err(E::custom)
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Uuid, E> {
        if self.text {
            return Err(E::invalid_type(de::Unexpected::Bytes(v), &self));
        }
        match <[u8; 16]>::try_from(v) {
            Ok(bytes) => Ok(Uuid::from_bytes(bytes)),
            Err(_) => Err(E::invalid_length(v.len(), &self)),
        }
    }
}

/// A `Deserialize` visitor that parses a borrowed `&str` with `T::from_str`, never building a
/// `String` first. `StreamId` and `EntryId` use it too.
pub(crate) struct FromStrVisitor<T> {
    expecting: &'static str,
    target: std::marker::PhantomData<T>,
}

impl<T> FromStrVisitor<T> {
    pub(crate) const fn new(expecting: &'static str) -> Self {
        Self {
            expecting,
            target: std::marker::PhantomData,
        }
    }
}

impl<T: FromStr<Err = Error>> de::Visitor<'_> for FromStrVisitor<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expecting)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
        v.parse().map_err(E::custom)
    }
}

/// A string read without copying when the input allows it (`serde_json` without escapes,
/// rmp-serde from a slice), and copied otherwise. Use it for map keys matched as `&str`.
pub(crate) struct BorrowedStr<'de>(Cow<'de, str>);

impl BorrowedStr<'_> {
    /// The text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for BorrowedStr<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(BorrowedStrVisitor(PhantomData))
    }
}

struct BorrowedStrVisitor<'a>(PhantomData<&'a str>);

impl<'de: 'a, 'a> Visitor<'de> for BorrowedStrVisitor<'a> {
    type Value = BorrowedStr<'a>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string")
    }

    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(BorrowedStr(Cow::Borrowed(v)))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(BorrowedStr(Cow::Owned(v.to_owned())))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(BorrowedStr(Cow::Owned(v)))
    }
}
