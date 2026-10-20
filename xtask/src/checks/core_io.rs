//! The `core-io` check. A crate whose Crates-table row in `ARCHITECTURE.md` lists `fs`,
//! `process`, `any I/O` or `clock` under "Never depends on" is held. A row with `fs`, `process`
//! or `any I/O` bans the standard library's file, network, process and environment APIs, and
//! reading the disk, the clock or the standard streams another way. A row with `clock` alone bans
//! both clocks' `now`, which the root `clippy.toml` bans for every crate that reads it.
//!
//! clippy enforces the bans, after macro expansion and with the compiler's own name lookup.
//! `xtask/core-io-bans.toml` lists every banned function and type for its `disallowed-methods`
//! and `disallowed-types` lints. clippy reads only the nearest `clippy.toml`, so a crate whose
//! row lists `fs`, `process` or `any I/O` has `crates/<name>/clippy.toml`, the root
//! `clippy.toml` with the list's arrays appended to its own. The check fails when that file is
//! missing or parses to anything else, and its fix prints the file to write.
//!
//! The compiler keeps the bans on in a held crate. Every target root of a held crate that
//! `cargo metadata` lists, the library, binaries, tests, examples and benches alike, has
//! `#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]` among its inner
//! attributes, with that meta in that order and not under `cfg_attr`. A macro can neither add nor
//! remove a crate-root attribute. Under `forbid`, an `allow` or `expect` of
//! `clippy::disallowed_methods` or `clippy::disallowed_types` anywhere in the target is error
//! E0453, and a level for `disallowed_methods` or `disallowed_types` in the manifest or in
//! `CLIPPY_ARGS` does not undo it. So a held crate gets no exception; a test that needs a
//! fixture reads it with `include_str!`. A root syn cannot parse fails at the syntax error. A
//! root outside the crate's folder, once its `.` and `..` components are resolved, fails at the
//! crate's manifest, since cargo builds it all the same and the cfg rule below reads only the
//! folder. A held crate also has no build script, neither a `build.rs` in its folder nor a
//! `build` key under `[package]`, since a build script's `cargo::rustc-env` lines reach the
//! clippy that lints the crate (`roots.rs`).
//!
//! clippy sees all of a held crate's code. The clippy step runs with `--all-targets` and
//! `--all-features` (`steps/plan.rs`), so clippy lints the crate once per target kind with every
//! feature on, and code is linted only when its cfg holds in that build. `test` holds in the test
//! build, `not(test)` in the others, and a positive feature in all of them. Any other cfg can
//! leave code out of every clippy run, so a held crate's cfgs use only `test` and positive
//! features. Every `.rs` file in git's list under the crate's folder is lexed with proc-macro2,
//! macro bodies and macro call arguments included. The predicate of each `cfg(..)`, `cfg!(..)`
//! and `cfg_attr(..)` is built only from `all`, `any`, `not`, `test` and `feature = "<literal>"`,
//! with no `feature` under a `not`. An attribute whose name is a metavariable fails, since a macro
//! could build a `cfg` from a passed name. A `path` attribute and `include!` fail, so every module
//! file sits at its default path under the crate and is lexed. A file that does not lex fails at
//! the line the lexer names (`cfgs.rs`).
//!
//! Lint configuration lives only in the root `clippy.toml`, the own `clippy.toml` of each crate
//! whose row lists `fs`, `process` or `any I/O`, and the root `[workspace.lints]`. So the check
//! also fails
//! - every other `clippy.toml` and every `.clippy.toml` in git's list of the repo's files,
//!   tracked and untracked but not ignored, since clippy reads the nearest one walking up from
//!   the crate, a `.clippy.toml` before a `clippy.toml`;
//! - a member's `Cargo.toml` without `[lints] workspace = true`, any member, held or not;
//! - a legacy `.cargo/config`, which cargo reads instead of `.cargo/config.toml`;
//! - in `.cargo/config.toml` (`lints.rs`), a `rustflags` key at any depth, since
//!   `--cap-lints allow` there beats `forbid`;
//! - in `.cargo/config.toml`, an `include` key, since cargo loads the files it names and core-io
//!   does not read them;
//! - in `.cargo/config.toml`, the `[build]` keys `rustc`, `rustc-wrapper` and
//!   `rustc-workspace-wrapper`, since each replaces or wraps the compiler clippy runs and can add
//!   `--cap-lints allow`;
//! - in `.cargo/config.toml`, an `[alias]` key named after an external cargo command xtask runs
//!   (`clippy`, `fmt`, `nextest`, `deny` or `shear`), since cargo runs the alias instead and
//!   only warns;
//! - in `.cargo/config.toml`, an `[env]` entry starting with `CLIPPY_` in any case.
//!
//! xtask also runs cargo without `CLIPPY_CONF_DIR` in its own environment (`process.rs`).
//! `tests/clippy.rs` runs clippy with the generated file on scratch crates, one module per form a
//! ban must catch or must pass, and one per form the forbid line must refuse.
//!
//! The file covers every target of the package clippy lints, `#[cfg(test)]` code included. Not
//! covered: code another crate runs for this one (a dependency, a proc macro), a cfg that a macro
//! from a crate that is not held wraps around code, since the cfg rule reads only the held
//! crate's files, an item a newer toolchain adds that the list does not name, a source file or a
//! clippy config git ignores, cargo config outside the repo (parent folders, `CARGO_HOME`),
//! `RUSTFLAGS` and `RUSTC_WRAPPER` in the environment, `CLIPPY_CONF_DIR` when clippy runs
//! without xtask, and a crate that is not a workspace member.
//! It counts the Crates-table rows it classified, held or not.

