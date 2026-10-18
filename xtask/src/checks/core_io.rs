//! The `core-io` check: a crate whose Crates-table row in `ARCHITECTURE.md` lists `fs`, `process`
//! or `any I/O` under "Never depends on" must not reach the standard library's file, network,
//! process, environment or clock APIs.
//!
//! For every workspace member whose row holds core I/O, it parses each `.rs` file under the
//! crate's `src/` with `syn`, resolves every name through the crate's `use` items, `type`
//! aliases and `extern crate` renames (`names.rs`), and reports each `use` leaf, path, qualified
//! path and macro token chain that reaches a banned path (`sites.rs`): `std::fs`, `std::net`,
//! `std::process`, `std::env` (inside them or naming them), `std::time::Instant::now` and
//! `std::time::SystemTime::now`. A glob import that could reach one fails on its own.
//!
//! Exceptions are whole files, listed in `xtask/core-io-exceptions.txt` (see `exceptions.rs`). It
//! counts the Crates-table rows it classified, held or not.

mod names;
mod sites;

use crate::checks::crates_table::{self, CrateRow, IoWord, Never};
use crate::checks::exceptions::{self, Exception};
use crate::checks::{self, Inspection, Noun, StaticCheck, violation};
use crate::violation::Violation;
use crate::workspace::{Context, Member};
use sites::Reach;
use std::fs;
use std::io;
use std::path::Path;

/// The exception file, repo-relative.
pub(crate) const EXCEPTIONS: &str = "xtask/core-io-exceptions.txt";

/// Fails on every place a crate that must not do I/O reaches the standard library's I/O.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "core-io",
    counts: Noun {
        one: "Crates table row",
        many: "Crates table rows",
    },
    place: "in the Crates table of ARCHITECTURE.md",
    run,
};

fn run(ctx: &Context) -> Inspection {
    let table = match crates_table::load(&ctx.root) {
        Ok(table) => table,
        Err(error) => return Inspection::new(0, error.into_violations()),
    };
    let (listed, mut violations) = exceptions::load(&ctx.root, EXCEPTIONS);
    violations.extend(
        listed
            .iter()
            .filter(|entry| !ctx.root.join(&entry.path).is_file())
            .map(stale),
    );
    for member in &ctx.members {
        if let Some(row) = table.row_for_package(&member.name)
            && row.holds_core_io()
        {
            violations.extend(check_crate(&ctx.root, member, row, &listed));
        }
    }
    Inspection::new(table.rows.len(), violations)
}

/// The violation for an exception whose path is not a file.
fn stale(entry: &Exception) -> Violation {
    violation(
        EXCEPTIONS,
        entry.line,
        format!(
            "`{}` is not a file in this repo, so its core-io exception is stale",
            entry.path
        ),
        "delete this line, or correct the path if the file moved",
    )
}

/// One parsed source file of a held crate.
struct Source {
    /// Repo-relative.
    path: String,
    module: names::Module,
    file: syn::File,
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
    let files = match rust_files(root, &src) {
        Ok(files) => files,
        Err(error) => {
            return vec![violation(
                crates_table::FILE,
                row.line,
                format!("could not list `{src}/` of {package}: {error}"),
                format!("make `{src}/` readable: core-io parses every .rs file in it"),
            )];
        }
    };
    if files.is_empty() {
        return vec![violation(
            crates_table::FILE,
            row.line,
            format!(
                "{package} has no .rs file under `{src}/`, so core-io cannot check that it stays \
                 free of std I/O (Never depends on: {})",
                held_word(row)
            ),
            format!("keep {package}'s Rust sources under `{src}/`, where core-io parses them"),
        )];
    }
    let exempt = |path: &str| listed.iter().any(|entry| entry.path == path);
    let mut violations = Vec::new();
    let mut sources = Vec::new();
    for (path, module) in files {
        let text = match checks::read_repo_file(root, &path) {
            Ok(Some(text)) => text,
            Ok(None) => continue,
            Err(unreadable) => {
                violations.push(unreadable);
                continue;
            }
        };
        match syn::parse_file(&text) {
            Ok(file) => sources.push(Source { path, module, file }),
            Err(error) if !exempt(&path) => {
                let line = u32::try_from(error.span().start().line)
                    .unwrap_or(u32::MAX)
                    .max(1);
                violations.push(violation(
                    &path,
                    line,
                    format!(
                        "`{path}` does not parse as Rust ({error}), so core-io cannot check it"
                    ),
                    format!(
                        "fix the syntax error; if rustc accepts the file, add `{path} <reason>` \
                         to {EXCEPTIONS}"
                    ),
                ));
            }
            Err(_) => {}
        }
    }
    let mut table = names::Table::default();
    for source in &sources {
        table.add_file(&source.module, &source.file);
    }
    for source in sources.iter().filter(|source| !exempt(&source.path)) {
        for site in sites::sites(&table, &source.module, &source.file) {
            violations.push(site_violation(&source.path, package, site));
        }
    }
    violations
}

