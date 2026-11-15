//! How the host shows text that came from outside it (a config, an agent) in anything the lawyer
//! approves: one rule, in one place, and the UI draws the result as given.
//!
//! - Each character that breaks a line, is blank or invisible, or turns text is written as its
//!   escape (`\n`, `\u{2028}`, `\u{3164}`), and `\` as `\\`, so nothing can add a line, pad a
//!   part out of view or reorder the words, and two different texts never show the same.
//! - A part holding right-to-left text sits between isolates (U+2068 … U+2069), so it can't
//!   reorder the digits and punctuation around it.
//! - Nothing here cuts text.

use icu_properties::props::{BidiClass, DefaultIgnorableCodePoint, GeneralCategory};
use icu_properties::{CodePointMapData, CodePointSetData};

/// An argument list (a program and its arguments) as shown: each part quoted as a shell reads it
/// back, joined by spaces; reads back to exactly its parts.
#[must_use]
pub fn argv(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| quoted(part))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Free text (a card's title, a question) as shown: whole, on one line, nothing invisible kept.
#[must_use]
pub fn text(text: &str) -> String {
    isolated(visible(text))
}

/// `part` as a shell reads it back: as it is, or in single quotes when it is empty or holds a
/// space or a quote; in double quotes, `"` escaped, when it holds a single quote. Its invisible
/// characters and backslashes are written out first ([`visible`]), so a `\` before `"` is always
/// that quote's escape and the text reads back one way only.
fn quoted(part: &str) -> String {
    let part = visible(part);
    let plain = !part.is_empty()
        && !part
            .chars()
            .any(|c| c.is_whitespace() || c == '\'' || c == '"');
    let shown = if plain {
        part
    } else if !part.contains('\'') {
        format!("'{part}'")
    } else {
        format!("\"{}\"", part.replace('"', "\\\""))
    };
    isolated(shown)
}

/// `text` with `\` written `\\`, and each character [`written_out`] as its escape (`\n`,
/// `\u{202e}`): a program can't add a line to the dialog, pad it with blanks, or show its words in
/// another order, and an escape never reads like text the program holds.
pub(crate) fn visible(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\\' {
                "\\\\".to_owned()
            } else if written_out(c) {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// Whether `c` is written as its escape: every control (Cc), format (Cf), line (Zl) and paragraph
/// (Zp) character, every space (Zs) but U+0020, every `Default_Ignorable_Code_Point` (U+00AD, the
/// Hangul fillers U+115F, U+1160, U+3164 and U+FFA0, variation selectors, tags…), and the blank
/// Braille pattern U+2800, which no property marks blank. The tables are ICU4X's compiled data
/// (`icu_properties` 2.3, Unicode 17).
fn written_out(c: char) -> bool {
    if c == ' ' {
        return false;
    }
    let category = CodePointMapData::<GeneralCategory>::new().get(c);
    matches!(
        category,
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::SpaceSeparator
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
    ) || CodePointSetData::new::<DefaultIgnorableCodePoint>().contains(c)
        || c == '\u{2800}'
}

/// `shown` between a first-strong isolate and its pop (U+2068, U+2069) when it holds right-to-left
/// letters or Arabic digits, which would otherwise reorder the text beside it; as it is otherwise.
/// A program's own U+2068 or U+2069 is written out ([`visible`]), so these are only ever ours.
pub(crate) fn isolated(shown: String) -> String {
    let bidi = CodePointMapData::<BidiClass>::new();
    let turns = shown.chars().any(|c| {
        matches!(
            bidi.get(c),
            BidiClass::RightToLeft | BidiClass::ArabicLetter | BidiClass::ArabicNumber
        )
    });
    if turns {
        format!("\u{2068}{shown}\u{2069}")
    } else {
        shown
    }
}

#[cfg(test)]
#[path = "shown_tests.rs"]
mod tests;
