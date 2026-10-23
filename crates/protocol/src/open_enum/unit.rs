//! The decoder [`open_unit_enum!`](super::open_unit_enum) writes: the kind alone, from a plain
//! string, or an unknown kind from a one-key map.

use std::fmt;

use serde::de::value::StrDeserializer;
use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};

use super::access::unit_kind_as_map;
use crate::raw::SkipSeed;
use crate::serde_util::BorrowedStr;

/// Decodes a unit-only enum through `T`, the derived decoder of its variants and its
/// `#[serde(other)]` catch-all, fed the kind alone. A known value is a plain string `"kind"`;
/// written as a one-key map `{"kind": payload}` it is an error, whatever the payload, since a
/// variant never changes shape. An unknown kind is the catch-all as a plain string or as the key
/// of a one-key map: a newer Clauseo may add a struct variant to the enum. Its payload may be any
/// shape, `null` included, skipped with [`SkipSeed`] (nesting counted), as a data enum's
/// catch-all takes it. `expecting` names the enum in errors; `is_unknown` tells the catch-all
/// apart.
pub(crate) fn deserialize_unit<'de, D, T>(
    deserializer: D,
    expecting: &'static str,
    is_unknown: fn(&T) -> bool,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    deserializer.deserialize_any(UnitVisitor {
        expecting,
        is_unknown,
    })
}

struct UnitVisitor<T> {
    expecting: &'static str,
    is_unknown: fn(&T) -> bool,
}

impl<'de, T: Deserialize<'de>> Visitor<'de> for UnitVisitor<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expecting)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
        T::deserialize(StrDeserializer::<E>::new(v))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
        let Some(key) = map.next_key::<BorrowedStr<'de>>()? else {
            return Err(de::Error::invalid_length(0, &"a map with one key"));
        };
        let kind = T::deserialize(StrDeserializer::<A::Error>::new(key.as_str()))?;
        if !(self.is_unknown)(&kind) {
            return Err(unit_kind_as_map(key.as_str()));
        }
        map.next_value_seed(SkipSeed { depth: 0 })?;
        if map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_length(2, &"a map with one key"));
        }
        Ok(kind)
    }
}
