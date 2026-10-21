//! Finds the Crates table in `ARCHITECTURE.md` with `pulldown-cmark`, reading tables as GitHub
//! does (GFM): a row without a leading pipe is a row, a blank line or another block ends the
//! table, and a `\|` is a pipe inside a cell. Cell text is what GitHub shows: code spans keep
//! their content without the backticks, other inline markup keeps only its text.

use super::{FILE, Fix, SECTION};
use crate::checks::{self, violation};
use crate::violation::Violation;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// The table under `## Crates`, as GFM reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Table {
    /// The 1-based line of the header row.
    pub header_line: u32,
    pub header: Vec<String>,
    pub rows: Vec<Row>,
    /// Lines of the section after the table that start with `|`: GFM reads them as text, not as
    /// rows, so a row there would be dropped. A second table in the section counts too.
    pub stray: Vec<u32>,
}

/// One body row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    /// The 1-based line of the row.
    pub line: u32,
    /// The cells as GFM reads them: padded with empty cells or cut to the header's count.
    pub cells: Vec<String>,
    /// How many cells the row's source line holds, split on `|` as GFM splits it: GFM pads or
    /// cuts a row to the header's count, so this is what the cell-count rule reads.
    pub written: usize,
}

/// Reads the table of the `## Crates` section of `text`. `Err` is the one problem that stops
/// the reading: no such section, or no table in it.
pub(super) fn read(text: &str) -> Result<Table, Violation> {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let line_of = |offset: usize| {
        checks::line_at(
            starts
                .partition_point(|start| *start <= offset)
                .saturating_sub(1),
        )
    };
    let options = Options::ENABLE_TABLES | Options::ENABLE_GFM;
    let mut heading: Option<(usize, String)> = None;
    let mut section: Option<usize> = None;
    let mut table: Option<Table> = None;
    let mut done = false;
    let mut row: Option<Row> = None;
    let mut cell: Option<String> = None;
    let mut in_head = false;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. })
                if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) =>
            {
                if section.is_some() {
                    break;
                }
                heading = (level == HeadingLevel::H2).then(|| (range.start, String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((start, title)) = heading.take()
                    && format!("## {}", title.trim()) == SECTION
                {
                    section = Some(start);
                }
            }
            Event::Start(Tag::Table(_)) if section.is_some() => {
                if done {
                    if let Some(found) = table.as_mut() {
                        found.stray.push(line_of(range.start));
                    }
                } else {
                    table = Some(Table {
                        header_line: line_of(range.start),
                        header: Vec::new(),
                        rows: Vec::new(),
                        stray: Vec::new(),
                    });
                }
            }
            Event::End(TagEnd::Table) => done = table.is_some(),
            Event::Start(Tag::TableHead) if !done && table.is_some() => in_head = true,
            Event::End(TagEnd::TableHead) => in_head = false,
            Event::Start(Tag::TableRow) if !done && table.is_some() => {
                let source = text.get(range.clone()).unwrap_or_default();
                row = Some(Row {
                    line: line_of(range.start),
                    cells: Vec::new(),
                    written: written_cells(source.lines().next().unwrap_or_default()),
                });
            }
            Event::End(TagEnd::TableRow) => {
                if let (Some(found), Some(done_row)) = (table.as_mut(), row.take()) {
                    found.rows.push(done_row);
                }
            }
            Event::Start(Tag::TableCell) if !done && table.is_some() => {
                cell = Some(String::new());
            }
            Event::End(TagEnd::TableCell) => {
                if let (Some(found), Some(text)) = (table.as_mut(), cell.take()) {
                    let text = text.trim().to_owned();
                    match row.as_mut() {
                        Some(body) if !in_head => body.cells.push(text),
                        _ => found.header.push(text),
                    }
                }
            }
            Event::Text(piece) | Event::Code(piece) => {
                if let Some(cell) = cell.as_mut() {
                    cell.push_str(&piece);
                } else if let Some((_, title)) = heading.as_mut() {
                    title.push_str(&piece);
                }
            }
            Event::Start(Tag::Paragraph) if done => {
                let source = text.get(range.clone()).unwrap_or_default();
                if let Some(found) = table.as_mut() {
                    let first = line_of(range.start);
                    found.stray.extend(
                        source
                            .lines()
                            .zip(first..)
                            .filter(|(line, _)| line.trim_start().starts_with('|'))
                            .map(|(_, line)| line),
                    );
                }
            }
            _ => {}
        }
    }
    let Some(heading) = section else {
        return Err(violation(
            FILE,
            1,
            format!("ARCHITECTURE.md has no `{SECTION}` section"),
            Fix::Shape,
        ));
    };
    table.ok_or_else(|| no_table(text, line_of(heading)))
}

/// The violation for a `## Crates` section, heading at line `heading`, that GFM reads no table
/// in: at the line after the first `|` line when the section has one, since a pipe table needs
/// its `|---|` separator line under the header to be a table at all.
fn no_table(text: &str, heading: u32) -> Violation {
    let section = text
        .lines()
        .zip(1u32..)
        .skip_while(|(_, line)| *line <= heading)
        .take_while(|(text, _)| !text.starts_with("# ") && !text.starts_with("## "));
    match section
        .into_iter()
        .find(|(text, _)| text.trim_start().starts_with('|'))
    {
        Some((_, line)) => violation(
            FILE,
            line + 1,
            "the Crates table's header is not followed by its `|---|` separator line",
            Fix::Shape,
        ),
        None => violation(
            FILE,
            heading,
            format!("the `{SECTION}` section has no table"),
            Fix::Shape,
        ),
    }
}

/// How many cells the table line `line` holds: split on `|` but not `\|`, with an optional
/// leading and trailing pipe, as GFM splits a row before reading its cells.
fn written_cells(line: &str) -> usize {
    let line = line.trim();
    let inner = line.strip_prefix('|').unwrap_or(line);
    let inner = match inner.strip_suffix('|') {
        Some(rest) if !rest.ends_with('\\') => rest,
        _ => inner,
    };
    let mut count = 1;
    let mut escaped = false;
    for c in inner.chars() {
        match c {
            '|' if !escaped => count += 1,
            _ => {}
        }
        escaped = c == '\\' && !escaped;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_cells_splits_on_unescaped_pipes_with_optional_outer_pipes() {
        assert_eq!(written_cells("| a | b |"), 2);
        assert_eq!(written_cells("a | b"), 2);
        assert_eq!(written_cells("| a \\| b | c |"), 2);
        assert_eq!(written_cells("| a | b | c | d | e | f |"), 6);
        assert_eq!(written_cells("plain"), 1);
    }
}
