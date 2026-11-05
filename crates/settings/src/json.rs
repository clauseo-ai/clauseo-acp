//! `settings.json` and `policy.json`: JSON with comments and trailing commas, read section by
//! section, with each value's line and column.
//!
//! Each section's values are found as raw slices of the file (`RawValue`), so a warning about a
//! value can say where it is. `serde_json_lenient` 0.2.4 skips a raw value without honouring its
//! trailing commas (`Deserializer::ignore_value`), so each trailing comma is first blanked to a
//! space, which keeps every byte where it was.

#[cfg(test)]
mod tests;

use crate::leaf::{self, Places, Raw};
use crate::schema::SECTIONS;
use crate::warning::{Origin, Position, Warning, WarningKind};
use serde::de::{Deserialize, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json_lenient::value::RawValue;
use std::borrow::Cow;
use std::collections::HashSet;
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

/// What [`read`] found: the values, the warnings, and the dotted name of every key nested
/// inside an unknown section that is an object (`clauseo.skills`, `clauseo.skills.extra`, each
/// a [`Raw`] with a `null` value at its section's place), so a policy's misnamed-key pass can
/// see a security section wrapped in an unknown one, through arrays too
/// (`{"clauseo": [{"skills": {}}]}` names `clauseo.skills`). `Err` when an unknown section's
/// value cannot be read whole (a number too big for an `f64`, nesting past the parser's
/// limit), so its names are unknown: a policy is then malformed. `settings.json`'s reader
/// ignores them.
/// Last, each name's place in the file ([`Places`]): its top-level entry's, from 1.
pub(crate) type Read = (Vec<Raw>, Vec<Warning>, Result<Vec<Raw>, Syntax>, Places);

/// The values of a file: one per key of a known section, in file order, with a warning for each
/// unknown section and each section that is not an object.
pub(crate) fn read(path: &Path, bytes: &[u8]) -> Result<Read, Syntax> {
    let text = bytes.strip_prefix(BOM).unwrap_or(bytes);
    if is_blank(text) {
        return Ok((Vec::new(), Vec::new(), Ok(Vec::new()), Places::default()));
    }
    let text = blank_trailing_commas(text);
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
    // A section the file sets twice is its last copy, whole: the save keeps only that copy, so
    // the earlier copy's entries must not load either. Its warnings go with it.
    let mut wrapped = Ok(Vec::new());
    let mut plans_seen = 0;
    let mut places = Places::default();
    let numbered = sections.0.into_iter().enumerate().collect();
    for (at, (section, value)) in leaf::keep_last(numbered, |(_, (section, _))| section.clone()) {
        let place = at + 1;
        plans_seen += usize::from(section.starts_with("plans."));
        if !SECTIONS.contains(&section.as_str()) {
            places.warnings.push(place);
            // A top-level `plans.` name can be pasted text: shown by its place.
            warnings.push(Warning {
                origin: origin(value),
                kind: WarningKind::UnknownKey {
                    key: crate::plans::shown_plan_key(&section, plans_seen),
                },
            });
            match (
                serde_json_lenient::from_str::<serde_json::Value>(value.get()),
                &mut wrapped,
            ) {
                (Ok(nested), Ok(names)) => {
                    nested_names(&section, &nested, &origin(value), names);
                    places.wrapped.resize(names.len(), place);
                }
                (Err(error), Ok(_)) => {
                    wrapped = Err(Syntax {
                        position: position(text, &newlines, value),
                        message: syntax(&error).message,
                    });
                }
                (_, Err(_)) => {}
            }
            continue;
        }
        let Ok(entries) = serde_json_lenient::from_str::<Entries<'_>>(value.get()) else {
            places.warnings.push(place);
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
            places.raws.push(place);
            raws.push(Raw {
                key: format!("{section}.{name}"),
                // The file already parsed, so this cannot fail; if it did, the value would be
                // `null` and refused when typed.
                value: serde_json_lenient::from_str(value.get()).unwrap_or_default(),
                origin: origin(value),
            });
        }
    }
    Ok((raws, warnings, wrapped, places))
}

