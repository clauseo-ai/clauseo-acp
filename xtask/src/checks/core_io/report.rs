//! The text of every core-io violation, so each message and its fix live side by side.

use super::EXCEPTIONS;
use super::sites::{Reach, Site};
use crate::checks::crates_table;
use crate::checks::exceptions::Exception;
use crate::checks::violation;
use crate::violation::Violation;

/// The violation for one site in the file `path` of `package`, whose sources are in `src`.
pub(super) fn site(path: &str, package: &str, src: &str, site: Site) -> Violation {
    let except = format!("or add `{path} <reason>` to {EXCEPTIONS}");
    let (message, fix) = match site.reach {
        Reach::Uses(banned) => (
            format!("{package} uses {banned}, which ARCHITECTURE.md forbids in this crate"),
            format!(
                "take what {banned} gives as a parameter, or move this code to a crate that may \
                 do I/O, {except}"
            ),
        ),
        Reach::Glob { prefix, reaches } => (
            format!(
                "{package} glob-imports {prefix}::*, which reaches {}; ARCHITECTURE.md forbids \
                 that in this crate",
                reaches.join(", ")
            ),
            format!("import the items you use from {prefix} by name instead, {except}"),
        ),
        Reach::Metavariable { written, reaches } => (
            format!(
                "{package} builds `{written}` from a macro metavariable, which may expand to {}; \
                 core-io cannot see the expansion, and ARCHITECTURE.md forbids those in this \
                 crate",
                reaches.join(", ")
            ),
            format!(
                "write the path out in full instead of building it from a metavariable, {except}"
            ),
        ),
        Reach::PathAttribute { module, path: file } => (
            match file {
                Some(file) => format!(
                    "{package} loads module `{module}` from `{file}` with #[path], and core-io \
                     parses only the module files Rust finds without it"
                ),
                None => format!(
                    "{package} loads module `{module}` with #[path], and core-io parses only the \
                     module files Rust finds without it"
                ),
            },
            format!(
                "drop #[path] and keep the module as `{module}.rs` or `{module}/mod.rs` beside \
                 its parent module under `{src}/`, so core-io parses it"
            ),
        ),
        Reach::Include => (
            format!("{package} pulls in code with include!, which core-io does not parse"),
            format!(
                "turn the included file into a module under `{src}/` (`mod name;`) instead of \
                 include!, so core-io parses it"
            ),
        ),
        Reach::UnreadableRule => (
            format!(
                "{package} has a macro_rules! rule whose body core-io cannot parse, even with each \
                 metavariable replaced by a placeholder of its kind, so core-io cannot check it"
            ),
            format!(
                "restructure the rule so its body is items, statements, expressions or a type \
                 (move the rest into a helper rule or a function), {except}"
            ),
        ),
    };
    violation(path, site.line, message, fix)
}

/// The violation for `path`, which `syn` could not parse (the error at `line`).
pub(super) fn unparsed(path: &str, line: u32, error: &syn::Error) -> Violation {
    violation(
        path,
        line,
        format!("`{path}` does not parse as Rust ({error}), so core-io cannot check it"),
        "fix the syntax; if rustc accepts the file, report that syn cannot parse it, so xtask \
         can be fixed",
    )
}

/// The violation for an exception whose path is not a file.
pub(super) fn not_a_file(entry: &Exception) -> Violation {
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

/// The violation for an exception whose file is in no crate that core-io checks.
pub(super) fn outside_held_crates(entry: &Exception) -> Violation {
    violation(
        EXCEPTIONS,
        entry.line,
        format!(
            "`{}` is in no crate whose row bans I/O, so its core-io exception is stale",
            entry.path
        ),
        "delete this line",
    )
}

/// The violation at the crate's row in `ARCHITECTURE.md`, for a problem with its sources.
pub(super) fn at_row(line: u32, message: String, fix: String) -> Violation {
    violation(crates_table::FILE, line, message, fix)
}