mod cfgs;
mod clippy_toml;
mod lints;
mod report;
mod roots;

use crate::changes;
use crate::checks::crates_table::{self, CrateRow, IoWord, Never};
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::{Context, Member};
use clippy_toml::Expected;
use std::collections::BTreeSet;
use std::path::Path;

/// Fails on every crate that must not do I/O and lacks what makes clippy ban it, and on every
/// file that could turn the bans off.
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
    let files = match changes::repo_files(&ctx.root) {
        Ok(files) => files,
        Err(error) => return checks::unlisted(CHECK.name, &error),
    };
    let held: Vec<(&Member, &CrateRow)> = ctx
        .members
        .iter()
        .filter_map(|member| {
            let row = table.row_for_package(&member.name)?;
            row.holds_core_io().then_some((member, row))
        })
        .collect();
    let mut violations = Vec::new();
    for (member, row) in &held {
        if bans_modules(row) {
            violations.extend(check_crate(&ctx.root, member, row, &expected));
        }
        violations.extend(roots::forbid_lines(&ctx.root, member));
        violations.extend(roots::build_script(&ctx.root, member));
        violations.extend(cfgs::cfg_rule(&ctx.root, member, &files));
    }
    for member in &ctx.members {
        violations.extend(lints::crate_manifest(&ctx.root, member));
    }
    let allowed: BTreeSet<String> = held
        .iter()
        .filter(|(_, row)| bans_modules(row))
        .map(|(member, _)| checks::join(&member.dir, CLIPPY_TOML))
        .chain([CLIPPY_TOML.to_owned()])
        .collect();
    violations.extend(
        files
            .iter()
            .filter(|file| is_clippy_config(file) && !allowed.contains(*file))
            .map(|file| report::stray_config(file)),
    );
    violations.extend(lints::cargo_config(&ctx.root));
    Inspection::new(table.rows.len(), violations)
}

/// The name of clippy's config file, at the root and in each held crate.
const CLIPPY_TOML: &str = "clippy.toml";

/// True when `file` (repo-relative) is a file clippy reads its config from: `clippy.toml`, or
/// `.clippy.toml`, which wins over a `clippy.toml` in the same folder.
fn is_clippy_config(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    name == CLIPPY_TOML || name == ".clippy.toml"
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
    let path = checks::join(&member.dir, CLIPPY_TOML);
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
                package,
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
