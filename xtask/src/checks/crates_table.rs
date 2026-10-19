//! The Crates table of `ARCHITECTURE.md`, read for the checks that enforce it.
//!
//! The table sits under `## Crates`: a pipe table with the columns `Crate`, `Owns`, `May depend
//! on`, `Never depends on` and `Status`. There is no Markdown parser among our dependencies, so
//! it is read by hand: each line from the first `|` line of the section to the first line that
//! does not start with `|` is a row; a row is split on `|` (a `\|` is a literal pipe), each cell
//! trimmed and its backticks removed. The second line must be the `|---|` separator. Markdown
//! also reads a line without a leading pipe as a row, and a blank line ends the table, so the
//! next non-blank line of the section after the table must hold no `|`: rows past the end
//! would be dropped without a word. Only the Crate, Never depends on and Status cells are read;
//! the others must be there, so every row has five cells.
//!
//! "Never depends on" entries are I/O words or crate names. The I/O words name standard library
//! APIs, as `ARCHITECTURE.md` defines them: `fs`, `process` and `any I/O` each mean the file,
//! network, process, environment and clock APIs; `clock` alone means the two clock `now`
//! functions. They win over a row of the same name (`process`).
//! `std`, and a standard library module that core-io bans whole (`net`, `env`, `os`: see
//! `core_io/bans.rs`), are rejected: they name no crate we use, and the I/O words are how the
//! table bans those APIs. `time` is the crates.io crate. A crate name that is a row of the
//! table, or `clauseo-` followed by one (`clauseo-process`, since a bare `process` is the I/O
//! word), is our package `clauseo-<name>`; any other name is the crates.io package of that name.
//! A name that looks like ours but is no row is rejected, so a typo never turns into a ban on a
//! crates.io package nobody uses: `clauseo-` followed by no row, a name holding `_` (our row
//! names hold `_`, crates.io names are written with `-` here, and cargo reads the two the
//! same), and a name one edit from a row name once `-` and `_` are folded.
//!
//! A ban names exactly one package. `tauri` bans the package `tauri`, not `tauri-runtime`,
//! `tauri-utils`, `wry` or a fork under another name; banning a family means listing each of
//! its packages.

use crate::checks::{self, core_io, violation};
use crate::violation::Violation;
use std::path::Path;

mod lines;
mod names;

use lines::{cells, entries, is_crate_name, is_package_name, is_rule, is_table_line, past_the_end};

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
    /// `clock`: only `Instant::now` and `SystemTime::now`.
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
    /// True when the core-io check reads the crate: its Never list has `fs`, `process` or
    /// `any I/O` (the whole ban) or `clock` (the two `now` functions only).
    pub(crate) fn holds_core_io(&self) -> bool {
        self.never.iter().any(|entry| matches!(entry, Never::Io(_)))
    }
}

impl CratesTable {
    /// The row of a workspace member, by its package name: `xtask` is row `xtask`, and
    /// `clauseo-<name>` is row `<name>` (a `-` in `<name>` also matches a `_` in the row, see
    /// `fold`).
    pub(crate) fn row_for_package(&self, package: &str) -> Option<&CrateRow> {
        let name = fold(row_name(package)?);
        self.row(&name)
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

/// `name` with every `-` written `_`: cargo treats the two as the same in a package name, so a
/// row `transport_ws` is the package `clauseo-transport-ws`, and a ban matches either spelling.
pub(crate) fn fold(name: &str) -> String {
    name.replace('-', "_")
}

/// The package of row `name`: `xtask`, else `clauseo-<name>`.
pub(crate) fn package_of(name: &str) -> String {
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
    let after = start + table.len();
    if let Some((index, line)) = lines
        .iter()
        .enumerate()
        .take(section_end)
        .skip(after)
        .find(|(_, line)| !line.trim().is_empty())
        && line.contains('|')
    {
        found.push(past_the_end(
            checks::line_at(after - 1),
            checks::line_at(index),
            line,
        ));
    }
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
    never: Vec<String>,
    status: Status,
}

/// The row at `line`, or `None` with its problems added to `found`.
fn raw_row(line: u32, cells: &[String], found: &mut Vec<Violation>) -> Option<RawRow> {
    let [name, _owns, _may, never, status] = cells else {
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
            } else if core_io::holds_banned_modules(entry) {
                found.push(violation(
                    FILE,
                    row.line,
                    format!(
                        "`{entry}` in the Never depends on column of `{}` names a standard library module, not a crate, so nothing would enforce it",
                        row.name
                    ),
                    format!(
                        "write `any I/O` (or `fs` or `process`) instead of `{entry}`: each bans the standard library's file, network, process, environment and clock APIs; `clock` alone bans only Instant::now and SystemTime::now"
                    ),
                ));
            } else if raw.iter().any(|other| other.name == *entry) {
                never.push(Never::Workspace(entry.clone()));
            } else if is_package_name(entry) {
                let rows: Vec<&str> = raw.iter().map(|other| other.name.as_str()).collect();
                match names::crate_entry(entry, &rows) {
                    Ok(entry) => never.push(entry),
                    Err(problem) => found.push(violation(
                        FILE,
                        row.line,
                        format!(
                            "`{entry}` in the Never depends on column of `{}` {}",
                            row.name, problem.what
                        ),
                        problem.fix,
                    )),
                }
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
            never,
            status: row.status,
        });
    }
    rows
}

#[cfg(test)]
mod tests;
