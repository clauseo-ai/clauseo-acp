//! The Crates table of `ARCHITECTURE.md`, read for the checks that enforce it.
//!
//! The table sits under `## Crates`: a pipe table with the columns `Crate`, `Owns`, `May depend
//! on`, `Never depends on` and `Status`. There is no Markdown parser among our dependencies, so
//! it is read by hand: each line from the first `|` line of the section to the first line that
//! does not start with `|` is a row; a row is split on `|` (a `\|` is a literal pipe), each cell
//! trimmed and its backticks removed. The second line must be the `|---|` separator.
//!
//! "Never depends on" entries are I/O words or crate names. The I/O words (`fs`, `process`,
//! `any I/O`, `clock`) mean the standard library's file, network, process, environment and clock
//! APIs, as `ARCHITECTURE.md` defines them, and win over a row of the same name (`process`). A
//! crate name that is a row of the table is our package `clauseo-<name>`; any other is the
//! crates.io package of that name.

use crate::checks::{self, violation};
use crate::violation::Violation;
use std::path::Path;

/// The file, at the repo root.
pub(crate) const FILE: &str = "ARCHITECTURE.md";

/// The heading of the section that holds the table.
const SECTION: &str = "## Crates";

/// The table's columns, in order.
const COLUMNS: [&str; 5] = [
    "Crate",
    "Owns",
    "May depend on",
    "Never depends on",
    "Status",
];

const SHAPE_FIX: &str = "keep the Crates table in ARCHITECTURE.md as one pipe table with the columns Crate | Owns | May depend on | Never depends on | Status: xtask/src/checks/crates_table.rs reads it";

/// The table, one row per crate, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CratesTable {
    pub rows: Vec<CrateRow>,
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrateRow {
    /// The 1-based line in `ARCHITECTURE.md`.
    pub line: u32,
    /// The crate's name without backticks: `xtask`, `desktop`, `fake_agent`.
    pub name: String,
    pub owns: String,
    /// The "May depend on" entries, as written (backticks removed), e.g. `none of ours`.
    pub may_depend_on: Vec<String>,
    pub never: Vec<Never>,
    pub status: Status,
}

/// Whether the crate exists yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Exists,
    Planned,
}

/// One "Never depends on" entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Never {
    /// One of the standard library's I/O APIs.
    Io(IoWord),
    /// Another row of the table, by its name: our package `clauseo-<name>`.
    Workspace(String),
    /// A crates.io package, by its name, e.g. `tauri`.
    External(String),
}

/// The I/O words of the "Never depends on" column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoWord {
    /// `fs`: the file, network, process, environment and clock APIs.
    Fs,
    /// `process`: the same APIs as `fs`.
    Process,
    /// `any I/O`: the same APIs.
    AnyIo,
    /// `clock`: the clock APIs.
    Clock,
}

impl IoWord {
    /// The word as the table writes it.
    pub(crate) fn word(self) -> &'static str {
        match self {
            IoWord::Fs => "fs",
            IoWord::Process => "process",
            IoWord::AnyIo => "any I/O",
            IoWord::Clock => "clock",
        }
    }

    /// The word `text` names, if any.
    fn parse(text: &str) -> Option<IoWord> {
        [IoWord::Fs, IoWord::Process, IoWord::AnyIo, IoWord::Clock]
            .into_iter()
            .find(|word| word.word() == text)
    }
}

impl Never {
    /// The cargo package this entry bans: `clauseo-<name>` for a row, the name itself for a
    /// crates.io package, `None` for an I/O word.
    pub(crate) fn package(&self) -> Option<String> {
        match self {
            Never::Io(_) => None,
            Never::Workspace(name) => Some(package_of(name)),
            Never::External(name) => Some(name.clone()),
        }
    }
}

impl CrateRow {
    /// True when the crate must not touch the standard library's I/O: its Never list has `fs`,
    /// `process` or `any I/O`.
    pub(crate) fn holds_core_io(&self) -> bool {
        self.never.iter().any(|entry| {
            matches!(
                entry,
                Never::Io(IoWord::Fs | IoWord::Process | IoWord::AnyIo)
            )
        })
    }

    /// The cargo package of this row: `xtask` for `xtask`, else `clauseo-<name>`.
    pub(crate) fn package(&self) -> String {
        package_of(&self.name)
    }
}

impl CratesTable {
    /// The row of a workspace member, by its package name: `xtask` is row `xtask`, and
    /// `clauseo-<name>` is row `<name>` (a `-` in `<name>` also matches a `_` in the row).
    pub(crate) fn row_for_package(&self, package: &str) -> Option<&CrateRow> {
        let name = row_name(package)?;
        self.row(name).or_else(|| self.row(&name.replace('-', "_")))
    }

