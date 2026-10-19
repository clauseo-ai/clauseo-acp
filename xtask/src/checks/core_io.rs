//! The `core-io` check. A crate whose Crates-table row in `ARCHITECTURE.md` lists `fs`,
//! `process` or `any I/O` under "Never depends on" must not name these standard-library paths:
//!
//! - `std::fs`, `std::net`, `std::process`, `std::env`: any path inside them, or naming them;
//! - `std::os::<any>::fs`, `std::os::<any>::net`, `std::os::<any>::process`: the same;
//! - `std::time::Instant::now` and `std::time::SystemTime::now`.
//!
//! A row that lists only `clock` bans the two `now` functions alone. Not covered: methods that
//! touch the disk or the clock without naming a banned path (`Path::exists`, `Path::metadata`,
//! `Path::read_dir`, `Path::canonicalize`, `UNIX_EPOCH.elapsed()`, `Instant::elapsed`),
//! `std::io::stdin` and the other standard streams, `build.rs`, `tests/` and `benches/`, and
//! code another crate runs for this one (a dependency, a proc macro).
//!
//! For each workspace member whose row holds core I/O, it parses every `.rs` file under the
//! crate's `src/` with `syn`, places each file in its crate roots (`files.rs`), resolves names
//! (`names.rs`) and reports each site that reaches a banned path (`sites.rs`, `bans.rs`). Code
//! the check would not parse fails on its own: a symlink under `src/`, a `#[path]` module, an
//! `include!`, and a `[lib]` or `[[bin]]` path outside `src/` in the crate's `Cargo.toml`.
//!
//! Exceptions are whole files of held crates, listed in `xtask/core-io-exceptions.txt` (see
//! `exceptions.rs`). It counts the Crates-table rows it classified, held or not.

mod bans;
mod files;
mod names;
mod report;
mod sites;
mod tokens;

use crate::checks::crates_table::{self, CrateRow, IoWord, Never};
use crate::checks::exceptions::{self, Exception};
use crate::checks::{self, Inspection, Noun, StaticCheck, violation};
use crate::violation::Violation;
use crate::workspace::{self, Context, Member};
use bans::Bans;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The exception file, repo-relative.
pub(crate) const EXCEPTIONS: &str = "xtask/core-io-exceptions.txt";

/// Fails on every place a crate that must not do I/O names a banned path of the standard library.
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

fn run(ctx: &Context) -> Inspection {
    let table = match crates_table::load(&ctx.root) {
        Ok(table) => table,
        Err(error) => return Inspection::new(0, error.into_violations()),
    };
    let held: Vec<(&Member, &CrateRow)> = ctx
        .members
        .iter()
        .filter_map(|member| {
            let row = table.row_for_package(&member.name)?;
            row.holds_core_io().then_some((member, row))
        })
        .collect();
    let (listed, mut violations) = exceptions::load(&ctx.root, EXCEPTIONS);
    for entry in &listed {
        if !ctx.root.join(&entry.path).is_file() {
            violations.push(report::not_a_file(entry));
        } else if !held
            .iter()
            .any(|(member, _)| workspace::is_under(&entry.path, &member.dir))
        {
            violations.push(report::outside_held_crates(entry));
        }
    }
    for (member, row) in held {
        violations.extend(check_crate(&ctx.root, member, row, &listed));
    }
    Inspection::new(table.rows.len(), violations)
}

