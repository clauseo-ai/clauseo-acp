//! Shared machinery for the enums that decode values a newer Clauseo may add.
//!
//! The serde is written by hand because each derived form loses a guarantee: `#[serde(other)]`
//! takes only a unit catch-all and drops the kind; an untagged catch-all buffers every value and
//! turns a known kind's bad payload into the catch-all; mirror structs can leave a variant out.
//!
//! Two shapes, both external tagging (a unit variant is a plain string, a data variant a one-key
//! object):
//! - Unit-only enums: [`open_unit_enum!`] writes the enum with a `#[serde(other)] Unknown`
//!   variant, its `ALL` constant and `variant_name`, from one list, so `ALL` cannot miss a variant.
//!   A known value is a plain string; an unknown kind is `Unknown` as a plain string or as the
//!   key of a one-key map, its payload skipped, since a newer Clauseo may add a struct variant.
//! - Enums with data (`DetachReason`, `ContentBlock` and the others): [`open_data_enum!`] writes
//!   the enum, its `VARIANT_NAMES`, `variant_name`, its kinds and its decoder from one list of
//!   `Variant = "kind"` rows. Nothing about a variant is written by hand: its payload decodes
//!   through a derived decoder of the known variants, so a new variant needs only its row.
//!   [`deserialize_open`] decodes in one pass: a string becomes a unit variant or the catch-all;
//!   a map's key is matched as a borrowed `&str` and its payload decoded straight into its
//!   variant; an unknown key becomes the catch-all ([`CatchAll`]).
//!
//! A variant never changes shape: a unit variant stays a plain string and a struct variant a
//! one-key map, and a variant that needs fields it lacks is a new variant, which an older reader
//! decodes to the catch-all. A known kind in the other form is an error (`access`), and so is
//! any payload under a known unit kind's key.

use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

use crate::raw::SkipSeed;
use crate::serde_util::BorrowedStr;

mod access;
mod unit;
mod unknown_kind;
mod with_names;

use access::{KnownKind, UnitKind};
pub(crate) use unit::deserialize_unit;
pub use unknown_kind::UnknownKind;
pub(crate) use with_names::enum_with_names;

/// Counts identifiers, for the length of an `ALL` or `VARIANT_NAMES` array.
macro_rules! count_idents {
    () => { 0usize };
    ($head:ident $($tail:ident)*) => { 1usize + $crate::open_enum::count_idents!($($tail)*) };
}
pub(crate) use count_idents;