/// The I/O word that makes `row` hold core I/O, as the table writes it.
fn held_word(row: &CrateRow) -> &'static str {
    row.never
        .iter()
        .find_map(|entry| match entry {
            Never::Io(word @ (IoWord::Fs | IoWord::Process | IoWord::AnyIo)) => Some(word.word()),
            _ => None,
        })
        .unwrap_or("fs")
}

/// The violation for one site in the file `path` of `package`.
fn site_violation(path: &str, package: &str, site: sites::Site) -> Violation {
    let except = format!("or add `{path} <reason>` to {EXCEPTIONS}");
    match site.reach {
        Reach::Uses(banned) => violation(
            path,
            site.line,
            format!("{package} uses {banned}, which ARCHITECTURE.md forbids in this crate"),
            format!(
                "take what {banned} gives as a parameter, or move this code to a crate that may \
                 do I/O, {except}"
            ),
        ),
        Reach::Glob { prefix, reaches } => violation(
            path,
            site.line,
            format!(
                "{package} glob-imports {prefix}::*, which reaches {}; ARCHITECTURE.md forbids \
                 that in this crate",
                reaches.join(", ")
            ),
            format!("import the items you use from {prefix} by name instead, {except}"),
        ),
    }
}

/// Every `.rs` file under the repo-relative directory `src` (recursively, sorted), with its
/// module path. A missing directory has none.
fn rust_files(root: &Path, src: &str) -> io::Result<Vec<(String, names::Module)>> {
    let mut found = Vec::new();
    let mut pending = vec![root.join(src)];
    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                let relative = checks::repo_path(&root.join(src), &path);
                found.push((checks::repo_path(root, &path), module_of(&relative)));
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The module path of a file at `relative` under `src/`: `lib.rs` and `main.rs` are the root,
/// `a/mod.rs` and `a.rs` are `a`.
fn module_of(relative: &str) -> names::Module {
    let mut parts: Vec<String> = relative
        .trim_end_matches(".rs")
        .split('/')
        .map(str::to_owned)
        .collect();
    let root_file = parts.len() == 1
        && parts
            .first()
            .is_some_and(|only| only == "lib" || only == "main");
    if root_file || parts.last().is_some_and(|last| last == "mod") {
        parts.pop();
    }
    parts
}

crate::checks::planted::cases! {
    // Each banned module, reached every way the brief lists.
    fs_use: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol uses std::fs, which ARCHITECTURE.md forbids in this crate\n    \
         fix: take what std::fs gives as a parameter, or move this code to a crate that may do I/O, or add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::fs_use,
    fs_group: Fails => tests::fs_group,
    fs_nested: Fails => tests::fs_nested,
    fs_std_alias: FailsWith(&[
        "crates/protocol/src/lib.rs:4: clauseo-protocol uses std::fs, which ARCHITECTURE.md forbids in this crate\n    \
         fix: take what std::fs gives as a parameter, or move this code to a crate that may do I/O, or add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::fs_std_alias,
    fs_leading_colon: Fails => tests::fs_leading_colon,
    fs_qualified: Fails => tests::fs_qualified,
    fs_extern_crate: Fails => tests::fs_extern_crate,
    fs_fn_body: Fails => tests::fs_fn_body,
    fs_inline_mod: Fails => tests::fs_inline_mod,
    fs_glob: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol glob-imports std::*, which reaches std::fs, std::net, std::process, std::env, std::time::Instant::now, std::time::SystemTime::now; ARCHITECTURE.md forbids that in this crate\n    \
         fix: import the items you use from std by name instead, or add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::fs_glob,
    fs_macro: FailsWith(&[
        "crates/protocol/src/lib.rs:2: clauseo-protocol uses std::fs, which ARCHITECTURE.md forbids in this crate\n    \
         fix: take what std::fs gives as a parameter, or move this code to a crate that may do I/O, or add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::fs_macro,
    fs_self_alias: Fails => tests::fs_self_alias,
    fs_rename: Fails => tests::fs_rename,
    fs_value: Fails => tests::fs_value,
    net_use: Fails => tests::net_use,
    net_group: Fails => tests::net_group,
    net_nested: Fails => tests::net_nested,
    net_std_alias: Fails => tests::net_std_alias,
    net_leading_colon: Fails => tests::net_leading_colon,
    net_qualified: Fails => tests::net_qualified,
    net_extern_crate: Fails => tests::net_extern_crate,
    net_fn_body: Fails => tests::net_fn_body,
    net_inline_mod: Fails => tests::net_inline_mod,
    net_glob: Fails => tests::net_glob,
    net_macro: Fails => tests::net_macro,
    net_self_alias: Fails => tests::net_self_alias,
    net_rename: Fails => tests::net_rename,
    net_value: Fails => tests::net_value,
    process_use: Fails => tests::process_use,
    process_group: Fails => tests::process_group,
    process_nested: Fails => tests::process_nested,
    process_std_alias: Fails => tests::process_std_alias,
    process_leading_colon: Fails => tests::process_leading_colon,
    process_qualified: Fails => tests::process_qualified,
    process_extern_crate: Fails => tests::process_extern_crate,
    process_fn_body: Fails => tests::process_fn_body,
    process_inline_mod: Fails => tests::process_inline_mod,
    process_glob: Fails => tests::process_glob,
    process_macro: Fails => tests::process_macro,
    process_self_alias: Fails => tests::process_self_alias,
    process_rename: Fails => tests::process_rename,
    process_value: Fails => tests::process_value,
    env_use: Fails => tests::env_use,
    env_group: Fails => tests::env_group,
    env_nested: Fails => tests::env_nested,
    env_std_alias: Fails => tests::env_std_alias,
    env_leading_colon: Fails => tests::env_leading_colon,
    env_qualified: Fails => tests::env_qualified,
    env_extern_crate: Fails => tests::env_extern_crate,
    env_fn_body: Fails => tests::env_fn_body,
    env_inline_mod: Fails => tests::env_inline_mod,
    env_glob: Fails => tests::env_glob,
    env_macro: Fails => tests::env_macro,
    env_self_alias: Fails => tests::env_self_alias,
    env_rename: Fails => tests::env_rename,
    env_value: Fails => tests::env_value,
    // Each banned clock, reached every way the brief lists.
    instant_use: Fails => tests::instant_use,
    instant_rename: Fails => tests::instant_rename,
    instant_type_alias: FailsWith(&[
        "crates/protocol/src/lib.rs:4: clauseo-protocol uses std::time::Instant::now, which ARCHITECTURE.md forbids in this crate\n    \
         fix: take what std::time::Instant::now gives as a parameter, or move this code to a crate that may do I/O, or add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::instant_type_alias,
    instant_qualified: Fails => tests::instant_qualified,
    instant_glob: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol glob-imports std::time::*, which reaches std::time::Instant::now, std::time::SystemTime::now; ARCHITECTURE.md forbids that in this crate\n    \
         fix: import the items you use from std::time by name instead, or add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::instant_glob,
    system_time_use: Fails => tests::system_time_use,
    system_time_rename: Fails => tests::system_time_rename,
    system_time_type_alias: Fails => tests::system_time_type_alias,
    system_time_qualified: Fails => tests::system_time_qualified,
    system_time_glob: Fails => tests::system_time_glob,
    /// A `use std::fs;` and a later `fs::read` are two sites.
    use_then_call: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol uses std::fs, which ARCHITECTURE.md forbids in this crate\n    \
         fix: take what std::fs gives as a parameter, or move this code to a crate that may do I/O, or add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
        "crates/protocol/src/lib.rs:4: clauseo-protocol uses std::fs, which ARCHITECTURE.md forbids in this crate\n    \
         fix: take what std::fs gives as a parameter, or move this code to a crate that may do I/O, or add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::use_then_call,
    /// A `type` alias of `Instant` in another file, called through its module path.
    alias_in_another_file: FailsWith(&[
        "crates/protocol/src/lib.rs:4: clauseo-protocol uses std::time::Instant::now, which ARCHITECTURE.md forbids in this crate\n    \
         fix: take what std::time::Instant::now gives as a parameter, or move this code to a crate that may do I/O, or add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::alias_in_another_file,
    /// `pub use std as s;` at the root, then `crate::s::fs` in another file.
    std_reexported_from_the_root: FailsWith(&[
        "crates/protocol/src/a.rs:2: clauseo-protocol uses std::fs, which ARCHITECTURE.md forbids in this crate\n    \
         fix: take what std::fs gives as a parameter, or move this code to a crate that may do I/O, or add `crates/protocol/src/a.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::std_reexported_from_the_root,
    /// A file that does not parse fails at the error's line.
    does_not_parse: FailsWith(&[
        "crates/protocol/src/lib.rs:3: `crates/protocol/src/lib.rs` does not parse as Rust (cannot parse string into token stream), so core-io cannot check it\n    \
         fix: fix the syntax error; if rustc accepts the file, add `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt",
    ]) => tests::does_not_parse,
    /// A held crate with no `.rs` file under its `src/` fails at its row.
    no_sources: FailsWith(&[
        "ARCHITECTURE.md:8: clauseo-protocol has no .rs file under `crates/protocol/src/`, so core-io cannot check that it stays free of std I/O (Never depends on: fs)\n    \
         fix: keep clauseo-protocol's Rust sources under `crates/protocol/src/`, where core-io parses them",
    ]) => tests::no_sources,
    /// An exception whose file does not exist fails at its line.
    stale_exception: FailsWith(&[
        "xtask/core-io-exceptions.txt:2: `crates/protocol/src/gone.rs` is not a file in this repo, so its core-io exception is stale\n    \
         fix: delete this line, or correct the path if the file moved",
    ]) => tests::stale_exception,
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