    /// The row called `name`.
    pub(crate) fn row(&self, name: &str) -> Option<&CrateRow> {
        self.rows.iter().find(|row| row.name == name)
    }
}

/// The row name of a package: `xtask` for `xtask`, `<name>` for `clauseo-<name>`, else `None`.
pub(crate) fn row_name(package: &str) -> Option<&str> {
    if package == "xtask" {
        Some(package)
    } else {
        package
            .strip_prefix("clauseo-")
            .filter(|name| !name.is_empty())
    }
}

/// The package of row `name`: `xtask`, else `clauseo-<name>`.
fn package_of(name: &str) -> String {
    if name == "xtask" {
        name.to_owned()
    } else {
        format!("clauseo-{name}")
    }
}

/// Why the table could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoadError {
    /// `ARCHITECTURE.md` does not exist. There is no line to point at, so this is no `Violation`:
    /// a check that reads the table inspects nothing, and its zero-inspected failure reports it.
    Missing,
    /// Problems in the file, each at its line (line 1 for the whole file).
    Invalid(Vec<Violation>),
}

impl LoadError {
    /// The violations to report: none for `Missing` (see there), else the problems.
    pub(crate) fn into_violations(self) -> Vec<Violation> {
        match self {
            LoadError::Missing => Vec::new(),
            LoadError::Invalid(found) => found,
        }
    }
}

/// Reads the Crates table of `ARCHITECTURE.md` under `root`.
pub(crate) fn load(root: &Path) -> Result<CratesTable, LoadError> {
    match checks::read_repo_file(root, FILE) {
        Ok(Some(text)) => parse(&text).map_err(LoadError::Invalid),
        Ok(None) => Err(LoadError::Missing),
        Err(unreadable) => Err(LoadError::Invalid(vec![unreadable])),
    }
}

/// Reads the Crates table from `text`, the contents of `ARCHITECTURE.md`. `Err` holds every
/// problem found, sorted by line.
pub(crate) fn parse(text: &str) -> Result<CratesTable, Vec<Violation>> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(heading) = lines.iter().position(|line| line.trim_end() == SECTION) else {
        return Err(vec![violation(
            FILE,
            1,
            format!("ARCHITECTURE.md has no `{SECTION}` section"),
            SHAPE_FIX,
        )]);
    };
    let section_end = lines
        .iter()
        .enumerate()
        .skip(heading + 1)
        .find(|(_, line)| line.starts_with("# ") || line.starts_with("## "))
        .map_or(lines.len(), |(index, _)| index);
    let section = lines.get(heading + 1..section_end).unwrap_or_default();
    let Some(first) = section.iter().position(|line| is_table_line(line)) else {
        return Err(vec![violation(
            FILE,
            checks::line_at(heading),
            format!("the `{SECTION}` section has no table"),
            SHAPE_FIX,
        )]);
    };
    let start = heading + 1 + first;
    let table: Vec<(u32, Vec<String>)> = lines
        .iter()
        .enumerate()
        .skip(start)
        .take_while(|(_, line)| is_table_line(line))
        .map(|(index, line)| (checks::line_at(index), cells(line)))
        .collect();
    let mut found = Vec::new();
    let header_line = checks::line_at(start);
    if table.first().map(|(_, cells)| cells.as_slice())
        != Some(COLUMNS.map(String::from).as_slice())
    {
        let written = table
            .first()
            .map(|(_, cells)| cells.join(" | "))
            .unwrap_or_default();
        return Err(vec![violation(
            FILE,
            header_line,
            format!(
                "the Crates table's columns are `{written}`, not `{}`",
                COLUMNS.join(" | ")
            ),
            SHAPE_FIX,
        )]);
    }
    match table.get(1) {
        Some((_, cells)) if cells.len() == COLUMNS.len() && cells.iter().all(|c| is_rule(c)) => {}
        _ => {
            return Err(vec![violation(
                FILE,
                header_line + 1,
                "the Crates table's header is not followed by its `|---|` separator line",
                SHAPE_FIX,
            )]);
        }
    }
    let raw: Vec<RawRow> = table
        .iter()
        .skip(2)
        .filter_map(|(line, cells)| raw_row(*line, cells, &mut found))
        .collect();
    let rows = classify(&raw, &mut found);
    if table.len() == 2 {
        found.push(violation(
            FILE,
            header_line,
            "the Crates table has no rows",
            "add a row for every crate that exists",
        ));
    }
    if found.is_empty() {
        Ok(CratesTable { rows })
    } else {
        found.sort_by_key(|problem| problem.line);
        Err(found)
    }
}