/// Writes a unit-only enum that decodes an unknown string to `Unknown`, with `ALL` and
/// `variant_name`.
///
/// It derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, serde's `Serialize` and (under
/// the `specta` feature) `specta::Type`, with `rename_all = "snake_case"`. Doc comments on the enum
/// and on each variant pass through, and no other attribute is accepted: a macro body cannot take
/// an attribute's name from a metavariable, so each form is named. `Unknown` comes last, in the
/// enum and in `ALL`. It decodes through [`deserialize_unit`]: a known value from its plain
/// string only; a kind this version does not know, as a plain string or as the key of a one-key
/// map `{"kind": payload}` (payload skipped, any shape), becoming `Unknown`.
///
/// After the enum, a line `test mod name;` names the test module the macro writes beside it: it
/// round-trips every value of `ALL` in JSON and `MessagePack` and reads an unknown kind, as a
/// string and as a key, to `Unknown` (`check::unit_enum`, in tests). The line is required.
macro_rules! open_unit_enum {
    (
        $(#[doc = $doc:expr])*
        $vis:vis enum $name:ident {
            $($(#[doc = $variant_doc:expr])* $variant:ident,)+
        }
        test mod $test:ident;
    ) => {
        $(#[doc = $doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        #[serde(rename_all = "snake_case")]
        $vis enum $name {
            $($(#[doc = $variant_doc])* $variant,)+
            /// A value from a newer Clauseo that this version does not know.
            #[serde(other)]
            Unknown,
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                /// The variants, for their derived decoder of a kind. It shadows the public
                /// enum's name inside this function; `Self` is still the public enum.
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "snake_case")]
                enum $name {
                    $($(#[doc = $variant_doc])* $variant,)+
                    #[serde(other)]
                    Unknown,
                }

                let expecting = concat!("enum ", stringify!($name));
                let is_unknown = |kind: &$name| matches!(kind, $name::Unknown);
                Ok(match $crate::open_enum::deserialize_unit(deserializer, expecting, is_unknown)? {
                    $($name::$variant => Self::$variant,)+
                    $name::Unknown => Self::Unknown,
                })
            }
        }

        #[cfg(test)]
        mod $test {
            #[test]
            fn every_value_round_trips_and_unknown_kinds_read_as_unknown() {
                $crate::open_enum::check::unit_enum(
                    &super::$name::ALL,
                    &super::$name::Unknown,
                    super::$name::variant_name,
                );
            }
        }

        impl $name {
            /// Every variant, in declaration order, `Unknown` last.
            $vis const ALL: [Self; $crate::open_enum::count_idents!($($variant)+) + 1] =
                [$(Self::$variant,)+ Self::Unknown];

            /// The Rust name of the variant, such as `"Unknown"`, for tests.
            #[doc(hidden)]
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

/// Writes an enum with data that decodes values a newer Clauseo may add, from one list.
///
/// Each row is `Variant = "kind"` for a unit variant or `Variant = "kind" { field: Type, .. }`
/// for a struct variant; `VARIANT_NAMES` lists the variants' Rust names, the catch-all last.
/// There are no tuple variants, so every variant can gain a field (an `Option` or a
/// `#[serde(default)]` one, so the old form still decodes). The rows give the serde names,
/// [`OpenDataEnum::kind_index`] and the decoder, which goes through a private copy of the known
/// variants with a derived `Deserialize`: field attributes such as `default` work there as on
/// the enum. The enum takes no container attributes (only doc comments and `#[derive(..)]`), so
/// an open enum accepts fields it does not know, as every logged type must; a command uses a
/// closed twin with `deny_unknown_fields` instead (`RestoreRequest` for `RestoreScope`). The copy
/// carries the enum's name, so decode errors name the public enum, and so does the decoder's
/// `expecting` (`enum DetachReason`).
///
/// A last row `_ => /// doc Unknown,` adds the catch-all `Unknown { kind }`, last in the enum
/// and in `VARIANT_NAMES`, and its [`CatchAll`]. Its `kind` is an [`UnknownKind`], which only this
/// crate builds, and the macro writes its serde form ([`serialize_kind`], the plain string) and
/// its TypeScript type (`string`). Without that line one of the rows is the catch-all and the
/// caller implements [`CatchAll`].
///
/// The macro derives serde's `Serialize` and (under the `specta` feature) `specta::Type`; the
/// caller writes the other derives.
///
/// After the enum, a line `test mod name with [sample, ..];` names the test module the macro
/// writes beside the enum and gives one sample value per row, compiled only in tests (the
/// `_ => Unknown` row has none; a catch-all row such as `Unsupported` does). The test fails
/// unless the samples name every row exactly once; it round-trips each in JSON and
/// `MessagePack`, and reads an unknown kind, as a string and as a key, to the catch-all
/// (`check::data_enum`, in tests). The line is required, so no enum goes untested.
macro_rules! open_data_enum {
    (
        $(#[doc = $doc:expr])*
        $(#[derive($($derive:ident),* $(,)?)])*
        $vis:vis enum $name:ident {
            $(
                $(#[doc = $variant_doc:expr])*
                $variant:ident = $kind:literal $({
                        $(
                            $(#[doc = $field_doc:expr])*
                            $(#[serde($($serde_key:ident $(= $serde_value:literal)?),+)])*
                            $(#[cfg_attr(feature = "specta", specta(type = $spec:ty))])?
                            $field:ident: $ty:ty
                        ),+ $(,)?
                })?
            ),+ $(,)?
            $(
                _ => $(#[doc = $unknown_doc:expr])*
                    $unknown:ident $(,)?
            )?
        }
        test mod $test:ident with [$($sample:expr),+ $(,)?];
    ) => {
        $(#[doc = $doc])*
        $(#[derive($($derive),*)])*
        #[derive(serde::Serialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        $vis enum $name {
            $(
                $(#[doc = $variant_doc])*
                #[serde(rename = $kind)]
                $variant $({
                    $(
                        $(#[doc = $field_doc])*
                        $(#[serde($($serde_key $(= $serde_value)?),+)])*
                        $(#[cfg_attr(feature = "specta", specta(type = $spec))])?
                        $field: $ty
                    ),+
                })?,
            )+
            $(
                $(#[doc = $unknown_doc])*
                #[serde(untagged, serialize_with = "crate::open_enum::serialize_kind")]
                #[cfg_attr(feature = "specta", specta(type = String))]
                $unknown {
                    /// The kind as read; only this crate builds one.
                    kind: $crate::open_enum::UnknownKind<$name>,
                },
            )?
        }

        impl $name {
            /// The Rust name of every variant, in declaration order, the catch-all last.
            $vis const VARIANT_NAMES: [&'static str; $crate::open_enum::count_idents!(
                $($variant)+ $($unknown)?
            )] = [$(stringify!($variant),)+ $(stringify!($unknown),)?];

            /// The Rust name of the variant, such as `"Unknown"`, for tests.
            #[doc(hidden)]
            $vis const fn variant_name(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => stringify!($variant),)+
                    $(Self::$unknown { .. } => stringify!($unknown),)?
                }
            }
        }

        $(
            impl $crate::open_enum::CatchAll for $name {
                fn unknown(kind: &str) -> Self {
                    Self::$unknown {
                        kind: $crate::open_enum::UnknownKind::new(kind),
                    }
                }
            }
        )?

        const _: () = {

            impl $crate::open_enum::OpenDataEnum for $name {
                const EXPECTING: &'static str = concat!("enum ", stringify!($name));
                fn kind_index(kind: &str) -> Option<u32> {
                    /// The variants' indexes, in row order, as the decoder numbers them.
                    #[repr(u32)]
                    enum Index {
                        $($variant,)+
                    }
                    match kind {
                        $($kind => Some(Index::$variant as u32),)+
                        _ => None,
                    }
                }

                fn known<'de, D: serde::Deserializer<'de>>(
                    deserializer: D,
                ) -> Result<Self, D::Error> {
                    /// The known variants, for their derived decoder. It shadows the public
                    /// enum's name inside this function, so decode errors name the public enum;
                    /// `Self` is still the public enum.
                    #[derive(serde::Deserialize)]
                    enum $name {
                        $(
                            $(#[doc = $variant_doc])*
                            #[serde(rename = $kind)]
                            $variant $({
                                $(
                                    $(#[doc = $field_doc])*
                                    $(#[serde($($serde_key $(= $serde_value)?),+)])*
                                    $field: $ty
                                ),+
                            })?,
                        )+
                    }

                    Ok(match <$name as serde::Deserialize>::deserialize(deserializer)? {
                        $($name::$variant $({ $($field),+ })? => {
                            Self::$variant $({ $($field),+ })?
                        })+
                    })
                }
            }

            impl<'de> serde::Deserialize<'de> for $name {
                fn deserialize<D: serde::Deserializer<'de>>(
                    deserializer: D,
                ) -> Result<Self, D::Error> {
                    $crate::open_enum::deserialize_open(deserializer)
                }
            }
        };

        #[cfg(test)]
        mod $test {
            use super::*;

            #[test]
            fn samples_cover_every_row_and_round_trip() {
                $crate::open_enum::check::data_enum(
                    &[$($sample),+],
                    &[$(stringify!($variant)),+],
                    $name::variant_name,
                );
            }
        }
    };
}
pub(crate) use open_data_enum;

/// Writes an unknown variant's kind back as a plain string: the `serialize_with` that
/// [`open_data_enum!`] puts on its `Unknown` variant. `T`, the enum, comes from the kind's type,
/// so the check cannot name another enum. A kind `T` knows is an error: written, it would decode
/// as that variant, or fail to decode at all.
///
/// A decoded `Unknown` is written as its kind string alone, its payload gone, so it only goes
/// to readers of this version or older (the webview, re-sent copies of an event). It never goes
/// back to the newer peer that wrote it: that peer expects `{"kind": {..}}` for a data variant
/// and refuses the plain string. The log keeps the original entry.
pub(crate) fn serialize_kind<T: OpenDataEnum, S: Serializer>(
    kind: &UnknownKind<T>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let kind = kind.as_str();
    if T::kind_index(kind).is_some() {
        return Err(serde::ser::Error::custom(format_args!(
            "{}: Unknown under the known kind {kind:?}",
            T::EXPECTING
        )));
    }
    serializer.serialize_str(kind)
}

/// An enum with data whose unknown values decode to a catch-all. [`open_data_enum!`] implements
/// it; [`deserialize_open`] decodes with it.
pub(crate) trait OpenDataEnum: CatchAll {
    /// What the decoder expected, for error messages, such as `"enum DetachReason"`.
    const EXPECTING: &'static str;

    /// The position of `kind` among the rows, which is its variant index in the decoder, or
    /// `None` for a kind this version does not know (unit and data kinds are both known, so a
    /// known kind in the wrong form is an error, not the catch-all). One `match` on the string;
    /// the decoder then picks the variant by index without comparing the string again.
    fn kind_index(kind: &str) -> Option<u32>;

    /// Decodes a known kind as an externally tagged enum: from [`UnitKind`] (a plain string,
    /// which only a unit kind takes) or from [`KnownKind`] (a one-key map, which only a struct
    /// kind takes, its payload read from the map).
    fn known<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>;
}

/// The catch-all of an [`OpenDataEnum`]: what a kind this version does not know becomes.
pub(crate) trait CatchAll: Sized {
    /// The catch-all for a plain string this version does not know.
    fn unknown(kind: &str) -> Self;

    /// The catch-all for an unknown key. The default skips its payload with [`SkipSeed`], which
    /// keeps nothing and allocates nothing but counts depth, so the payload nests at most
    /// [`MAX_RAW_DEPTH`](crate::MAX_RAW_DEPTH) lists and maps in JSON and `MessagePack` alike
    /// (`IgnoredAny` has no limit of its own). Override it to keep the payload.
    fn unknown_data<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Self, A::Error> {
        map.next_value_seed(SkipSeed { depth: 0 })?;
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
        match T::kind_index(v) {
            Some(index) => T::known(UnitKind::<E>::new(index, v)),
            None => Ok(T::unknown(v)),
        }
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
        let Some(key) = map.next_key::<BorrowedStr<'de>>()? else {
            return Err(de::Error::invalid_length(0, &"a map with one key"));
        };
        let kind = key.as_str();
        let value = match T::kind_index(kind) {
            Some(index) => T::known(KnownKind::new(index, kind, &mut map))?,
            None => T::unknown_data(kind, &mut map)?,
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
pub(crate) mod check;
#[cfg(test)]
mod tests;
