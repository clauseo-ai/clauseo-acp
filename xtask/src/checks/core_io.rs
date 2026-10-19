//! The `core-io` check. A crate whose Crates-table row in `ARCHITECTURE.md` lists `fs`,
//! `process` or `any I/O` under "Never depends on" may not use the standard library's file,
//! network, process and environment APIs, nor read the disk, the clock or the standard streams
//! another way. clippy enforces that, after macro expansion and with the compiler's own name
//! lookup: `xtask/core-io-bans.toml` lists every banned function and type for its
//! `disallowed-methods` and `disallowed-types` lints, and clippy reads only the nearest
//! `clippy.toml`, so each such crate has `crates/<name>/clippy.toml`, the root `clippy.toml` with
//! the list's arrays appended to its own. This check fails when that file is missing or parses
//! to anything else; its fix prints the file to write. A row that lists only `clock` needs no
//! file: the root `clippy.toml` bans both clocks' `now` for every crate.
//!
//! An exception is `#[expect(clippy::disallowed_methods, reason = "..")]` (or
//! `disallowed_types`) on the code that needs it. `tests/clippy.rs` runs clippy with the
//! generated file on one scratch crate, one module per form a ban must catch or must pass.
//!
//! The file covers every target of the package clippy lints: `src/`, `build.rs`, `tests/`,
//! `benches/` and `#[cfg(test)]` code alike. Not covered: code behind another target's `cfg`
//! (clippy lints it only on that target), code another crate runs for this one (a dependency, a
//! proc macro), and an item a newer toolchain adds that the list does not name. It counts the
//! Crates-table rows it classified, held or not.

mod clippy_toml;
mod report;

use crate::checks::crates_table::{self, CrateRow, IoWord, Never};
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::{Context, Member};
use clippy_toml::Expected;
use std::path::Path;

/// Fails on every crate that must not do I/O and lacks the `clippy.toml` that bans it.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "core-io",
    counts: Noun {
        one: "Crates table row",
        many: "Crates table rows",
    },
    place: || format!("in the Crates table of {}", crates_table::FILE),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// The names the Crates table rejects in a Never cell, where an I/O word belongs: `std` and the
/// standard library modules the I/O words ban (`fs`, `net`, `process`, `env`, `os`). Not `time`,
/// where only the clocks' `now` are banned and which is the crates.io crate.
const STD_MODULE_NAMES: [&str; 6] = ["std", "fs", "net", "process", "env", "os"];

/// True when `name` is `std` or a standard library module the I/O words ban (see
/// `STD_MODULE_NAMES`): `fs`, `process` and `any I/O` each ban all of them.
pub(crate) fn holds_banned_modules(name: &str) -> bool {
    STD_MODULE_NAMES.contains(&name)
}

fn run(ctx: &Context) -> Inspection {
    let table = match crates_table::load(&ctx.root) {
        Ok(table) => table,
        Err(error) => return Inspection::stopped_on(error.into_violations()),
    };
    let expected = match clippy_toml::load(&ctx.root) {
        Ok(expected) => expected,
        Err(problems) => return Inspection::stopped_on(problems),
    };
    let violations = ctx
        .members
        .iter()
        .filter_map(|member| {
            let row = table.row_for_package(&member.name)?;
            bans_modules(row).then_some((member, row))
        })
        .filter_map(|(member, row)| check_crate(&ctx.root, member, row, &expected))
        .collect();
    Inspection::new(table.rows.len(), violations)
}

/// True when `row` holds core I/O (`CrateRow::holds_core_io`) with `fs`, `process` or
/// `any I/O`, so its crate needs the list's bans. A row with `clock` alone needs no file: the
/// root `clippy.toml` bans both clocks' `now` for every crate.
fn bans_modules(row: &CrateRow) -> bool {
    row.holds_core_io()
        && row.never.iter().any(|entry| {
            matches!(
                entry,
                Never::Io(IoWord::Fs | IoWord::Process | IoWord::AnyIo)
            )
        })
}

/// The violation for the held crate `member`, whose row is `row`, if its `clippy.toml` is not
/// `expected`.
fn check_crate(
    root: &Path,
    member: &Member,
    row: &CrateRow,
    expected: &Expected,
) -> Option<Violation> {
    let package = &member.name;
    let path = checks::join(&member.dir, "clippy.toml");
    let text = match checks::read_repo_file(root, &path) {
        Ok(Some(text)) => text,
        Ok(None) => {
            return Some(report::missing_crate_file(
                row.line,
                package,
                &held_words(row),
                &path,
                &expected.text,
            ));
        }
        Err(unreadable) => return Some(unreadable),
    };
    match toml::from_str::<toml::Table>(&text) {
        Ok(found) if found == expected.table => None,
        Ok(found) => Some(report::crate_file_differs(
            &path,
            package,
            &clippy_toml::differing_keys(&found, &expected.table),
            &expected.text,
        )),
        Err(error) => {
            let line = error
                .span()
                .map_or(1, |span| clippy_toml::line_number(&text, span.start));
            Some(report::crate_file_not_toml(
                &path,
                line,
                &error,
                &expected.text,
            ))
        }
    }
}

/// The I/O words of `row`, as the table writes them.
fn held_words(row: &CrateRow) -> String {
    let words: Vec<&str> = row
        .never
        .iter()
        .filter_map(|entry| match entry {
            Never::Io(word) => Some(word.word()),
            _ => None,
        })
        .collect();
    words.join(", ")
}

#[cfg(test)]
mod tests;
