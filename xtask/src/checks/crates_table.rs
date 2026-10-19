//! The Crates table of `ARCHITECTURE.md`, read for the checks that enforce it.
//!
//! The table sits under `## Crates`: a pipe table with the columns `Crate`, `Owns`, `May depend
//! on`, `Never depends on` and `Status`. `table.rs` reads it with `pulldown-cmark` as GitHub
//! renders it (GFM): the first table of the section, a row without a leading pipe included, a
//! `\|` being a pipe inside a cell, and a code span's text without its backticks. A blank line
//! ends a table, and GFM shows a `|` line after it as text, so a line of the section after the
//! table that starts with `|`, or a second table, fails. A line GFM does not read as part of the
//! table is not a row: a row written with no pipe at either end after a blank line is a
//! paragraph, and its crate, having no row, fails the `dependencies` check once it exists.
//! Only the Crate, Never depends on and Status cells are read; the others must be there, so
//! every row has five cells, counted as written (GFM pads a short row and cuts a long one).
//!
//! This doc is the one home of the exact rules for the "Never depends on" entries;
//! `ARCHITECTURE.md` says what a row's author needs, and defines the I/O words (`fs`,
//! `process`, `any I/O`, `clock`). An entry is read as the first of these that fits:
//! - `<name> (crates.io)`: the crates.io package `<name>`, whatever the rules below say of it,
//!   for a real crate that sits one edit from a row (`rmcp`, near `mcp`);
//! - an I/O word, exactly as written. It wins over a row of the same name (`process`);
//! - an I/O word in other letter case (`FS`, `Any I/O`): rejected with the exact word to write,
//!   not read as a crates.io name;
//! - `std`, or a standard library module that core-io bans whole (`net`, `env`, `os`: see
//!   `core_io.rs`), in any letter case (`Std`, `NET`): rejected, since it names no crate we use
//!   and the I/O words are how the table bans those APIs. `time` is the crates.io crate;
//! - a row's name: that row's crate, the package `clauseo-<name>` (`xtask` for `xtask`);
//! - a row's package, in lowercase and starting `clauseo-`, `-` and `_` read the same after
//!   that (`clauseo-process`, since a bare `process` is the I/O word): that row's crate;
//! - any other name starting with `clauseo-` or `clauseo_` in any letter case: rejected, with
//!   the package to write when it is a row's (`clauseo_process`, `Clauseo-Process`), and
//!   `clauseo-xtask` too (the xtask row's package is `xtask`);
//! - a name at most one edit from a row name, comparing in lowercase with `-` read as `_` (see
//!   `fold`), an inserted, deleted or replaced letter or a swap of two adjacent letters each
//!   counting as one edit: rejected as a likely typo of that row (`procesz`, `Mcp`,
//!   `fake-agent`);
//! - any other name holding `_`: rejected, since only our row names are written with `_` and
//!   a crates.io name is written here with `-` (this check reads the two the same);
//! - any other package name: the crates.io package of that name;
//! - anything else: rejected as neither an I/O word nor a crate name.
//!
//! A rejected look-alike never turns into a ban on a crates.io package nobody uses, and its fix
//! says how to write the ban either way. A ban names exactly one package. `tauri` bans the
//! package `tauri` (in any letter case: crates.io names are unique ignoring case), not
//! `tauri-runtime`, `tauri-utils`, `wry` or a fork under another name; banning a family means
//! listing each of its packages.

use crate::checks::{self, core_io, violation};
use crate::violation::Violation;
use std::path::Path;

mod cells;
mod names;
mod table;

use cells::{entries, is_crate_name, is_package_name};

/// The number of columns, as a pattern.
const COLUMNS_LEN: usize = COLUMNS.len();

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
        IoWord::ALL.into_iter().find(|word| word.word() == text)
    }

    /// The word `text` names once letter case is ignored (`FS`, `Any I/O`), if any.
    fn parse_ignoring_case(text: &str) -> Option<IoWord> {
        IoWord::ALL
            .into_iter()
            .find(|word| word.word().eq_ignore_ascii_case(text))
    }

    /// Every I/O word.
    const ALL: [IoWord; 4] = [IoWord::Fs, IoWord::Process, IoWord::AnyIo, IoWord::Clock];
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

