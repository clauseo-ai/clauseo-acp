//! The comments of a YAML file, which every YAML parser drops, found with `saphyr-parser`'s
//! events: a `#` outside every scalar's span. Line numbers come from the parser's markers:
//! `line()` counts from 1 and `col()` counts characters from 0 (saphyr-parser 0.2.2,
//! `src/scanner.rs`, `Marker`; checked by the tests in `pins/tests.rs`).

use saphyr_parser::{Event, Marker, Parser, ScalarStyle, Span};

/// A parsed file: where its scalars are.
pub(super) struct Doc {
    /// The span and style of every scalar, so `comments` can tell a `#` in a string from a
    /// comment.
    scalars: Vec<(Span, ScalarStyle)>,
}

/// Why a file is not YAML, at the line the parser stopped on.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct NotYaml {
    pub line: u32,
    /// The parser's description, e.g. `did not find expected node content`.
    pub info: String,
}

/// Parses `text`, every document of it.
pub(super) fn parse(text: &str) -> Result<Doc, NotYaml> {
    let mut scalars = Vec::new();
    for event in Parser::new_from_str(text) {
        let (event, span) = event.map_err(|error| NotYaml {
            line: line(*error.marker()),
            info: error.info().to_owned(),
        })?;
        if let Event::Scalar(_, style, _, _) = event {
            scalars.push((span, style));
        }
    }
    Ok(Doc { scalars })
}

impl Doc {
    /// Every comment, as its line and the text after its `#`. A `#` starts a comment at the
    /// start of a line or after a space or tab, unless a scalar's span holds it (a `#` inside a
    /// quoted string or a block scalar is text).
    pub(super) fn comments<'t>(&self, text: &'t str) -> Vec<(u32, &'t str)> {
        let lines: Vec<&str> = text.lines().collect();
        let scalars: Vec<Place> = self
            .scalars
            .iter()
            .map(|(span, style)| {
                let start = (line(span.start), span.start.col());
                let end = match style {
                    ScalarStyle::DoubleQuoted => quoted_end(&lines, start, '"'),
                    ScalarStyle::SingleQuoted => quoted_end(&lines, start, '\''),
                    _ => (line(span.end), span.end.col()),
                };
                Place { start, end }
            })
            .collect();
        let mut out = Vec::new();
        for (index, source) in lines.iter().enumerate() {
            let at = crate::checks::line_at(index);
            let mut previous = None;
            for (col, (offset, ch)) in source.char_indices().enumerate() {
                let starts = previous.is_none_or(|before: char| before == ' ' || before == '\t');
                if ch == '#' && starts && !scalars.iter().any(|place| place.holds((at, col))) {
                    let rest = source.get(offset.saturating_add(1)..).unwrap_or_default();
                    out.push((at, rest));
                    break;
                }
                previous = Some(ch);
            }
        }
        out
    }
}

/// Where a scalar is: from its first character to just past its last, as (line from 1, column
/// in characters from 0).
struct Place {
    start: (u32, usize),
    end: (u32, usize),
}

impl Place {
    fn holds(&self, here: (u32, usize)) -> bool {
        self.start <= here && here < self.end
    }
}

/// Just past the closing quote of the quoted scalar whose opening quote is at `start`.
/// saphyr-parser 0.2.2 ends a quoted scalar's span after the blanks and comment that follow it,
/// so the end is found here: `\` escapes the next character in double quotes, and `''` is a
/// quote in single quotes. An unclosed one (the parser refuses it) runs to the end.
fn quoted_end(lines: &[&str], start: (u32, usize), quote: char) -> (u32, usize) {
    let (first, open) = start;
    let first_index = usize::try_from(first.saturating_sub(1)).unwrap_or(usize::MAX);
    for (index, source) in lines.iter().enumerate().skip(first_index) {
        let at = crate::checks::line_at(index);
        let skip = if index == first_index {
            open.saturating_add(1)
        } else {
            0
        };
        let mut chars = source.chars().enumerate().skip(skip).peekable();
        while let Some((col, ch)) = chars.next() {
            if quote == '"' && ch == '\\' {
                chars.next();
            } else if ch == quote {
                if quote == '\'' && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    return (at, col.saturating_add(1));
                }
            }
        }
    }
    (u32::MAX, 0)
}

/// The line of a marker, from 1.
fn line(marker: Marker) -> u32 {
    u32::try_from(marker.line()).unwrap_or(u32::MAX)
}
