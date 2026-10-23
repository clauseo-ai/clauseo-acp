//! Numbers on the wire and in the log. Every 64-bit integer is a checked newtype (`Seq`, `Cursor`,
//! [`Count`]) that stays at most [`MAX_SAFE`] (2^53 - 1), so TypeScript reads it as an exact
//! `number`; a larger value cannot be built and fails to decode. A producer that passes on a count
//! an agent reported caps it with [`Count::clamped`] first. Every float is a [`Finite`].
//!
//! A `Count` or `Finite` field added to a logged type after v1 is an `Option` or carries
//! `#[serde(default)]`, like any new field, so an entry written before it still decodes.
//! `tests/corpus.rs` guards this: it decodes frozen v1 entries of every kind and fails otherwise.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::Error;

/// The largest integer a JavaScript `number` holds exactly (2^53 - 1). Every 64-bit value in this
/// crate is at most this, so TypeScript reads it as a `number`.
pub const MAX_SAFE: u64 = (1 << 53) - 1;

fn check_range(value: u64, min: u64, kind: &'static str) -> Result<u64, Error> {
    if (min..=MAX_SAFE).contains(&value) {
        Ok(value)
    } else {
        Err(Error::OutOfRange {
            kind,
            value: i128::from(value),
            min: i128::from(min),
            max: i128::from(MAX_SAFE),
        })
    }
}

/// Defines a newtype over `u64` in the range the struct names (`$min..=MAX_SAFE`), a `number`
/// in TypeScript.
// specta: `u32` and `i32` render as TypeScript `number`; `f64` would render `number | null`.
macro_rules! safe_u64 {
    ($(#[doc = $doc:expr])* pub struct $name:ident($min:literal ..= MAX_SAFE);) => {
        $(#[doc = $doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "specta", derive(specta::Type), specta(type = u32))]
        pub struct $name(u64);

        impl $name {
            #[doc = concat!("Wraps `value`, which must be in ", stringify!($min), "..=[`MAX_SAFE`].")]
            pub fn new(value: u64) -> Result<Self, Error> {
                check_range(value, $min, stringify!($name)).map(Self)
            }

            /// The number inside.
            pub const fn get(self) -> u64 {
                self.0
            }

            /// The next value, or an error past [`MAX_SAFE`].
            pub fn next(self) -> Result<Self, Error> {
                check_range(self.0.saturating_add(1), $min, stringify!($name)).map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u64(self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

safe_u64! {
    /// An event's position in its stream: gapless, starting at 1, assigned by the host at commit.
    pub struct Seq(1..=MAX_SAFE);
}
safe_u64! {
    /// A view patch position in one stream, starting at 0 before the first patch.
    pub struct Cursor(0..=MAX_SAFE);
}
safe_u64! {
    /// A count, a size or a byte offset: tokens, bytes. Producers pass a count an agent reported
    /// through [`Count::clamped`].
    pub struct Count(0..=MAX_SAFE);
}

impl Seq {
    /// The first event of every stream.
    pub const FIRST: Self = Self(1);
}

impl Cursor {
    /// The position before any patch.
    pub const ZERO: Self = Self(0);
}

impl Count {
    /// Zero.
    pub const ZERO: Self = Self(0);

    /// `value`, or [`MAX_SAFE`] when it is larger: for counts an agent reports, which Clauseo
    /// passes on but does not trust.
    pub fn clamped(value: u64) -> Self {
        Self(value.min(MAX_SAFE))
    }
}

impl Default for Count {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Every `u32` is in range.
impl From<u32> for Count {
    fn from(value: u32) -> Self {
        Self(u64::from(value))
    }
}

/// Milliseconds since the Unix epoch, UTC. Display only: order by [`Seq`], never by time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = i32))]
pub struct Timestamp(i64);

impl Timestamp {
    /// The epoch itself, for placeholder values.
    pub const ZERO: Self = Self(0);

    /// Wraps `millis`, whose magnitude must be at most [`MAX_SAFE`].
    pub fn new(millis: i64) -> Result<Self, Error> {
        if millis.unsigned_abs() <= MAX_SAFE {
            Ok(Self(millis))
        } else {
            let max = i128::from(MAX_SAFE);
            Err(Error::OutOfRange {
                kind: "Timestamp",
                value: i128::from(millis),
                min: -max,
                max,
            })
        }
    }

    /// Milliseconds since the Unix epoch.
    pub const fn as_millis(self) -> i64 {
        self.0
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.0)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(i64::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// A finite `f64`: never NaN or an infinity. `serde_json` writes those as `null`, which no
/// reader decodes back into a number, so [`Finite::new`] and `Deserialize` refuse them and a
/// producer bug fails when the event is committed. Written as a plain number in every format.
///
/// Equality and order are [`f64::total_cmp`], which is total for every value this type holds
/// (so `Eq` and `Ord` hold). Like their JSON text, `-0.0` and `0.0` are different values.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(try_from = "f64")]
// specta: `i32` renders as TypeScript `number`; `f64` would render `number | null`.
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = i32))]
pub struct Finite(f64);

impl Finite {
    /// `value`, or `None` when it is NaN or an infinity.
    pub const fn new(value: f64) -> Option<Self> {
        if value.is_finite() {
            Some(Self(value))
        } else {
            None
        }
    }

    /// The number inside.
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl PartialEq for Finite {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0).is_eq()
    }
}

impl Eq for Finite {}

impl PartialOrd for Finite {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Finite {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl TryFrom<f64> for Finite {
    type Error = crate::Error;

    /// `value`; an error naming the rule when it is NaN or an infinity.
    fn try_from(value: f64) -> Result<Self, crate::Error> {
        Self::new(value).ok_or(crate::Error::Format {
            kind: "Finite",
            rule: "must be a finite number, not NaN or an infinity",
        })
    }
}

impl serde::Serialize for Finite {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}
