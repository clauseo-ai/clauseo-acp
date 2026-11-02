//! The capped integers: each refuses a value outside its range on the way in, so no exact size,
//! time or other unbounded figure can travel.

use std::fmt;

/// A value outside a capped integer's range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfRange;

impl fmt::Display for OutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a number outside its range")
    }
}

impl std::error::Error for OutOfRange {}

/// An integer newtype that holds only `MIN..=MAX`, serialised as a JSON number.
macro_rules! capped_int {
    (
        $($doc:literal)*
        pub struct $name:ident($int:ty), $min:literal ..= $max:literal;
    ) => {
        $(#[doc = $doc])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name($int);

        impl $name {
            /// The smallest value.
            pub const MIN: $int = $min;
            /// The largest value.
            pub const MAX: $int = $max;

            /// The value, or `None` outside `MIN..=MAX`.
            pub const fn new(value: $int) -> Option<Self> {
                if value >= Self::MIN && value <= Self::MAX {
                    Some(Self(value))
                } else {
                    None
                }
            }

            /// The value.
            pub const fn get(self) -> $int {
                self.0
            }
        }

        impl TryFrom<u64> for $name {
            type Error = OutOfRange;

            fn try_from(value: u64) -> Result<Self, OutOfRange> {
                <$int>::try_from(value).ok().and_then(Self::new).ok_or(OutOfRange)
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> u64 {
                u64::from(value.0)
            }
        }

        impl super::closed::sealed::Sealed for $name {}
        impl super::Closed for $name {}

        #[cfg(test)]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "integer",
                    "minimum": $min,
                    "maximum": $max,
                })
            }
        }

        #[cfg(test)]
        impl super::tests::sample::Sample for $name {
            fn sample(tape: &mut super::tests::sample::Tape) -> Self {
                let span = u64::from(Self::MAX - Self::MIN) + 1;
                let offset = <$int>::try_from(tape.below(span)).unwrap();
                Self(Self::MIN + offset)
            }
        }
    };
}

capped_int! {
    "How many times a cell's combination happened in one day: 1 to 9,999. A cell stops at"
    "9,999 (research 37 §2.2), and a zero cell is never sent."
    pub struct Count(u16), 1..=9999;
}

impl Count {
    /// One.
    pub const ONE: Count = Count(1);

    /// One more, stopping at [`Count::MAX`].
    #[must_use]
    pub const fn plus_one(self) -> Count {
        self.plus(Count::ONE)
    }

    /// The sum, stopping at [`Count::MAX`].
    #[must_use]
    pub const fn plus(self, other: Count) -> Count {
        let sum = self.0.saturating_add(other.0);
        Count(if sum > Count::MAX { Count::MAX } else { sum })
    }
}

capped_int! {
    "`agent_install.tries`: 1 to 3."
    pub struct Tries(u8), 1..=3;
}

capped_int! {
    "`setting_changed`'s text size, in points: 14 to 20, as `ui.font_size`."
    pub struct TextSize(u8), 14..=20;
}

capped_int! {
    "The OS's major version: macOS 13 to 26, Windows 10 or 11 (none on Linux). Which pairs are"
    "allowed is [`super::Platform`]'s rule."
    pub struct OsMajor(u8), 10..=26;
}

capped_int! {
    "The schema's number, `1`. Any other value is refused."
    pub struct SchemaVersion(u8), 1..=1;
}

impl SchemaVersion {
    /// Schema 1, the only one.
    pub const V1: SchemaVersion = SchemaVersion(1);
}

capped_int! {
    "The count of an `active_week` cell, always 1."
    pub struct One(u8), 1..=1;
}

impl One {
    /// One.
    pub const ONE: One = One(1);
}
