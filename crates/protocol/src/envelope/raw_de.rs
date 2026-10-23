//! A [`Deserializer`] over a [`RawPayload`] already read, so an `event` that came before its
//! envelope's `schema_version`, held by [`HeldSeed`](super::held::HeldSeed), decodes once the
//! version is known: as this version's [`DomainEvent`](crate::DomainEvent), or as the
//! `RawPayload` an older version's upcast takes, under `RawPayload`'s own rules.
//!
//! It hands each visitor what the format the payload was read from would have: borrowed strings
//! and bytes, `MessagePack`'s binary data as bytes, and the format's `is_human_readable`, so ids
//! and hashes read the same as when the event is decoded in place. A string is a unit variant and
//! a map with one entry an enum's variant with its value, as in JSON and `MessagePack`.

use std::marker::PhantomData;

use serde::de::value::{BorrowedStrDeserializer, MapDeserializer, SeqDeserializer};
use serde::de::{self, DeserializeSeed, Deserializer, EnumAccess, IntoDeserializer, VariantAccess};
use serde::de::{Deserialize, Visitor};

use crate::RawPayload;

/// Reads `value` as a format would that reports `human_readable` as its `is_human_readable`.
pub(super) struct RawDeserializer<'de, E> {
    /// The value read.
    value: &'de RawPayload,
    /// The `is_human_readable` of the format `value` was read from.
    human_readable: bool,
    /// The format's error type.
    error: PhantomData<E>,
}

impl<'de, E> RawDeserializer<'de, E> {
    /// Reads `value`, read from a format whose `is_human_readable` is `human_readable`.
    pub(super) const fn new(value: &'de RawPayload, human_readable: bool) -> Self {
        Self {
            value,
            human_readable,
            error: PhantomData,
        }
    }

    /// Reads `value`, a part of this one, from the same format.
    const fn part(&self, value: &'de RawPayload) -> Self {
        Self::new(value, self.human_readable)
    }
}

impl<'de, E: de::Error> IntoDeserializer<'de, E> for RawDeserializer<'de, E> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de, E: de::Error> Deserializer<'de> for RawDeserializer<'de, E> {
    type Error = E;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, E> {
        match self.value {
            RawPayload::Null => visitor.visit_unit(),
            RawPayload::Bool(value) => visitor.visit_bool(*value),
            RawPayload::I64(value) => visitor.visit_i64(*value),
            RawPayload::U64(value) => visitor.visit_u64(*value),
            RawPayload::F64(value) => visitor.visit_f64(*value),
            RawPayload::Str(text) => visitor.visit_borrowed_str(text),
            RawPayload::Bytes(bytes) => visitor.visit_borrowed_bytes(bytes),
            RawPayload::Seq(items) => {
                let mut seq = SeqDeserializer::new(items.iter().map(|item| self.part(item)));
                let value = visitor.visit_seq(&mut seq)?;
                seq.end()?;
                Ok(value)
            }
            RawPayload::Map(entries) => {
                let parts = entries
                    .iter()
                    .map(|(key, value)| (self.part(key), self.part(value)));
                let mut map = MapDeserializer::new(parts);
                let value = visitor.visit_map(&mut map)?;
                map.end()?;
                Ok(value)
            }
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, E> {
        if matches!(self.value, RawPayload::Null) {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, E> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, E> {
        if let RawPayload::Str(text) = self.value {
            return visitor.visit_enum(BorrowedStrDeserializer::new(text));
        }
        if let RawPayload::Map(entries) = self.value
            && let [(key, value)] = &**entries
        {
            return visitor.visit_enum(Variant {
                key: self.part(key),
                value: self.part(value),
            });
        }
        // Anything else is not an enum; the visitor names what it expected.
        self.deserialize_any(visitor)
    }

    fn is_human_readable(&self) -> bool {
        self.human_readable
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf unit
        unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

/// An enum's variant written as a map with one entry: the variant's name and its value.
struct Variant<'de, E> {
    /// The variant's name.
    key: RawDeserializer<'de, E>,
    /// Its value.
    value: RawDeserializer<'de, E>,
}

impl<'de, E: de::Error> EnumAccess<'de> for Variant<'de, E> {
    type Error = E;
    type Variant = RawDeserializer<'de, E>;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), E> {
        Ok((seed.deserialize(self.key)?, self.value))
    }
}

impl<'de, E: de::Error> VariantAccess<'de> for RawDeserializer<'de, E> {
    type Error = E;

    fn unit_variant(self) -> Result<(), E> {
        <()>::deserialize(self)
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, E> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, E> {
        self.deserialize_any(visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, E> {
        self.deserialize_any(visitor)
    }
}
