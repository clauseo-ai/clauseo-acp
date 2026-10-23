//! `settings.json` and `policy.json`: JSON with comments and trailing commas, read section by
//! section, with each value's line and column.
//!
//! Each section's values are found as raw slices of the file (`RawValue`), so a warning about a
//! value can say where it is. `serde_json_lenient` 0.2.4 skips a raw value without honouring its
//! trailing commas (`Deserializer::ignore_value`), so each trailing comma is first blanked to a
//! space, which keeps every byte where it was.

#[cfg(test)]
mod tests;

use crate::leaf::Raw;
use crate::schema::SECTIONS;
use crate::warning::{Origin, Position, Warning, WarningKind};
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json_lenient::value::RawValue;
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;

/// The UTF-8 byte order mark some Windows editors write first.
pub(crate) const BOM: &[u8] = b"\xEF\xBB\xBF";

/// The file is not JSON with comments, or not one object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Syntax {
    pub position: Position,
    pub message: String,
}

/// The values of a file: one per key of a known section, in file order, with a warning for each
/// unknown section and each section that is not an object.
pub(crate) fn read(path: &Path, bytes: &[u8]) -> Result<(Vec<Raw>, Vec<Warning>), Syntax> {
    let text = blank_trailing_commas(bytes.strip_prefix(BOM).unwrap_or(bytes));
    let text = text.as_ref();
    let newlines: Vec<usize> = text
        .iter()
        .enumerate()
        .filter_map(|(at, &byte)| (byte == b'\n').then_some(at))
        .collect();
    let origin = |at: &RawValue| Origin::File {
        path: path.to_owned(),
        position: Some(position(text, &newlines, at)),
    };
    let sections: Entries<'_> =
        serde_json_lenient::from_slice(text).map_err(|error| syntax(&error))?;
    let mut raws = Vec::new();
    let mut warnings = Vec::new();
    for (section, value) in sections.0 {
        if !SECTIONS.contains(&section.as_str()) {
            warnings.push(Warning {
                origin: origin(value),
                kind: WarningKind::UnknownKey { key: section },
            });
            continue;
        }
        let Ok(entries) = serde_json_lenient::from_str::<Entries<'_>>(value.get()) else {
            warnings.push(Warning {
                origin: origin(value),
                kind: WarningKind::InvalidValue {
                    key: section,
                    message: "a section must be an object".to_owned(),
                },
            });
            continue;
        };
        for (name, value) in entries.0 {
            raws.push(Raw {
                key: format!("{section}.{name}"),
                // The file already parsed, so this cannot fail; if it did, the value would be
                // `null` and refused when typed.
                value: serde_json_lenient::from_str(value.get()).unwrap_or_default(),
                origin: origin(value),
            });
        }
    }
    Ok((raws, warnings))
}

/// Where [`blank_trailing_commas`] is in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scan {
    /// JSON outside strings and comments.
    Code,
    /// After a `/` in code: a comment starts if `/` or `*` follows.
    Slash,
    LineComment,
    BlockComment,
    /// After a `*` in a block comment: it ends if `/` follows.
    BlockStar,
    String,
    /// After a `\` in a string.
    Escape,
}

/// `text` with each trailing comma (one that only whitespace and comments separate from a `}`
/// or `]`) replaced by a space. Commas inside strings and comments stay.
fn blank_trailing_commas(text: &[u8]) -> Cow<'_, [u8]> {
    // The last comma in code with only whitespace and comments after it so far.
    let mut comma = None;
    let mut trailing = Vec::new();
    let mut scan = Scan::Code;
    for (at, &byte) in text.iter().enumerate() {
        scan = match (scan, byte) {
            (Scan::String, b'"') | (Scan::LineComment, b'\n') | (Scan::BlockStar, b'/') => {
                Scan::Code
            }
            (Scan::String, b'\\') => Scan::Escape,
            (Scan::String | Scan::Escape, _) => Scan::String,
            (Scan::LineComment, _) | (Scan::Slash, b'/') => Scan::LineComment,
            (Scan::BlockComment | Scan::BlockStar, b'*') => Scan::BlockStar,
            (Scan::BlockComment | Scan::BlockStar, _) | (Scan::Slash, b'*') => Scan::BlockComment,
            // A `/` that starts no comment is code, so no comma before it is trailing.
            (Scan::Slash, _) => {
                comma = None;
                code(at, byte, &mut comma, &mut trailing)
            }
            (Scan::Code, _) => code(at, byte, &mut comma, &mut trailing),
        };
    }
    if trailing.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut blanked = text.to_vec();
    for at in trailing {
        if let Some(comma) = blanked.get_mut(at) {
            *comma = b' ';
        }
    }
    Cow::Owned(blanked)
}

/// One byte of code: notes a comma, marks it trailing at a `}` or `]`, and forgets it at
/// anything else but whitespace or the start of a comment.
fn code(at: usize, byte: u8, comma: &mut Option<usize>, trailing: &mut Vec<usize>) -> Scan {
    match byte {
        b'/' => return Scan::Slash,
        b',' => *comma = Some(at),
        b'}' | b']' => trailing.extend(comma.take()),
        _ if byte.is_ascii_whitespace() => {}
        _ => *comma = None,
    }
    if byte == b'"' {
        Scan::String
    } else {
        Scan::Code
    }
}

/// The line and column where `value`, a slice of `text`, starts.
fn position(text: &[u8], newlines: &[usize], value: &RawValue) -> Position {
    let offset = value
        .get()
        .as_ptr()
        .addr()
        .saturating_sub(text.as_ptr().addr());
    // A value never starts on a newline, so the search never finds `offset` itself; either way
    // the index counts the newlines before it.
    let (Ok(line) | Err(line)) = newlines.binary_search(&offset);
    let line_start = line
        .checked_sub(1)
        .and_then(|before| newlines.get(before))
        .map_or(0, |&at| at + 1);
    Position {
        line: line + 1,
        column: offset - line_start + 1,
    }
}

/// A parse error, without the " at line L column C" the parser appends to its message.
pub(crate) fn syntax(error: &serde_json_lenient::Error) -> Syntax {
    let (line, column) = (error.line(), error.column());
    let message = error.to_string();
    let suffix = format!(" at line {line} column {column}");
    Syntax {
        position: Position { line, column },
        message: message.strip_suffix(&suffix).unwrap_or(&message).to_owned(),
    }
}

/// A JSON object's entries in file order, each value left as its slice of the file.
struct Entries<'a>(Vec<(String, &'a RawValue)>);

impl<'de: 'a, 'a> Deserialize<'de> for Entries<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(EntriesVisitor(PhantomData))
    }
}

struct EntriesVisitor<'a>(PhantomData<&'a ()>);

impl<'de: 'a, 'a> Visitor<'de> for EntriesVisitor<'a> {
    type Value = Entries<'a>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entries<'a>, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = map.next_entry::<String, &'a RawValue>()? {
            entries.push(entry);
        }
        Ok(Entries(entries))
    }
}