/// `name` in lowercase with every `-` written `_`: crates.io names are unique ignoring case
/// and `-` against `_`, so a row `transport_ws` matches `clauseo-transport-ws` too, and a
/// ban (`Tauri`) matches the package (`tauri`) whatever the spelling.
pub(crate) fn fold(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
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
    let table = table::read(text).map_err(|problem| vec![problem])?;
    let header_line = table.header_line;
    if table.header != COLUMNS.map(String::from) {
        return Err(vec![violation(
            FILE,
            header_line,
            format!(
                "the Crates table's columns are `{}`, not `{}`",
                table.header.join(" | "),
                COLUMNS.join(" | ")
            ),
            SHAPE_FIX,
        )]);
    }
    let mut found: Vec<Violation> = table
        .stray
        .iter()
        .map(|line| {
            after_the_table(
                table.rows.last().map_or(header_line + 1, |row| row.line),
                *line,
            )
        })
        .collect();
    let raw: Vec<RawRow> = table
        .rows
        .iter()
        .filter_map(|row| raw_row(row, &mut found))
        .collect();
    let rows = classify(&raw, &mut found);
    if table.rows.is_empty() {
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

/// The violation for line `line` of the `## Crates` section, after the table (whose last line
/// is `end`), which starts with `|` or starts a second table: GFM shows it outside the table,
/// so a row written there would never be read.
fn after_the_table(end: u32, line: u32) -> Violation {
    violation(
        FILE,
        line,
        format!(
            "the Crates table ends at line {end}, but line {line} after it starts with `|`, so a row there is not part of the table"
        ),
        format!(
            "remove the blank lines between line {end} and line {line}, so the row joins the table, or move the text at line {line} out of the `{SECTION}` section: a blank line ends a table"
        ),
    )
}

/// The violation for `entry`, in the Never cell of row `row` at `line`: the I/O word `word`
/// with other letter case. Only a package name could be a crates.io ban, so only then does the
/// message say so and the fix offer the `(crates.io)` marker.
fn io_word_in_other_case(entry: &str, word: IoWord, row: &str, line: u32) -> Violation {
    let exact = word.word();
    let (would, or_marker) = if is_package_name(entry) {
        (
            ", and as written it would ban a crates.io package instead",
            format!(
                ", or `{entry} {}` if you mean the crates.io package",
                names::CRATES_IO
            ),
        )
    } else {
        ("", String::new())
    };
    violation(
        FILE,
        line,
        format!(
            "`{entry}` in the Never depends on column of `{row}` is the I/O word `{exact}` written with other letter case{would}"
        ),
        format!("write `{exact}` to ban the standard library's APIs it names{or_marker}"),
    )
}

/// A row split into cells, before its "Never depends on" entries are classified.
struct RawRow {
    line: u32,
    name: String,
    never: Vec<String>,
    status: Status,
}

/// The body row `row`, or `None` with its problems added to `found`.
fn raw_row(row: &table::Row, found: &mut Vec<Violation>) -> Option<RawRow> {
    let line = row.line;
    let ([name, _owns, _may, never, status], COLUMNS_LEN) = (row.cells.as_slice(), row.written)
    else {
        found.push(violation(
            FILE,
            line,
            format!(
                "this Crates table row has {} cells, not {}",
                row.written,
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
            if let Some(name) = names::marked_crates_io(entry)
                && is_package_name(name)
            {
                never.push(Never::External(name.to_owned()));
            } else if let Some(word) = IoWord::parse(entry) {
                never.push(Never::Io(word));
            } else if let Some(word) = IoWord::parse_ignoring_case(entry) {
                found.push(io_word_in_other_case(entry, word, &row.name, row.line));
            } else if core_io::holds_banned_modules(entry)
                || core_io::holds_banned_modules(&entry.to_ascii_lowercase())
            {
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
                    "write fs, process, any I/O, clock, a crate of the table or a crates.io package name (as `<name> (crates.io)` to mark it as one), separated by commas",
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
