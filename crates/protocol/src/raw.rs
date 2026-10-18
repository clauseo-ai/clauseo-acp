//! `RawPayload`: a value of any shape, kept exactly as read so it can be written back.
//!
//! It is what an unknown event's payload, an elicitation schema and an unknown content block are
//! held as. It reads through `deserialize_any`, so it works the same from JSON (the wire) and from
//! `MessagePack` (R7's stored envelope), and writes back variant for variant, map entries in their
//! original order. `serde_json::Value` would sort map keys and refuse `MessagePack`'s binary data;
//! `serde_json`'s `RawValue` works only inside `serde_json`.

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

use crate::Error;

/// A value of any shape, in the order it was read.
///
/// Written back with the format's own encoding: a value read from `serde_json` writes back the
/// same compact JSON, and one read from rmp-serde the same `MessagePack`, as long as the input was
/// in that library's canonical form (see the crate's tests for the exceptions).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = JsonValue))]
pub enum RawPayload {
    /// JSON `null`, `MessagePack` `nil`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A negative integer (formats report non-negative ones as `U64`).
    I64(i64),
    /// A non-negative integer.
    U64(u64),
    /// A float. `MessagePack`'s 32-bit floats widen to this and write back as 64-bit.
    F64(f64),
    /// A string.
    Str(String),
    /// Binary data (`MessagePack` `bin`; JSON has none).
    Bytes(Vec<u8>),
    /// A list.
    Seq(Vec<RawPayload>),
    /// A map, entries in their original order; keys may repeat if the input repeated them.
    Map(Vec<(RawPayload, RawPayload)>),
}

impl RawPayload {
    /// The value as compact JSON text, as `serde_json` writes it.
    ///
    /// Fails with [`Error::NotJson`] when a map key has no JSON form (binary data, a list, a map
    /// or null), which only `MessagePack` input can produce.
    pub fn to_json_text(&self) -> Result<String, Error> {
        serde_json::to_string(self).map_err(Error::NotJson)
    }
}

impl Serialize for RawPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::I64(value) => serializer.serialize_i64(*value),
            Self::U64(value) => serializer.serialize_u64(*value),
            Self::F64(value) => serializer.serialize_f64(*value),
            Self::Str(value) => serializer.serialize_str(value),
            Self::Bytes(value) => serializer.serialize_bytes(value),
            Self::Seq(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                items
                    .iter()
                    .try_for_each(|item| seq.serialize_element(item))?;
                seq.end()
            }
            Self::Map(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                entries
                    .iter()
                    .try_for_each(|(key, value)| map.serialize_entry(key, value))?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for RawPayload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(RawVisitor)
    }
}

struct RawVisitor;

impl<'de> Visitor<'de> for RawVisitor {
    type Value = RawPayload;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<RawPayload, E> {
        Ok(RawPayload::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<RawPayload, E> {
        Ok(RawPayload::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<RawPayload, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<RawPayload, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<RawPayload, E> {
        Ok(RawPayload::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<RawPayload, E> {
        // A format that reports a non-negative number as signed still gets one form per number.
        Ok(u64::try_from(v).map_or(RawPayload::I64(v), RawPayload::U64))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<RawPayload, E> {
        Ok(RawPayload::U64(v))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<RawPayload, E> {
        Ok(RawPayload::F64(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<RawPayload, E> {
        Ok(RawPayload::Str(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<RawPayload, E> {
        Ok(RawPayload::Str(v))
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<RawPayload, E> {
        Ok(RawPayload::Bytes(v.to_vec()))
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<RawPayload, E> {
        Ok(RawPayload::Bytes(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawPayload, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(RawPayload::Seq(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawPayload, A::Error> {
        let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(0).min(4096));
        while let Some(entry) = map.next_entry()? {
            entries.push(entry);
        }
        Ok(RawPayload::Map(entries))
    }
}

/// The TypeScript shape of a [`RawPayload`]: any JSON value. specta has no `unknown` outside
/// specta-typescript, which this crate may not depend on, so the union says the same thing.
#[cfg(feature = "specta")]
#[derive(serde::Serialize, specta::Type)]
#[serde(untagged)]
#[expect(
    dead_code,
    reason = "only its specta::Type impl is used, to describe RawPayload"
)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<JsonValue>),
    Object(std::collections::BTreeMap<String, JsonValue>),
}

#[cfg(test)]
mod tests;
