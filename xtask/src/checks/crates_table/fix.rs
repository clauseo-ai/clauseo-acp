//! The fix lines of the Crates table reader, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use super::SECTION;
use super::names::CRATES_IO;
use std::fmt;

/// A fix line of the Crates table reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// The table's header is not `Crate | Owns | May depend on | Never depends on | Status`,
    /// or a row does not have five cells as written.
    Shape,
    /// The `## Crates` section has a `|` line after the table, or a second table: `end` is the
    /// table's last line and `line` the line after it.
    AfterTable { end: u32, line: u32 },
    /// The table has no rows.
    NoRows,
    /// A row's Status is neither `exists` nor `planned`.
    StatusWord,
    /// A row's crate name is not a directory name under `crates/`.
    CrateName,
    /// A crate has a second row.
    OneRow,
    /// A Never entry names a standard library module, which nothing would enforce.
    StdModule { entry: String },
    /// A Never entry is neither an I/O word nor a crate name.
    NotAName,
    /// A Never entry is an I/O word in other letter case, such as `FS`. `exact` is the word
    /// as the table writes it. `entry` is the entry when it is also a package name, which the
    /// fix then offers as a crates.io package with the `(crates.io)` marker.
    IoWordCase {
        exact: &'static str,
        entry: Option<String>,
    },
    /// A Never entry is one edit from a row name, read as a typo of that row. `ban` is how to
    /// write our crate, `package` its package, and `entry` the entry as written.
    TypoBan {
        ban: String,
        package: String,
        entry: String,
    },
    /// A Never entry holds `_` but is no row name: the crates.io spelling uses `-`, so `dashed`
    /// is the entry with `-` for `_`.
    Underscore { dashed: String },
    /// A Never entry is a row's package written wrongly (`clauseo_process`, `Clauseo-Process`).
    /// `row` is the row name when it also bans our crate, as for `process` (`clauseo-process`).
    OwnPackage {
        package: String,
        row: Option<String>,
        entry: String,
    },
    /// A Never entry is `clauseo-` or `clauseo_` with no row name after the prefix.
    NoRowName { entry: String },
    /// A Never entry names a row whose package has no prefix, such as `clauseo-xtask`: the
    /// package is the row name itself. `ban` is how to write our crate, `package` its package.
    Ban {
        ban: String,
        package: String,
        entry: String,
    },
    /// A Never entry starts with `clauseo-` or `clauseo_` and names no row. `row` is the name
    /// after the prefix, and `near` the spellings that ban a row one edit from it (empty when
    /// there are none).
    NoRow {
        near: Vec<String>,
        row: String,
        entry: String,
    },
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::Shape => f.write_str(
                "keep the Crates table in ARCHITECTURE.md as one pipe table with the columns Crate | \
                 Owns | May depend on | Never depends on | Status: xtask/src/checks/crates_table.rs \
                 reads it",
            ),
            Fix::AfterTable { end, line } => write!(
                f,
                "remove the blank lines between line {end} and line {line}, so the row joins the \
                 table, or move the text at line {line} out of the `{SECTION}` section: a blank \
                 line ends a table"
            ),
            Fix::NoRows => f.write_str("add a row for every crate that exists"),
            Fix::StatusWord => f.write_str("write `exists` or `planned` in the Status column"),
            Fix::CrateName => f.write_str(
                "name the crate as its directory under crates/: lowercase letters, digits and _",
            ),
            Fix::OneRow => f.write_str("keep one row per crate"),
            Fix::StdModule { entry } => write!(
                f,
                "write `any I/O` (or `fs` or `process`) instead of `{entry}`: each bans the standard \
                 library's file, network, process, environment and clock APIs; `clock` alone bans \
                 only Instant::now and SystemTime::now"
            ),
            Fix::NotAName => f.write_str(
                "write fs, process, any I/O, clock, a crate of the table or a crates.io package name \
                 (as `<name> (crates.io)` to mark it as one), separated by commas",
            ),
            Fix::IoWordCase { exact, entry } => {
                write!(
                    f,
                    "write `{exact}` to ban the standard library's APIs it names"
                )?;
                match entry {
                    Some(entry) => write!(
                        f,
                        ", or `{entry} {CRATES_IO}` if you mean the crates.io package"
                    ),
                    None => Ok(()),
                }
            }
            Fix::TypoBan {
                ban,
                package,
                entry,
            } => write!(
                f,
                "write `{ban}` to ban our crate `{package}`, or `{entry} {CRATES_IO}` if you mean \
                 the crates.io package: the module doc of xtask/src/checks/crates_table.rs lists \
                 which names read as ours"
            ),
            Fix::Underscore { dashed } => write!(
                f,
                "write `{dashed}` for the crates.io package (this check reads `-` and `_` the same), \
                 or the row name of the crate you mean if it is ours"
            ),
            Fix::OwnPackage {
                package,
                row,
                entry,
            } => {
                write!(f, "write `{package}`")?;
                if let Some(row) = row {
                    write!(f, " (or the row name `{row}`)")?;
                }
                write!(
                    f,
                    " to ban our crate, or `{entry} {CRATES_IO}` if you mean the crates.io package"
                )
            }
            Fix::NoRowName { entry } => write!(
                f,
                "write the name that bans the crate you mean (a name in the Crate column), or \
                 `{entry} {CRATES_IO}` if you mean the crates.io package"
            ),
            Fix::Ban {
                ban,
                package,
                entry,
            } => write!(
                f,
                "write `{ban}` to ban our crate `{package}`, or `{entry} {CRATES_IO}` if you mean \
                 the crates.io package"
            ),
            Fix::NoRow { near, row, entry } => {
                let hint = if near.is_empty() {
                    "a name in the Crate column".to_owned()
                } else {
                    let spellings: Vec<String> =
                        near.iter().map(|name| format!("`{name}`")).collect();
                    format!("perhaps {}", spellings.join(" or "))
                };
                write!(
                    f,
                    "write the name that bans the crate you mean ({hint}), add a row `{row}` for the \
                     crate, or write `{entry} {CRATES_IO}` if you mean the crates.io package"
                )
            }
        }
    }
}

#[cfg(test)]
mod tests;
