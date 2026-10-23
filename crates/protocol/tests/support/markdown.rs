//! Reads the tables of a Markdown document with `pulldown-cmark`, as xtask reads the Crates
//! table, so the tests that hold a doc to the code see the tables a reader sees: a row in a code
//! block, an indented block or an HTML comment is no row, a short row has empty cells for the
//! columns it leaves out, and a cell's names are its code spans, however they are separated.
//!
//! Each test file includes it as below; `pub` keeps a file that uses only some of it free of
//! dead-code warnings.
//!
//! ```text
//! #[cfg(test)]
//! #[path = "support/markdown.rs"]
//! pub mod markdown;
//! ```

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// One table cell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cell {
    /// The cell's text, each code span written back in backticks, HTML comments left out, the
    /// ends trimmed.
    pub text: String,
    /// The cell's code spans, in order.
    pub code: Vec<String>,
}

/// One table: its header row and its body rows, every row as wide as the header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    /// The header row.
    pub header: Vec<Cell>,
    /// The body rows, in order.
    pub rows: Vec<Vec<Cell>>,
}

impl Table {
    /// The index of the column whose header reads `name`; panics when there is none.
    pub fn column(&self, name: &str) -> usize {
        self.header
            .iter()
            .position(|cell| cell.text == name)
            .unwrap_or_else(|| panic!("no column {name:?} in {:?}", self.header))
    }

    /// Each body row's cell in the column named `name`.
    pub fn cells(&self, name: &str) -> Vec<&Cell> {
        let column = self.column(name);
        self.rows.iter().map(|row| &row[column]).collect()
    }
}

/// The tables in each section whose heading text `wanted` accepts, in document order. A section
/// runs from its heading to the next heading of the same or a higher level, so its subsections
/// are in it.
pub fn tables_where(doc: &str, wanted: impl Fn(&str) -> bool) -> Vec<Table> {
    let mut tables = Vec::new();
    // The level of the section being read, while inside one.
    let mut section: Option<HeadingLevel> = None;
    // The heading being read: its level and its text so far.
    let mut heading: Option<(HeadingLevel, String)> = None;
    let mut table: Option<Table> = None;
    let mut row: Vec<Cell> = Vec::new();
    let mut cell: Option<Cell> = None;
    for event in Parser::new_ext(doc, Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => heading = Some((level, String::new())),
            Event::End(TagEnd::Heading(_)) => {
                let (level, text) = heading.take().expect("a heading ends after it starts");
                if section.is_some_and(|open| level <= open) {
                    section = None;
                }
                if section.is_none() && wanted(text.trim()) {
                    section = Some(level);
                }
            }
            Event::Start(Tag::Table(_)) if section.is_some() => table = Some(Table::default()),
            Event::End(TagEnd::Table) => tables.extend(table.take()),
            Event::Start(Tag::TableCell) => cell = Some(Cell::default()),
            Event::End(TagEnd::TableCell) => {
                let mut done = cell.take().expect("a cell ends after it starts");
                done.text = done.text.trim().to_owned();
                row.push(done);
            }
            Event::End(TagEnd::TableHead) => {
                let header = std::mem::take(&mut row);
                if let Some(table) = &mut table {
                    table.header = header;
                }
            }
            Event::End(TagEnd::TableRow) => {
                let done = std::mem::take(&mut row);
                if let Some(table) = &mut table {
                    table.rows.push(done);
                }
            }
            Event::Text(text) => {
                if let Some(cell) = &mut cell {
                    cell.text.push_str(&text);
                } else if let Some((_, heading)) = &mut heading {
                    heading.push_str(&text);
                }
            }
            Event::Code(code) => {
                if let Some(cell) = &mut cell {
                    cell.text.extend(["`", &code, "`"]);
                    cell.code.push(code.into_string());
                } else if let Some((_, heading)) = &mut heading {
                    heading.push_str(&code);
                }
            }
            _ => {}
        }
    }
    tables
}

/// The one table in the section headed `heading` whose header has a column `column`; panics
/// when there is none or more than one.
pub fn table(doc: &str, heading: &str, column: &str) -> Table {
    let mut found: Vec<Table> = tables_where(doc, |text| text == heading)
        .into_iter()
        .filter(|table| table.header.iter().any(|cell| cell.text == column))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "tables with a column {column:?} under the heading {heading:?}"
    );
    found.remove(0)
}