/// A row split into cells, before its "Never depends on" entries are classified.
struct RawRow {
    line: u32,
    name: String,
    owns: String,
    may_depend_on: Vec<String>,
    never: Vec<String>,
    status: Status,
}

/// The row at `line`, or `None` with its problems added to `found`.
fn raw_row(line: u32, cells: &[String], found: &mut Vec<Violation>) -> Option<RawRow> {
    let [name, owns, may, never, status] = cells else {
        found.push(violation(
            FILE,
            line,
            format!(
                "this Crates table row has {} cells, not {}",
                cells.len(),
                COLUMNS.len()
            ),
            SHAPE_FIX,
        ));
        return None;
    };
    let status = match status.as_str() {
        "exists" => Some(Status::Exists),
        "planned" => Some(Status::Planned),
        other => {
            found.push(violation(
                FILE,
                line,
                format!("crate `{name}` has status `{other}`"),
                "write `exists` or `planned` in the Status column",
            ));
            None
        }
    };
    if !is_crate_name(name) {
        found.push(violation(
            FILE,
            line,
            format!("`{name}` is not a crate name"),
            "name the crate as its directory under crates/: lowercase letters, digits and _",
        ));
        return None;
    }
    Some(RawRow {
        line,
        name: name.clone(),
        owns: owns.clone(),
        may_depend_on: entries(may),
        never: entries(never),
        status: status?,
    })
}

/// The rows with their "Never depends on" entries classified against every row's name. A
/// repeated name, or an entry that is neither an I/O word nor a crate name, goes to `found`.
fn classify(raw: &[RawRow], found: &mut Vec<Violation>) -> Vec<CrateRow> {
    let mut rows = Vec::new();
    for (index, row) in raw.iter().enumerate() {
        if let Some(first) = raw.iter().take(index).find(|other| other.name == row.name) {
            found.push(violation(
                FILE,
                row.line,
                format!(
                    "crate `{}` has a second row (the first is at line {})",
                    row.name, first.line
                ),
                "keep one row per crate",
            ));
        }
        let mut never = Vec::new();
        for entry in &row.never {
            if let Some(word) = IoWord::parse(entry) {
                never.push(Never::Io(word));
            } else if raw.iter().any(|other| other.name == *entry) {
                never.push(Never::Workspace(entry.clone()));
            } else if is_package_name(entry) {
                never.push(Never::External(entry.clone()));
            } else {
                found.push(violation(
                    FILE,
                    row.line,
                    format!(
                        "`{entry}` in the Never depends on column of `{}` is neither an I/O word nor a crate name",
                        row.name
                    ),
                    "write fs, process, any I/O, clock, a crate of the table or a crates.io package name, separated by commas",
                ));
            }
        }
        rows.push(CrateRow {
            line: row.line,
            name: row.name.clone(),
            owns: row.owns.clone(),
            may_depend_on: row.may_depend_on.clone(),
            never,
            status: row.status,
        });
    }
    rows
}

/// True for a line of a pipe table: it starts with `|` after any indent.
fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// The cells of a table line: split on `|` (not `\|`, which is a literal pipe), the outer pipes
/// dropped, each cell trimmed and its backticks removed.
fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let inner = line.strip_prefix('|').unwrap_or(line);
    let inner = match inner.strip_suffix('|') {
        Some(rest) if !rest.ends_with('\\') => rest,
        _ => inner,
    };
    let mut found = Vec::new();
    let mut cell = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => found.push(std::mem::take(&mut cell)),
            '`' => {}
            _ => cell.push(c),
        }
    }
    found.push(cell);
    found.iter().map(|cell| cell.trim().to_owned()).collect()
}

/// The comma-separated entries of a cell, trimmed, empty ones dropped.
fn entries(cell: &str) -> Vec<String> {
    cell.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

/// True for a separator cell: `---`, with an optional `:` at either end.
fn is_rule(cell: &str) -> bool {
    let dashes = cell.trim_start_matches(':').trim_end_matches(':');
    !dashes.is_empty() && dashes.chars().all(|c| c == '-')
}

/// `^[a-z][a-z0-9_]*$`: a directory under `crates/`.
fn is_crate_name(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_lowercase())
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `^[A-Za-z0-9_-]+$`: what crates.io accepts as a package name.
fn is_package_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests;
