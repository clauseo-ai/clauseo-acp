//! The sealed trait [`Closed`] and the macros that declare every payload type with it.
//!
//! A type is `Closed` when every value it can hold is listed in the schema: one of this crate's
//! enums, a capped integer, a fixed-format date, week or version, an id, or a struct or list of
//! those. `String`, `&str`, `PathBuf`, `Vec<u8>` and the bare integers are not, and only this crate
//! can implement the trait, so no field of a payload can carry free text. Each payload struct is
//! checked by [`closed_fields!`], which fails to compile unless every field is `Closed`
//! (`tests/closed.rs` shows a `String` field refused).

/// A type whose every value the schema lists, so it may be a field of a payload. Sealed: only
/// this crate implements it.
pub trait Closed: sealed::Sealed {}

pub(crate) mod sealed {
    /// The seal on [`super::Closed`]: this module is private, so no other crate can name it.
    pub trait Sealed {}
}

/// Compiles only when `T` is [`Closed`].
#[doc(hidden)]
pub const fn require_closed<T: Closed + ?Sized>() {}

impl sealed::Sealed for bool {}
/// Two values, each named in the schema (`managed`, `after_launch`, `crash_reports`).
impl Closed for bool {}

impl<T: Closed> sealed::Sealed for Option<T> {}
/// A field that may be left out (`matter_opened.place` until stage D's `matters::place`).
impl<T: Closed> Closed for Option<T> {}

impl<T: Closed> sealed::Sealed for Vec<T> {}
/// A list of closed values: the cells of one event. `Vec<u8>` is not, since `u8` is not.
impl<T: Closed> Closed for Vec<T> {}

/// Fails to compile unless every field of the struct `$name` is [`Closed`]. The fields are
/// named, not typed: the check destructures a value of the struct with no `..`, so a field left
/// out of the list fails too, and each field's real type is the one checked. Every payload
/// struct of this crate is checked through it (by way of `seal!`). Exported only so the
/// compile-fail test can show a `String` field refused; another crate's struct checked with it
/// is still not `Closed`, since only this crate implements the trait.
#[doc(hidden)]
#[macro_export]
macro_rules! closed_fields {
    ($name:ident { $($field:ident),* $(,)? }) => {
        const _: fn(&$name) = |value| {
            let $name { $($field),* } = value;
            $($crate::schema::require_closed_value($field);)*
        };
    };
}

/// Compiles only when `T` is [`Closed`]; [`closed_fields!`] calls it once per field.
#[doc(hidden)]
pub const fn require_closed_value<T: Closed + ?Sized>(_: &T) {}

/// Seals a payload struct: checks its fields with [`closed_fields!`], implements [`Closed`],
/// and (in tests) draws arbitrary values of it. The struct itself is written out above the call,
/// with `#[serde(deny_unknown_fields)]`.
macro_rules! seal {
    ($name:ident { $($field:ident),* $(,)? }) => {
        $crate::closed_fields!($name { $($field),* });

        impl $crate::schema::closed::sealed::Sealed for $name {}
        impl $crate::schema::Closed for $name {}

        #[cfg(test)]
        impl $crate::schema::tests::sample::Sample for $name {
            fn sample(tape: &mut $crate::schema::tests::sample::Tape) -> Self {
                Self {
                    $($field: $crate::schema::tests::sample::Sample::sample(tape)),*
                }
            }
        }
    };
}

/// A closed enum of fixed names: serialised as its name, with `ALL` and `as_str`, and sealed.
/// Docs are string literals (the enum's, then each variant's before it), since a macro may not
/// pass attributes through (expect-allowlist).
macro_rules! closed_enum {
    (
        $($doc:literal)*
        pub enum $name:ident {
            $($($variant_doc:literal)* $variant:ident = $wire:literal,)+
        }
    ) => {
        $(#[doc = $doc])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            serde::Serialize, serde::Deserialize,
        )]
        #[cfg_attr(test, derive(schemars::JsonSchema))]
        pub enum $name {
            $($(#[doc = $variant_doc])* #[serde(rename = $wire)] $variant,)+
        }

        impl $name {
            /// Every value, in the schema's order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The name on the wire.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire,)+
                }
            }
        }

        impl $crate::schema::closed::sealed::Sealed for $name {}
        impl $crate::schema::Closed for $name {}

        #[cfg(test)]
        impl $crate::schema::tests::sample::Sample for $name {
            fn sample(tape: &mut $crate::schema::tests::sample::Tape) -> Self {
                tape.pick(Self::ALL)
            }
        }
    };
}

pub(crate) use {closed_enum, seal};
