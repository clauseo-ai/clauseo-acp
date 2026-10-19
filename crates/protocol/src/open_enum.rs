//! Shared machinery for the enums that decode values a newer Clauseo may add.
//!
//! Two shapes, both external tagging (a unit variant is a plain string, a data variant a one-key
//! object):
//! - Unit-only enums: [`open_unit_enum!`] writes the enum with a `#[serde(other)] Unknown`
//!   variant, its `ALL` constant and `variant_name`, from one list, so `ALL` cannot miss a variant.
//! - Enums with data (`DetachReason`, `LimitSource`, `SteerOutcome`): the type implements
//!   [`OpenDataEnum`] by hand and its `Deserialize` calls [`deserialize_open`], which decodes in
//!   one pass: a string becomes a unit variant or `Unknown`; a map's key is matched as a borrowed
//!   `&str` and its payload decoded straight into its type; an unknown key becomes `Unknown`.
//!   Their `Serialize` is derived, with the `Unknown { kind }` variant marked
//!   `#[serde(untagged, serialize_with = "serialize_kind")]` so it writes back the plain string.
//!   [`enum_with_all!`] writes such an enum with its `ALL` and `variant_name` from one list.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

/// Counts identifiers, for the length of an `ALL` array.
macro_rules! count_idents {
    () => { 0usize };
    ($head:ident $($tail:ident)*) => { 1usize + $crate::open_enum::count_idents!($($tail)*) };
}
pub(crate) use count_idents;

/// Writes a unit-only enum that decodes an unknown string to `Unknown`, with `ALL` and
/// `variant_name`.
///
/// It derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, serde and (under the `specta`
/// feature) `specta::Type`, with `rename_all = "snake_case"`. Doc comments and other attributes on
/// the enum and on each variant pass through. `Unknown` comes last, in the enum and in `ALL`.
macro_rules! open_unit_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        #[serde(rename_all = "snake_case")]
        $vis enum $name {
            $($(#[$variant_meta])* $variant,)+
            /// A value from a newer Clauseo that this version does not know.
            #[serde(other)]
            Unknown,
        }

        impl $name {
            /// Every variant, in declaration order, `Unknown` last.
            $vis const ALL: [Self; $crate::open_enum::count_idents!($($variant)+) + 1] =
                [$(Self::$variant,)+ Self::Unknown];

            /// The Rust name of the variant, such as `"Unknown"`.
            $vis const fn variant_name(&self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)+
                    Self::Unknown => "Unknown",
                }
            }
        }
    };
}
pub(crate) use open_unit_enum;

/// Writes an enum of unit and struct variants with `ALL` and `variant_name`, from one list, so
/// `ALL` cannot miss a variant.
///
/// Each struct field is written `name: Type = placeholder`; `ALL` builds each data variant from
/// the placeholders (nil ids, zero timestamps, empty strings), which must be `const`. Nothing is
/// derived: the caller writes the derives, and every attribute passes through.
macro_rules! enum_with_all {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident $({
                    $($(#[$field_meta:meta])* $field:ident: $ty:ty = $placeholder:expr),+ $(,)?
                })?
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant $({ $($(#[$field_meta])* $field: $ty),+ })?,
            )+
        }

        impl $name {
            /// Every variant, in declaration order; data variants hold placeholder data.
            $vis const ALL: [Self; $crate::open_enum::count_idents!($($variant)+)] =
                [$(Self::$variant $({ $($field: $placeholder),+ })?,)+];

            /// The Rust name of the variant, such as `"Unknown"`.
            $vis const fn variant_name(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => stringify!($variant),)+
                }
            }
        }
    };
}
pub(crate) use enum_with_all;

/// Writes an unknown variant's kind back as a plain string. Use it as the
/// `#[serde(untagged, serialize_with = "...")]` of a derived `Unknown { kind }` variant.
pub(crate) fn serialize_kind<S: Serializer>(kind: &str, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(kind)
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

/// An enum with data whose unknown values decode to a catch-all. Implement it by hand and call
/// [`deserialize_open`] from `Deserialize`.
pub(crate) trait OpenDataEnum: Sized {
    /// What the decoder expected, for error messages, such as `"a detach reason"`.
    const EXPECTING: &'static str;
    /// The keys of the data variants. A plain string naming one of them is an error, not
    /// `Unknown`.
    const DATA_KINDS: &'static [&'static str];

    /// The unit variant named `kind`, or `None`.
    fn unit(kind: &str) -> Option<Self>;

    /// When `kind` names a data variant, decodes its payload with `map.next_value()` and
    /// returns it; otherwise returns `Ok(None)` without reading the value.
    fn data<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Option<Self>, A::Error>;

    /// The catch-all for a kind this version does not know.
    fn unknown(kind: &str) -> Self;

    /// The catch-all for an unknown key; the default skips its payload with `IgnoredAny`.
    /// Override it to keep the payload (as a `RawPayload`).
    fn unknown_data<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Self, A::Error> {
        map.next_value::<IgnoredAny>()?;
        Ok(Self::unknown(kind))
    }
}

/// Decodes an [`OpenDataEnum`] in one pass, from any self-describing format.
pub(crate) fn deserialize_open<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: OpenDataEnum,
{
    deserializer.deserialize_any(OpenVisitor(PhantomData))
}

struct OpenVisitor<T>(PhantomData<T>);

impl<'de, T: OpenDataEnum> Visitor<'de> for OpenVisitor<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(T::EXPECTING)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
        if T::DATA_KINDS.contains(&v) {
            return Err(E::invalid_type(de::Unexpected::Str(v), &self));
        }
        Ok(T::unit(v).unwrap_or_else(|| T::unknown(v)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
        let Some(key) = map.next_key::<BorrowedStr<'de>>()? else {
            return Err(de::Error::invalid_length(0, &"a map with one key"));
        };
        let kind = key.as_str();
        let value = if let Some(value) = T::data(kind, &mut map)? {
            value
        } else if let Some(value) = T::unit(kind) {
            map.next_value::<()>()?;
            value
        } else {
            T::unknown_data(kind, &mut map)?
        };
        if map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_length(2, &"a map with one key"));
        }
        Ok(value)
    }
}

/// Writes a one-key map `{key: value}`, the form of a data variant.
pub(crate) fn serialize_one_key<S, V>(
    serializer: S,
    key: &str,
    value: &V,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    V: Serialize + ?Sized,
{
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry(key, value)?;
    map.end()
}

#[cfg(test)]
mod tests;
