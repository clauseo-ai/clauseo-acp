//! How the host shows text that came from outside it (a config, an agent) in anything the lawyer
//! approves: one rule, in one place, and the UI draws the result as given.
//!
//! - Each character that breaks a line, is blank, invisible, private-use or unassigned, or turns
//!   text is written as its escape (`\n`, `\u{2028}`, `\u{3164}`), as is every combining mark
//!   past the third on one letter, and `\` as `\\`, so nothing can add a line, pad a part out of
//!   view, draw an icon, cover the line above or reorder the words, and two different texts never
//!   show the same.
//! - A part holding right-to-left text is drawn left to right (U+2066 … U+2069) with each
//!   right-to-left run isolated inside it (U+2067 … U+2069), so it reads in the order it runs.
//! - The quoting is the app's own notation, not a shell's: it reads back one way only, but is not
//!   meant to be pasted into a terminal.
//! - Nothing here cuts text.

use icu_properties::props::{BidiClass, DefaultIgnorableCodePoint, GeneralCategory};
use icu_properties::{CodePointMapData, CodePointSetData};

/// An argument list (a program and its arguments) as shown: each part in the app's own quoting
/// notation, joined by spaces; reads back to exactly its parts.
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

/// `part` in the app's own notation (shell-like, not a shell's): as it is, or in single quotes
/// when it is empty or holds a space or a quote; in double quotes, `"` escaped, when it holds a
/// single quote. Its invisible
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

/// The most combining marks drawn on one letter; the rest are written out.
const MAX_MARKS: usize = 3;

/// `text` with `\` written `\\`, and each character [`written_out`], and each combining mark
/// past [`MAX_MARKS`] in a row, as its escape (`\n`, `\u{202e}`, `\u{369}`): a program can't add
/// a line to the dialog, pad it with blanks, stack marks over the line above, or show its words in
/// another order, and an escape never reads like text the program holds.
pub(crate) fn visible(text: &str) -> String {
    let category = CodePointMapData::<GeneralCategory>::new();
    let mut marks = 0;
    text.chars()
        .map(|c| {
            let mark = matches!(
                category.get(c),
                GeneralCategory::NonspacingMark
                    | GeneralCategory::SpacingMark
                    | GeneralCategory::EnclosingMark
            );
            marks = if mark { marks + 1 } else { 0 };
            if c == '\\' {
                "\\\\".to_owned()
            } else if written_out(c) || marks > MAX_MARKS {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// Whether `c` is written as its escape: every control (Cc), format (Cf), private-use (Co),
/// unassigned (Cn), line (Zl) and paragraph (Zp) character, every space (Zs) but U+0020, every
/// `Default_Ignorable_Code_Point` (U+00AD, the Hangul fillers U+115F, U+1160, U+3164 and U+FFA0,
/// variation selectors, tags…), and the blank Braille pattern U+2800, which no property marks
/// blank. The tables are ICU4X's compiled data
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
            | GeneralCategory::PrivateUse
            | GeneralCategory::Unassigned
            | GeneralCategory::SpaceSeparator
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
    ) || CodePointSetData::new::<DefaultIgnorableCodePoint>().contains(c)
        || c == '\u{2800}'
}

/// `shown` as it is when it holds no right-to-left letter or Arabic digit; otherwise forced left
/// to right (U+2066 … U+2069), each run of right-to-left letters and Arabic digits, with the marks
/// on them, isolated right to left inside it (U+2067 … U+2069). So the words, digits and
/// punctuation read in the order they run, and nothing beside the part moves. A program's own
/// isolates are written out ([`visible`]), so these are only ever ours.
pub(crate) fn isolated(shown: String) -> String {
    let bidi = CodePointMapData::<BidiClass>::new();
    let right_to_left = |c: char| {
        matches!(
            bidi.get(c),
            BidiClass::RightToLeft | BidiClass::ArabicLetter | BidiClass::ArabicNumber
        )
    };
    if !shown.chars().any(right_to_left) {
        return shown;
    }
    let mut out = String::with_capacity(shown.len() + 8);
    out.push('\u{2066}');
    let mut in_run = false;
    for c in shown.chars() {
        let joins = right_to_left(c) || (in_run && bidi.get(c) == BidiClass::NonspacingMark);
        if joins && !in_run {
            out.push('\u{2067}');
        } else if !joins && in_run {
            out.push('\u{2069}');
        }
        in_run = joins;
        out.push(c);
    }
    if in_run {
        out.push('\u{2069}');
    }
    out.push('\u{2069}');
    out
}

#[cfg(test)]
#[path = "shown_tests.rs"]
mod tests;
