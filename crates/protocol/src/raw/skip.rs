//! `SkipSeed`: skips a value an open enum's catch-all does not keep, counting its nesting as a
//! [`RawPayload`](crate::RawPayload) does.

use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use super::decode_depth;
#[cfg(doc)]
use super::{MAX_RAW_DEPTH, RawPayload};

/// Skips one value that sits inside `depth` lists and maps, keeping nothing: scalars, strings
/// and bytes are ignored as they come (borrowed or not, nothing is allocated here), and lists
/// and maps are walked with `depth + 1`, so a value nested deeper than [`MAX_RAW_DEPTH`] fails
/// with the same error as a [`RawPayload`]. It goes through `deserialize_any`, never
/// `deserialize_ignored_any`, which `serde_json` answers without showing the nesting.
#[derive(Clone, Copy)]
pub(crate) struct SkipSeed {
    /// The lists and maps around the value.
    pub(crate) depth: usize,
}

impl<'de> DeserializeSeed<'de> for SkipSeed {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl SkipSeed {
    /// The seed for the items of a list or map this value opens; an error past [`MAX_RAW_DEPTH`].
    fn items<E: de::Error>(self) -> Result<Self, E> {
        let depth = decode_depth(self.depth)?;
        Ok(Self { depth })
    }
}

impl<'de> Visitor<'de> for SkipSeed {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any value")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_bytes<E: de::Error>(self, _: &[u8]) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let items = self.items()?;
        while seq.next_element_seed(items)?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let items = self.items()?;
        while map.next_key_seed(items)?.is_some() {
            map.next_value_seed(items)?;
        }
        Ok(())
    }
}
