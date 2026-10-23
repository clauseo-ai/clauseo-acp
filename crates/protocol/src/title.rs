//! Titles a person reads: the rules for showing one, and [`Title`], a title the lawyer typed.

use std::fmt;
use std::str::FromStr;

use compact_str::CompactString;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::Error;

/// Most characters (`char`s, Unicode scalar values) in a title or a matter name.
pub const MAX_TITLE_CHARS: usize = 200;

/// Checks that `title` is safe to show as one line of a title.
///
/// - At most [`MAX_TITLE_CHARS`] characters.
/// - At least one character that shows: not every character is invisible or a combining mark
///   (see below).
/// - No control character (`char::is_control`: C0, DEL and C1), no bidi embedding, override or
///   isolate (U+202A to U+202E, U+2066 to U+2069), which can make a title read as something
///   else, and no line or paragraph separator (U+2028, U+2029). The bidi marks U+200E, U+200F
///   and U+061C are allowed next to visible text.
///
/// "Shows nothing" is a heuristic, not a font's answer: whitespace, Unicode's
/// `Default_Ignorable_Code_Point` characters (zero-width characters, bidi controls and marks,
/// variation selectors, the soft hyphen, the Hangul fillers U+115F, U+1160, U+3164 and U+FFA0,
/// U+034F, U+180E, the tag characters U+E0000 to U+E0FFF), and U+2800 (the blank Braille
/// pattern) count as invisible. A combining mark shows only on a character before it; these
/// count, and no other nonspacing mark does: the combining blocks U+0300 to U+036F, U+1AB0 to
/// U+1AFF, U+1DC0 to U+1DFF, U+20D0 to U+20FF and U+FE20 to U+FE2F, and the marks people meet
/// most in other scripts: Cyrillic U+0483 to U+0489; Hebrew U+0591 to U+05BD, U+05BF, U+05C1,
/// U+05C2, U+05C4, U+05C5, U+05C7; Arabic U+0610 to U+061A, U+064B to U+065F, U+0670, U+06D6
/// to U+06DC, U+06DF to U+06E4, U+06E7, U+06E8, U+06EA to U+06ED; Thai U+0E31, U+0E34 to
/// U+0E3A, U+0E47 to U+0E4E.
///
/// [`Title`] applies it to what the lawyer types. Agent text (`session_info_update` titles,
/// tool titles) may be checked with it before it is shown, but an event never refuses an
/// agent's title on decode, and the UI shows every title bidi-isolated.
///
/// # Errors
///
/// [`Error::TooLarge`] for a title over [`MAX_TITLE_CHARS`], which is checked first, so a long
/// title of spaces is too large rather than invisible; [`Error::Format`] for the other rules;
/// `kind` is `"Title"`.
pub fn check_display_title(title: &str) -> Result<(), Error> {
    let chars = title.chars().count();
    if chars > MAX_TITLE_CHARS {
        return Err(Error::TooLarge {
            kind: "Title",
            what: "title in characters",
            size: chars,
            max: MAX_TITLE_CHARS,
        });
    }
    if !shows_something(title) {
        return Err(Error::Format {
            kind: "Title",
            rule: "a title has a visible character",
        });
    }
    if title
        .chars()
        .any(|c| c.is_control() || is_refused_in_title(c))
    {
        return Err(Error::Format {
            kind: "Title",
            rule: "a title holds no control, bidi control or line separator character",
        });
    }
    Ok(())
}

/// Whether `text` holds a character that shows, as [`check_display_title`] counts it.
pub(crate) fn shows_something(text: &str) -> bool {
    !text
        .chars()
        .all(|c| is_invisible(c) || is_combining_mark(c))
}

fn is_refused_in_title(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}'
    )
}

/// Whitespace, `Default_Ignorable_Code_Point` (Unicode 16's derived property, written out here)
/// and U+2800.
fn is_invisible(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{115F}'..='\u{1160}'
                | '\u{17B4}'..='\u{17B5}'
                | '\u{180B}'..='\u{180F}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{2800}'
                | '\u{3164}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF0}'..='\u{FFF8}'
                | '\u{1BCA0}'..='\u{1BCA3}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0000}'..='\u{E0FFF}'
        )
}

/// The combining marks [`check_display_title`] lists.
fn is_combining_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{0483}'..='\u{0489}'
            | '\u{0591}'..='\u{05BD}'
            | '\u{05BF}'
            | '\u{05C1}'..='\u{05C2}'
            | '\u{05C4}'..='\u{05C5}'
            | '\u{05C7}'
            | '\u{0610}'..='\u{061A}'
            | '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{06D6}'..='\u{06DC}'
            | '\u{06DF}'..='\u{06E4}'
            | '\u{06E7}'..='\u{06E8}'
            | '\u{06EA}'..='\u{06ED}'
            | '\u{0E31}'
            | '\u{0E34}'..='\u{0E3A}'
            | '\u{0E47}'..='\u{0E4E}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}

/// A title the lawyer typed (a thread's new title, a matter's name). [`Title::new`] and
/// `Deserialize` apply [`check_display_title`], so a command never holds one that breaks it.
/// Serialised as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
pub struct Title(CompactString);

impl Title {
    /// Checks `value` with [`check_display_title`] and wraps it.
    ///
    /// # Errors
    ///
    /// As [`check_display_title`].
    pub fn new(value: &str) -> Result<Self, Error> {
        check_display_title(value)?;
        Ok(Self(CompactString::from(value)))
    }

    /// The text inside.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Title {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Title {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        Self::new(text)
    }
}

impl Serialize for Title {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Title {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = CompactString::deserialize(deserializer)?;
        check_display_title(&value).map_err(de::Error::custom)?;
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests;