/// Pushes `prefix.<key>` for each key of `value`, when it is an object, then the same for each
/// value under it, at every depth; an array's items are walked under `prefix` itself.
fn nested_names(prefix: &str, value: &serde_json::Value, origin: &Origin, names: &mut Vec<Raw>) {
    let entries = match value {
        serde_json::Value::Object(entries) => entries,
        serde_json::Value::Array(items) => {
            for item in items {
                nested_names(prefix, item, origin, names);
            }
            return;
        }
        _ => return,
    };
    for (key, value) in entries {
        let name = format!("{prefix}.{key}");
        nested_names(&name, value, origin, names);
        names.push(Raw {
            key: name,
            value: serde_json::Value::Null,
            origin: origin.clone(),
        });
    }
}

/// The message [`NoDuplicates`] fails with, so [`duplicate`] tells it from a syntax error.
const DUPLICATE: &str = "duplicate key";

/// Where a file holds a key twice in one object, at any level: `Ok(Some)` with the place just
/// after the second copy's name, or `Ok(None)` when every object's keys differ. `serde_json`
/// keeps the last copy silently, so this walks the file itself. Never the key: only its place.
///
/// `Err` when the walk cannot finish though [`read`] may have: a number too big for an `f64`
/// (`1e400`), or nesting deeper than the parser's limit (128), which `read` skips over as a
/// raw value. A duplicate after it would go unseen, so a policy reads such a file as malformed
/// (host's review of #450); `settings.json` only loses the duplicate warning.
pub(crate) fn duplicate(bytes: &[u8]) -> Result<Option<Position>, Syntax> {
    let text = bytes.strip_prefix(BOM).unwrap_or(bytes);
    if is_blank(text) {
        return Ok(None);
    }
    let text = blank_trailing_commas(text);
    match serde_json_lenient::from_slice::<NoDuplicates>(&text) {
        Err(error) => {
            let found = syntax(&error);
            if found.message == DUPLICATE {
                Ok(Some(found.position))
            } else {
                Err(found)
            }
        }
        Ok(NoDuplicates) => Ok(None),
    }
}

/// Any JSON value whose objects, at every level, hold each key once.
struct NoDuplicates;

impl<'de> Deserialize<'de> for NoDuplicates {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NoDuplicatesVisitor)
    }
}

struct NoDuplicatesVisitor;

impl<'de> Visitor<'de> for NoDuplicatesVisitor {
    type Value = NoDuplicates;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<NoDuplicates, A::Error> {
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            if !seen.insert(key) {
                return Err(A::Error::custom(DUPLICATE));
            }
            map.next_value::<NoDuplicates>()?;
        }
        Ok(NoDuplicates)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<NoDuplicates, A::Error> {
        while seq.next_element::<NoDuplicates>()?.is_some() {}
        Ok(NoDuplicates)
    }

    fn visit_bool<E>(self, _: bool) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }

    fn visit_i64<E>(self, _: i64) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }

    fn visit_u64<E>(self, _: u64) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }

    fn visit_f64<E>(self, _: f64) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }

    fn visit_str<E>(self, _: &str) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }

    fn visit_unit<E>(self) -> Result<NoDuplicates, E> {
        Ok(NoDuplicates)
    }
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

/// Whether `text` holds only whitespace and comments: a file `touch` made, or one an editor saved
/// with nothing but a comment in it. Such a file reads as `{}`, with no warning. An unclosed
/// block comment or a lone `/` is not blank, so the parser reports it.
pub(crate) fn is_blank(text: &[u8]) -> bool {
    let mut scan = Scan::Code;
    for &byte in text {
        scan = match (scan, byte) {
            (Scan::Code, b'/') => Scan::Slash,
            (Scan::Code, _) if byte.is_ascii_whitespace() => Scan::Code,
            (Scan::LineComment, b'\n') | (Scan::BlockStar, b'/') => Scan::Code,
            (Scan::LineComment, _) | (Scan::Slash, b'/') => Scan::LineComment,
            (Scan::BlockComment | Scan::BlockStar, b'*') => Scan::BlockStar,
            (Scan::BlockComment | Scan::BlockStar, _) | (Scan::Slash, b'*') => Scan::BlockComment,
            _ => return false,
        };
    }
    matches!(scan, Scan::Code | Scan::LineComment)
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
