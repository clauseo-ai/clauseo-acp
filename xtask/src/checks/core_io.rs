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
    place: "in the Crates table of ARCHITECTURE.md",
    source: "xtask/src/checks/crates_table.rs",
    run,
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

/// The text of the violation for a banned path at `crates/protocol/src/<file>:<line>`.
#[cfg(test)]
macro_rules! uses {
    ($file:literal, $line:literal, $banned:literal) => {
        concat!(
            "crates/protocol/src/",
            $file,
            ":",
            $line,
            ": clauseo-protocol uses ",
            $banned,
            ", which ARCHITECTURE.md forbids in this crate\n    fix: take what ",
            $banned,
            " gives as a parameter, or move this code to a crate that may do I/O, or add \
             `crates/protocol/src/",
            $file,
            " <reason>` to xtask/core-io-exceptions.txt"
        )
    };
}

/// The text of the violation for a glob import of `$prefix` in `lib.rs`.
#[cfg(test)]
macro_rules! glob {
    ($line:literal, $prefix:literal, $reaches:expr) => {
        concat!(
            "crates/protocol/src/lib.rs:",
            $line,
            ": clauseo-protocol glob-imports ",
            $prefix,
            "::*, which reaches ",
            $reaches,
            "; ARCHITECTURE.md forbids that in this crate\n    \
             fix: import the items you use from ",
            $prefix,
            " by name instead, or add \
             `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt"
        )
    };
}

/// The text of the violation for a path built from a metavariable in `lib.rs`.
#[cfg(test)]
macro_rules! metavariable {
    ($line:literal, $written:literal, $reaches:expr) => {
        concat!(
            "crates/protocol/src/lib.rs:", $line, ": clauseo-protocol builds `", $written, "` from a \
             macro metavariable, which may expand to ", $reaches, "; core-io cannot see the \
             expansion, and ARCHITECTURE.md forbids those in this crate\n    fix: write the path \
             out in full instead of building it from a metavariable, or add \
             `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt"
        )
    };
}

/// Every banned path, as a glob of `std` lists them.
#[cfg(test)]
macro_rules! all_bans {
    () => {
        "std::fs, std::net, std::process, std::env, std::os::*::fs, std::os::*::net, \
         std::os::*::process, std::time::Instant::now, std::time::SystemTime::now"
    };
}

