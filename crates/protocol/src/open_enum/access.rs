//! The deserializers [`OpenDataEnum::known`](super::OpenDataEnum::known) feeds a derived enum
//! decoder: the variant goes by its index, already matched from the kind string once, and a
//! struct variant's payload is read straight from the map, with nothing buffered.
//!
//! A variant never changes shape, so each form of a known kind reads as one shape only: a plain
//! string as a unit variant, a one-key map as a struct variant. A unit kind written as a map, or
//! a struct kind written as a plain string, is an error that names the kind.

use std::marker::PhantomData;

use serde::de::value::U32Deserializer;
use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, MapAccess, Unexpected, VariantAccess, Visitor,
};

/// A known kind and the map that holds its payload, read as an externally tagged enum: the kind
/// (by index) is the variant, the map's next value its content. Only enum decoders accept it.
pub(super) struct KnownKind<'m, 'k, A> {
    index: u32,
    kind: &'k str,
    map: &'m mut A,
}

impl<'m, 'k, A> KnownKind<'m, 'k, A> {
    /// The kind `kind`, at `index` among the rows, its payload the next value of `map`.
    pub(super) fn new(index: u32, kind: &'k str, map: &'m mut A) -> Self {
        Self { index, kind, map }
    }
}

impl<'de, A: MapAccess<'de>> Deserializer<'de> for KnownKind<'_, '_, A> {
    type Error = A::Error;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, A::Error> {
        Err(de::Error::custom("a known kind decodes only as an enum"))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        visitor.visit_enum(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map struct identifier
        ignored_any
    }
}

impl<'de, A: MapAccess<'de>> EnumAccess<'de> for KnownKind<'_, '_, A> {
    type Error = A::Error;
    type Variant = Self;

    // The index is the row's position, which is the variant's index in the private copy of the
    // known variants only because that copy takes no variant attributes (`skip`, `other`) that
    // would shift it.
    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), A::Error> {
        let variant = seed.deserialize(U32Deserializer::<A::Error>::new(self.index))?;
        Ok((variant, self))
    }
}

impl<'de, A: MapAccess<'de>> VariantAccess<'de> for KnownKind<'_, '_, A> {
    type Error = A::Error;

    /// A unit kind written as a key, `{"closed": ..}`: an error whatever the payload, since a
    /// unit variant is always a plain string.
    fn unit_variant(self) -> Result<(), A::Error> {
        Err(unit_kind_as_map(self.kind))
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, A::Error> {
        self.map.next_value_seed(seed)
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, A::Error> {
        Err(de::Error::custom("open enums have no tuple variants"))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.map.next_value_seed(StructSeed { fields, visitor })
    }
}

/// The error for a known unit kind written as a one-key map, in either kind of open enum.
pub(super) fn unit_kind_as_map<E: de::Error>(kind: &str) -> E {
    E::custom(format_args!(
        "`{kind}` has no fields, so it is a plain string, not a map"
    ))
}

/// Reads a struct variant's content with the variant's own visitor.
struct StructSeed<V> {
    fields: &'static [&'static str],
    visitor: V,
}

impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for StructSeed<V> {
    type Value = V::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        deserializer.deserialize_struct("", self.fields, self.visitor)
    }
}

/// A known kind read from a plain string, by index: a unit variant decodes; a struct variant is
/// an error that names the kind.
pub(super) struct UnitKind<'k, E> {
    index: u32,
    kind: &'k str,
    error: PhantomData<E>,
}

impl<'k, E> UnitKind<'k, E> {
    /// The kind `kind`, at `index` among the rows.
    pub(super) fn new(index: u32, kind: &'k str) -> Self {
        Self {
            index,
            kind,
            error: PhantomData,
        }
    }
}

impl<'de, E: de::Error> Deserializer<'de> for UnitKind<'_, E> {
    type Error = E;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, E> {
        Err(E::custom("a known kind decodes only as an enum"))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, E> {
        visitor.visit_enum(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map struct identifier
        ignored_any
    }
}

impl<'de, E: de::Error> EnumAccess<'de> for UnitKind<'_, E> {
    type Error = E;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), E> {
        let variant = seed.deserialize(U32Deserializer::<E>::new(self.index))?;
        Ok((variant, self))
    }
}

impl<'de, E: de::Error> VariantAccess<'de> for UnitKind<'_, E> {
    type Error = E;

    fn unit_variant(self) -> Result<(), E> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, _seed: T) -> Result<T::Value, E> {
        Err(E::invalid_type(Unexpected::UnitVariant, &"newtype variant"))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, _visitor: V) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::UnitVariant, &"tuple variant"))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, E> {
        Err(struct_kind_as_string(self.kind))
    }
}

/// The error for a known kind with fields written as a plain string.
fn struct_kind_as_string<E: de::Error>(kind: &str) -> E {
    E::custom(format_args!(
        "`{kind}` has fields, so it is a one-key map, not a plain string"
    ))
}