/// Every violation in the held crate `member`, whose row is `row`.
fn check_crate(
    root: &Path,
    member: &Member,
    row: &CrateRow,
    listed: &[Exception],
) -> Vec<Violation> {
    let package = &member.name;
    let src = checks::join(&member.dir, "src");
    let listing = match files::list(root, &src) {
        Ok(listing) => listing,
        Err(error) => {
            return vec![report::at_row(
                row.line,
                format!("could not list `{src}/` of {package}: {error}"),
                format!("make `{src}/` readable: core-io parses every .rs file in it"),
            )];
        }
    };
    let mut violations: Vec<Violation> = listing
        .symlinks
        .iter()
        .map(|link| {
            report::at_row(
                row.line,
                format!("`{link}` in {package} is a symlink, which core-io does not follow"),
                format!(
                    "replace the symlink with the file or folder it points to, so the code sits \
                     under `{src}/`"
                ),
            )
        })
        .collect();
    let targets = target_paths(root, member, &src, &mut violations);
    if listing.files.is_empty() {
        violations.push(report::at_row(
            row.line,
            format!(
                "{package} has no .rs file under `{src}/`, so core-io cannot check that it stays \
                 free of std I/O (Never depends on: {})",
                held_words(row)
            ),
            format!("keep {package}'s Rust sources under `{src}/`, where core-io parses them"),
        ));
        return violations;
    }
    let exempt = |path: &str| listed.iter().any(|entry| entry.path == path);
    let mut parsed = BTreeMap::new();
    for path in &listing.files {
        match checks::read_repo_file(root, path) {
            Ok(Some(text)) => match syn::parse_file(&text) {
                Ok(file) => {
                    parsed.insert(path.clone(), file);
                }
                Err(error) if !exempt(path) => {
                    let line = u32::try_from(error.span().start().line).unwrap_or(u32::MAX);
                    violations.push(report::unparsed(path, line.max(1), &error));
                }
                Err(_) => {}
            },
            Ok(None) => {}
            Err(unreadable) => violations.push(unreadable),
        }
    }
    let roots = files::roots(&src, &listing.files, &targets);
    let placements = files::place(&src, &roots, &parsed);
    let mut table = names::Table::default();
    for placement in &placements {
        if let Some(file) = parsed.get(&placement.path) {
            table.add_file(&placement.module, file);
        }
    }
    let bans = Bans {
        modules: row.never.iter().any(|entry| {
            matches!(
                entry,
                Never::Io(IoWord::Fs | IoWord::Process | IoWord::AnyIo)
            )
        }),
    };
    let mut found: BTreeMap<&str, BTreeSet<sites::Site>> = BTreeMap::new();
    for placement in placements
        .iter()
        .filter(|placement| !exempt(&placement.path))
    {
        if let Some(file) = parsed.get(&placement.path) {
            let sites = sites::sites(&table, bans, &placement.module, file);
            found.entry(&placement.path).or_default().extend(sites);
        }
    }
    for (path, sites) in found {
        violations.extend(
            sites
                .into_iter()
                .map(|site| report::site(path, package, &src, site)),
        );
    }
    violations
}

/// The `[lib]` and `[[bin]]` paths of the crate's `Cargo.toml`; a path outside `src` and a
/// manifest that cannot be read go to `violations`.
fn target_paths(
    root: &Path,
    member: &Member,
    src: &str,
    violations: &mut Vec<Violation>,
) -> Vec<files::TargetPath> {
    let manifest = checks::join(&member.dir, "Cargo.toml");
    let text = match checks::read_repo_file(root, &manifest) {
        Ok(Some(text)) => text,
        Ok(None) => return Vec::new(),
        Err(unreadable) => {
            violations.push(unreadable);
            return Vec::new();
        }
    };
    let targets = match files::target_paths(&text) {
        Ok(targets) => targets,
        Err(error) => {
            let start = error.span().map_or(0, |span| span.start);
            let head = text.get(..start).unwrap_or(&text);
            let line = u32::try_from(head.matches('\n').count() + 1).unwrap_or(u32::MAX);
            violations.push(violation(
                &manifest,
                line,
                format!(
                    "core-io cannot read the target paths of `{manifest}`: {}",
                    error.message()
                ),
                "fix the manifest so cargo and core-io can read it",
            ));
            return Vec::new();
        }
    };
    for target in targets.iter().filter(|target| target.in_src.is_none()) {
        violations.push(violation(
            &manifest,
            target.line,
            format!(
                "{}'s {} path `{}` is outside `{src}/`, and core-io parses only that folder",
                member.name, target.table, target.path
            ),
            format!("move the target's root file under `{src}/` and set `path` to it"),
        ));
    }
    targets
}

/// The I/O words that make `row` hold core I/O, as the table writes them.
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