crate::checks::planted::cases! {
    // Each banned module, reached every way the brief and the reviews list.
    fs_use: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => tests::fs_use,
    fs_group: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => tests::fs_group,
    fs_nested: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => tests::fs_nested,
    fs_std_alias: FailsWith(&[uses!("lib.rs", 4, "std::fs")]) => tests::fs_std_alias,
    fs_leading_colon: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => tests::fs_leading_colon,
    fs_qualified: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => tests::fs_qualified,
    fs_extern_crate: FailsWith(&[uses!("lib.rs", 4, "std::fs")]) => tests::fs_extern_crate,
    fs_fn_body: FailsWith(&[uses!("lib.rs", 2, "std::fs"), uses!("lib.rs", 3, "std::fs")]) => tests::fs_fn_body,
    fs_inline_mod: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => tests::fs_inline_mod,
    fs_glob: FailsWith(&[glob!(1, "std", all_bans!())]) => tests::fs_glob,
    fs_macro: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => tests::fs_macro,
    fs_self_alias: FailsWith(&[uses!("lib.rs", 4, "std::fs")]) => tests::fs_self_alias,
    fs_rename: FailsWith(&[uses!("lib.rs", 1, "std::fs"), uses!("lib.rs", 4, "std::fs")]) => tests::fs_rename,
    fs_value: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => tests::fs_value,
    fs_raw: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => tests::fs_raw,
    net_use: FailsWith(&[uses!("lib.rs", 1, "std::net")]) => tests::net_use,
    net_group: FailsWith(&[uses!("lib.rs", 1, "std::net")]) => tests::net_group,
    net_nested: FailsWith(&[uses!("lib.rs", 1, "std::net")]) => tests::net_nested,
    net_std_alias: FailsWith(&[uses!("lib.rs", 4, "std::net")]) => tests::net_std_alias,
    net_leading_colon: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => tests::net_leading_colon,
    net_qualified: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => tests::net_qualified,
    net_extern_crate: FailsWith(&[uses!("lib.rs", 4, "std::net")]) => tests::net_extern_crate,
    net_fn_body: FailsWith(&[uses!("lib.rs", 2, "std::net"), uses!("lib.rs", 3, "std::net")]) => tests::net_fn_body,
    net_inline_mod: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => tests::net_inline_mod,
    net_glob: FailsWith(&[glob!(1, "std", all_bans!())]) => tests::net_glob,
    net_macro: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => tests::net_macro,
    net_self_alias: FailsWith(&[uses!("lib.rs", 4, "std::net")]) => tests::net_self_alias,
    net_rename: FailsWith(&[uses!("lib.rs", 1, "std::net"), uses!("lib.rs", 4, "std::net")]) => tests::net_rename,
    net_value: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => tests::net_value,
    net_raw: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => tests::net_raw,
    process_use: FailsWith(&[uses!("lib.rs", 1, "std::process")]) => tests::process_use,
    process_group: FailsWith(&[uses!("lib.rs", 1, "std::process")]) => tests::process_group,
    process_nested: FailsWith(&[uses!("lib.rs", 1, "std::process")]) => tests::process_nested,
    process_std_alias: FailsWith(&[uses!("lib.rs", 4, "std::process")]) => tests::process_std_alias,
    process_leading_colon: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => tests::process_leading_colon,
    process_qualified: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => tests::process_qualified,
    process_extern_crate: FailsWith(&[uses!("lib.rs", 4, "std::process")]) => tests::process_extern_crate,
    process_fn_body: FailsWith(&[uses!("lib.rs", 2, "std::process"), uses!("lib.rs", 3, "std::process")]) => tests::process_fn_body,
    process_inline_mod: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => tests::process_inline_mod,
    process_glob: FailsWith(&[glob!(1, "std", all_bans!())]) => tests::process_glob,
    process_macro: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => tests::process_macro,
    process_self_alias: FailsWith(&[uses!("lib.rs", 4, "std::process")]) => tests::process_self_alias,
    process_rename: FailsWith(&[uses!("lib.rs", 1, "std::process"), uses!("lib.rs", 4, "std::process")]) => tests::process_rename,
    process_value: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => tests::process_value,
    process_raw: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => tests::process_raw,
    env_use: FailsWith(&[uses!("lib.rs", 1, "std::env")]) => tests::env_use,
    env_group: FailsWith(&[uses!("lib.rs", 1, "std::env")]) => tests::env_group,
    env_nested: FailsWith(&[uses!("lib.rs", 1, "std::env")]) => tests::env_nested,
    env_std_alias: FailsWith(&[uses!("lib.rs", 4, "std::env")]) => tests::env_std_alias,
    env_leading_colon: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => tests::env_leading_colon,
    env_qualified: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => tests::env_qualified,
    env_extern_crate: FailsWith(&[uses!("lib.rs", 4, "std::env")]) => tests::env_extern_crate,
    env_fn_body: FailsWith(&[uses!("lib.rs", 2, "std::env"), uses!("lib.rs", 3, "std::env")]) => tests::env_fn_body,
    env_inline_mod: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => tests::env_inline_mod,
    env_glob: FailsWith(&[glob!(1, "std", all_bans!())]) => tests::env_glob,
    env_macro: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => tests::env_macro,
    env_self_alias: FailsWith(&[uses!("lib.rs", 4, "std::env")]) => tests::env_self_alias,
    env_rename: FailsWith(&[uses!("lib.rs", 1, "std::env"), uses!("lib.rs", 4, "std::env")]) => tests::env_rename,
    env_value: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => tests::env_value,
    env_raw: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => tests::env_raw,
    // Each banned clock.
    instant_use: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => tests::instant_use,
    instant_rename: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => tests::instant_rename,
    instant_type_alias: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => tests::instant_type_alias,
    instant_qualified: FailsWith(&[uses!("lib.rs", 2, "std::time::Instant::now")]) => tests::instant_qualified,
    instant_glob: FailsWith(&[glob!(1, "std::time", "std::time::Instant::now, std::time::SystemTime::now")]) => tests::instant_glob,
    instant_self_in_impl: FailsWith(&[uses!("lib.rs", 7, "std::time::Instant::now")]) => tests::instant_self_in_impl,
    instant_paren_alias: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => tests::instant_paren_alias,
    system_time_use: FailsWith(&[uses!("lib.rs", 4, "std::time::SystemTime::now")]) => tests::system_time_use,
    system_time_rename: FailsWith(&[uses!("lib.rs", 4, "std::time::SystemTime::now")]) => tests::system_time_rename,
    system_time_type_alias: FailsWith(&[uses!("lib.rs", 4, "std::time::SystemTime::now")]) => tests::system_time_type_alias,
    system_time_qualified: FailsWith(&[uses!("lib.rs", 2, "std::time::SystemTime::now")]) => tests::system_time_qualified,
    system_time_glob: FailsWith(&[glob!(1, "std::time", "std::time::Instant::now, std::time::SystemTime::now")]) => tests::system_time_glob,
    system_time_self_in_impl: FailsWith(&[uses!("lib.rs", 7, "std::time::SystemTime::now")]) => tests::system_time_self_in_impl,
    system_time_paren_alias: FailsWith(&[uses!("lib.rs", 4, "std::time::SystemTime::now")]) => tests::system_time_paren_alias,
    /// Raw identifiers count without their `r#`.
    raw_std: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => tests::raw_std,
    raw_use: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => tests::raw_use,
    instant_raw_now: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => tests::instant_raw_now,
    /// An associated type bound to a clock, through `<Z as K>::C` and `Self::C`.
    associated_type_qualified: FailsWith(&[uses!("lib.rs", 14, "std::time::Instant::now")]) => tests::associated_type_qualified,
    associated_type_self: FailsWith(&[uses!("lib.rs", 11, "std::time::Instant::now")]) => tests::associated_type_self,
    /// The platform modules under `std::os`.
    os_unix_fs: FailsWith(&[uses!("lib.rs", 2, "std::os::unix::fs")]) => tests::os_unix_fs,
    os_unix_net: FailsWith(&[uses!("lib.rs", 1, "std::os::unix::net")]) => tests::os_unix_net,
    os_unix_process: FailsWith(&[uses!("lib.rs", 1, "std::os::unix::process")]) => tests::os_unix_process,
    os_windows_fs: FailsWith(&[uses!("lib.rs", 2, "std::os::windows::fs")]) => tests::os_windows_fs,
    os_glob: FailsWith(&[glob!(1, "std::os", "std::os::*::fs, std::os::*::net, std::os::*::process")]) => tests::os_glob,
    os_unix_glob: FailsWith(&[glob!(1, "std::os::unix", "std::os::unix::fs, std::os::unix::net, std::os::unix::process")]) => tests::os_unix_glob,
    /// Paths a macro metavariable starts or continues.
    metavariable_module: FailsWith(&[metavariable!(3, "std::$x", all_bans!())]) => tests::metavariable_module,
    metavariable_now: FailsWith(&[metavariable!(3, "$t::now", "std::time::Instant::now, std::time::SystemTime::now")]) => tests::metavariable_now,
    metavariable_qualified_now: FailsWith(&[metavariable!(3, "$t::now", "std::time::Instant::now, std::time::SystemTime::now")]) => tests::metavariable_qualified_now,
    /// Bindings made inside macro bodies that parse.
    macro_block_use: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => tests::macro_block_use,
    macro_rules_item: FailsWith(&[uses!("lib.rs", 10, "std::time::Instant::now")]) => tests::macro_rules_item,
    /// `extern crate self as me;` names the crate root.
    extern_crate_self: FailsWith(&[uses!("lib.rs", 5, "std::fs")]) => tests::extern_crate_self,
    /// Each crate root has its own `crate::`.
    bin_crate_root: FailsWith(&[uses!("bin/x.rs", 5, "std::fs")]) => tests::bin_crate_root,
    main_crate_root: FailsWith(&[uses!("m.rs", 2, "std::fs")]) => tests::main_crate_root,
    /// Sites across files and lines.
    use_then_call: FailsWith(&[uses!("lib.rs", 1, "std::fs"), uses!("lib.rs", 4, "std::fs")]) => tests::use_then_call,
    alias_in_another_file: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => tests::alias_in_another_file,
    std_reexported_from_the_root: FailsWith(&[uses!("a.rs", 2, "std::fs")]) => tests::std_reexported_from_the_root,
    /// Code the check would not parse fails on its own.
    path_attribute: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol loads module `io` from `../io.rs` \
         with #[path], and core-io parses only the module files Rust finds without it
    \
         fix: drop #[path] and keep the module as `io.rs` or `io/mod.rs` beside its \
         parent module under `crates/protocol/src/`, so core-io parses it",
    ]) => tests::path_attribute,
    include_macro: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol pulls in code with include!, \
         which core-io does not parse\n    \
         fix: turn the included file into a module under \
         `crates/protocol/src/` (`mod name;`) instead of include!, so core-io parses it",
    ]) => tests::include_macro,
    lib_path_outside_src: FailsWith(&[
        "crates/protocol/Cargo.toml:5: clauseo-protocol's [lib] path `lib.rs` is outside \
         `crates/protocol/src/`, and core-io parses only that folder\n    \
         fix: move the \
         target's root file under `crates/protocol/src/` and set `path` to it",
    ]) => tests::lib_path_outside_src,
    bin_path_outside_src: FailsWith(&[
        "crates/protocol/Cargo.toml:6: clauseo-protocol's [[bin]] path `tools/main.rs` is \
         outside `crates/protocol/src/`, and core-io parses only that folder\n    \
         fix: \
         move the target's root file under `crates/protocol/src/` and set `path` to it",
    ]) => tests::bin_path_outside_src,
    /// Problems with the inputs.
    does_not_parse: FailsWith(&[
        "crates/protocol/src/lib.rs:3: `crates/protocol/src/lib.rs` does not parse as \
         Rust (cannot parse string into token stream), so core-io cannot check it
    \
         fix: fix the syntax; if rustc accepts the file, report that syn cannot parse it, \
         so xtask can be fixed",
    ]) => tests::does_not_parse,
    no_sources: FailsWith(&[
        "ARCHITECTURE.md:8: clauseo-protocol has no .rs file under \
         `crates/protocol/src/`, so core-io cannot check that it stays free of std I/O \
         (Never depends on: fs, process)\n    \
         fix: keep clauseo-protocol's Rust sources \
         under `crates/protocol/src/`, where core-io parses them",
    ]) => tests::no_sources,
    stale_exception: FailsWith(&[
        "xtask/core-io-exceptions.txt:2: `crates/protocol/src/gone.rs` is not a file in \
         this repo, so its core-io exception is stale\n    \
         fix: delete this line, or \
         correct the path if the file moved",
    ]) => tests::stale_exception,
    exception_outside_held_crates: FailsWith(&[
        "xtask/core-io-exceptions.txt:1: `xtask/core-io-exceptions.txt` is in no crate \
         whose row bans I/O, so its core-io exception is stale\n    \
         fix: delete this line",
    ]) => tests::exception_outside_held_crates,
    /// `std::time::Duration` and `std::io::Error` are allowed.
    allowed_items: Passes => tests::allowed_items,
    /// `Instant` named only as a type is allowed.
    instant_as_a_type: Passes => tests::instant_as_a_type,
    /// A file listed in the exception file is not checked.
    listed_exception: Passes => tests::listed_exception,
    /// A local `fs` module and another crate's `time::Instant` are not std's.
    local_names_like_banned_ones: Passes => tests::local_names_like_banned_ones,
}

#[cfg(test)]
mod tests;
