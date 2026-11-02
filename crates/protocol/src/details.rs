//! A matter's details for the rail: the client's name, the next date, the change an
//! `UpdateMatter` makes to an optional field, and the matter's status over its threads.

use std::fmt;
use std::str::FromStr;

use compact_str::CompactString;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::open_enum::enum_with_names;
use crate::{Error, Timestamp, check_display_title};

/// The client a matter is for, as the lawyer typed it: trimmed of surrounding whitespace, then
/// held to the title rules ([`check_display_title`]): 1 to
/// [`MAX_TITLE_CHARS`](crate::MAX_TITLE_CHARS) characters, at least one that shows, no control,
/// bidi control or line separator. [`ClientName::new`] and `Deserialize` both trim and check, so
/// `"  Acme  "` decodes as `"Acme"` and `"   "` is refused. Serialised as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
pub struct ClientName(CompactString);

impl ClientName {
    /// Trims `value` and checks it with [`check_display_title`].
    ///
    /// # Errors
    ///
    /// As [`check_display_title`], on the trimmed text.
    pub fn new(value: &str) -> Result<Self, Error> {
        let trimmed = value.trim();
        check_display_title(trimmed)?;
        Ok(Self(CompactString::from(trimmed)))
    }

    /// The text inside.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ClientName {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        Self::new(text)
    }
}

impl Serialize for ClientName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ClientName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = CompactString::deserialize(deserializer)?;
        Self::new(&value).map_err(de::Error::custom)
    }
}

/// A calendar date with no time or zone, such as a matter's next hearing: ISO 8601's
/// `YYYY-MM-DD`, years 0001 to 9999, a day the month has (29 February only in a leap year of
/// the proleptic Gregorian calendar). [`Date::new`], `FromStr` and `Deserialize` check the same
/// rules; the text form is exactly ten characters. Ordered by date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

/// The rule a date's text breaks, as one sentence.
const DATE_RULE: &str = "a date is YYYY-MM-DD, a day that exists, from year 0001 to 9999";

impl Date {
    /// The date `year`-`month`-`day`.
    ///
    /// # Errors
    ///
    /// [`Error::Format`] (`kind` `"Date"`) for a year outside 1..=9999, a month outside 1..=12
    /// or a day the month does not have.
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, Error> {
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => 0,
        };
        if (1..=9999).contains(&year) && (1..=days).contains(&day) {
            Ok(Self { year, month, day })
        } else {
            Err(Error::Format {
                kind: "Date",
                rule: DATE_RULE,
            })
        }
    }

    /// The year, 1 to 9999.
    pub const fn year(self) -> u16 {
        self.year
    }

    /// The month, 1 to 12.
    pub const fn month(self) -> u8 {
        self.month
    }

    /// The day of the month, from 1.
    pub const fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for Date {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        let bad = || Error::Format {
            kind: "Date",
            rule: DATE_RULE,
        };
        let bytes = text.as_bytes();
        let digits = |range: std::ops::Range<usize>| -> Result<u16, Error> {
            let part = bytes.get(range).ok_or_else(bad)?;
            part.iter().try_fold(0_u16, |acc, &byte| {
                if byte.is_ascii_digit() {
                    Ok(acc * 10 + u16::from(byte - b'0'))
                } else {
                    Err(bad())
                }
            })
        };
        if bytes.len() != 10 || bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
            return Err(bad());
        }
        let year = digits(0..4)?;
        let month = u8::try_from(digits(5..7)?).map_err(|_too_big| bad())?;
        let day = u8::try_from(digits(8..10)?).map_err(|_too_big| bad())?;
        Self::new(year, month, day)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = CompactString::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

/// What an `UpdateMatter` does to one optional detail. JSON: a missing field keeps it, `null`
/// clears it, a value sets it. The field carries `#[serde(default, skip_serializing_if =
/// "FieldChange::is_keep")]`, so `Keep` is written as a missing field and reads back as one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FieldChange<T> {
    /// Leave it as it is: the field is missing.
    #[default]
    Keep,
    /// Clear it: the field is `null`.
    Clear,
    /// Set it to this value.
    Set(T),
}

impl<T> FieldChange<T> {
    /// Whether this keeps the detail, for `skip_serializing_if`.
    pub const fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}

impl<T: Serialize> Serialize for FieldChange<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // Never written in a command (`skip_serializing_if`); a bare value reads as null.
            Self::Keep | Self::Clear => serializer.serialize_none(),
            Self::Set(value) => serializer.serialize_some(value),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for FieldChange<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            None => Self::Clear,
            Some(value) => Self::Set(value),
        })
    }
}

enum_with_names! {
    /// What a matter is doing, over all its threads, for its row in the rail. Computed by the
    /// host from the threads each time it is asked; never logged or stored, so a new variant is
    /// additive (0002's freeze rules: the desktop and the cli ship with the host).
    /// `docs/decisions/0022-matters-for-the-rail.md` gives the precedence when threads differ.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "specta", derive(specta::Type))]
    pub enum MatterStatus {
        /// Nothing runs and nothing waits to be read.
        Idle,
        /// A thread's turn is starting, running or waiting for the matter's writer.
        Working,
        /// A thread waits for the lawyer's answer to a permission request.
        NeedsPermission,
        /// A thread waits for the lawyer's answer to a question (an elicitation).
        NeedsAnswer,
        /// A thread's last turn finished (`end_turn`) after the matter was last opened.
        Ready,
        /// A thread's last turn stopped short (cancelled, failed, cut off) after the matter was
        /// last opened.
        Stopped {
            /// When that turn ended.
            at: Timestamp,
        },
        /// A thread stopped at the account's usage limit.
        PausedAtLimit {
            /// The earliest reset among the paused threads, if any said when.
            #[serde(skip_serializing_if = "Option::is_none", default)]
            resets_at: Option<Timestamp>,
        },
    }
}

impl MatterStatus {
    /// `Idle`, what a missing status reads as (`#[serde(default = "MatterStatus::idle")]`).
    pub const fn idle() -> Self {
        Self::Idle
    }
}

#[cfg(test)]
mod tests;
