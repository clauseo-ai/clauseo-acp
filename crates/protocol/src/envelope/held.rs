//! `HeldSeed`: reads an `event` that came before its envelope's `schema_version` into a
//! [`RawPayload`], under none of `RawPayload`'s own rules, so the version can pick how it is read.
//!
//! It keeps any value the format reads: lists and maps nested as deep as the format allows
//! (`serde_json`'s recursion limit; a `MessagePack` reader's `MAX_MSGPACK_DEPTH`, which it sets
//! on its decoder), and any map key, a list or a map included. [`MAX_RAW_DEPTH`] and the map-key
//! rule apply where the held event is decoded through
//! [`RawDeserializer`](super::raw_de::RawDeserializer): each `RawPayload` the event holds counts
//! from its own root there, as when the event is decoded in place.

use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::RawPayload;
#[cfg(doc)]
use crate::{MAX_MSGPACK_DEPTH, MAX_RAW_DEPTH};

/// Reads one value as the format gives it, with no depth limit or key rule of its own.
#[derive(Clone, Copy)]
pub(super) struct HeldSeed;

impl<'de> DeserializeSeed<'de> for HeldSeed {
    type Value = RawPayload;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<RawPayload, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for HeldSeed {
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

    /// A `MessagePack` ext value, which rmp-serde hands over as a newtype around its type and
    /// data.
    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<RawPayload, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<RawPayload, E> {
        Ok(RawPayload::Bool(v))
    }

    /// Kept as given: the decode through the held value reads a non-negative one as a
    /// `RawPayload` would.
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<RawPayload, E> {
        Ok(RawPayload::I64(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<RawPayload, E> {
        Ok(RawPayload::U64(v))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<RawPayload, E> {
        Ok(RawPayload::F64(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<RawPayload, E> {
        Ok(RawPayload::Str(v.into()))
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<RawPayload, E> {
        Ok(RawPayload::Bytes(v.into()))
    }

    /// Grows as items arrive, never trusting a `MessagePack` header's claimed length.
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawPayload, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(self)? {
            items.push(item);
        }
        Ok(RawPayload::Seq(items.into_boxed_slice()))
    }

    /// Any key, a list or a map included; grows as entries arrive.
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawPayload, A::Error> {
        let mut entries = Vec::new();
        while let Some(key) = map.next_key_seed(self)? {
            entries.push((key, map.next_value_seed(self)?));
        }
        Ok(RawPayload::Map(entries.into_boxed_slice()))
    }
}
