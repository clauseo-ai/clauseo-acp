//! The text of every core-io violation, so each message and its fix live side by side.

use super::clippy_toml::{LIST, ROOT};
use crate::checks::{crates_table, violation};
use crate::violation::Violation;

/// The fix for `package`'s `clippy.toml` at `path`: what to write and how to make clippy read it.
fn write_this(path: &str, package: &str, text: &str) -> String {
    format!(
        "write `{path}` with exactly the text below, the root {ROOT} with the arrays of {LIST} \
         appended (clippy reads only the nearest clippy.toml), then run `cargo clean -p \
         {package}`: cargo does not lint a crate again when its clippy.toml appears after it \
         last linted it\n{text}"
    )
}

/// The violation at the row (line `line` of `ARCHITECTURE.md`) of `package`, whose
/// `clippy.toml` at `path` is missing; `words` are the I/O words of its row.
pub(super) fn missing_crate_file(
    line: u32,
    package: &str,
    words: &str,
    path: &str,
    text: &str,
) -> Violation {
    violation(
        crates_table::FILE,
        line,
        format!(
            "{package}'s row bans I/O (Never depends on: {words}), but `{path}` is missing, so \
             clippy does not ban the standard library's I/O in it"
        ),
        write_this(path, package, text),
    )
}

/// The violation for the `clippy.toml` at `path` of `package`, which parses but differs from
/// the root file plus the list in the keys `keys`.
pub(super) fn crate_file_differs(
    path: &str,
    package: &str,
    keys: &[String],
    text: &str,
) -> Violation {
    let keys: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
    violation(
        path,
        1,
        format!(
            "`{path}` of {package} is not the root {ROOT} plus {LIST}: it differs in {}",
            keys.join(", ")
        ),
        write_this(path, package, text),
    )
}

/// The violation for `package`'s `clippy.toml` at `path`, which is not TOML (the error at
/// `line`).
pub(super) fn crate_file_not_toml(
    path: &str,
    package: &str,
    line: u32,
    error: &toml::de::Error,
    text: &str,
) -> Violation {
    let first = error.message().lines().next().unwrap_or_default().trim();
    violation(
        path,
        line,
        format!("`{path}` is not valid TOML ({first})"),
        write_this(path, package, text),
    )
}

/// The violation for the root file or the list, missing at `rel`.
pub(super) fn missing_input(rel: &str) -> Violation {
    violation(
        rel,
        1,
        format!("`{rel}` is missing, and core-io builds each core crate's clippy.toml from it"),
        format!("restore `{rel}` from git"),
    )
}

/// The violation for the root file or the list at `rel`, which `toml` could not read.
pub(super) fn not_toml(rel: &str, line: u32, error: &toml::de::Error) -> Violation {
    let first = error.message().lines().next().unwrap_or_default().trim();
    let fix = if rel == LIST {
        "fix the line above: the file holds only `disallowed-methods` and `disallowed-types`, \
         arrays of `{ path, reason }` tables, each with an optional `replacement` and \
         `allow-invalid`"
    } else if rel == ROOT {
        "fix the line above so clippy and core-io can read the file"
    } else {
        "fix the line above so cargo and core-io can read the file"
    };
    violation(rel, line, format!("`{rel}` is not valid: {first}"), fix)
}

/// The violation for the root file's `key`, at `line`, which is not an array to append to.
pub(super) fn root_not_an_array(key: &str, line: u32) -> Violation {
    violation(
        ROOT,
        line,
        format!(
            "`{key}` in {ROOT} is not an array, so core-io cannot append {LIST}'s `{key}` to it"
        ),
        format!("write `{key}` as an array of `{{ path, reason }}` tables"),
    )
}

/// The violation for an entry of the list's `key` whose reason is empty.
pub(super) fn empty_reason(key: &str, path: &str, line: u32) -> Violation {
    violation(
        LIST,
        line,
        format!(
            "the `{key}` entry for `{path}` has an empty reason, which clippy prints with each hit"
        ),
        "say why a core crate may not use the item, and what to do instead",
    )
}

/// The violation for an entry of the list's `key` whose path an earlier entry already names.
pub(super) fn listed_twice(key: &str, path: &str, line: u32) -> Violation {
    violation(
        LIST,
        line,
        format!("`{path}` is listed twice in `{key}`"),
        "delete this entry",
    )
}

/// The violation for the clippy config at `path`, which is neither the root `clippy.toml` nor the
/// file of a crate whose row bans fs, process or any I/O.
pub(super) fn stray_config(path: &str) -> Violation {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let under = if dir.is_empty() {
        "every crate in the repo".to_owned()
    } else {
        format!("the crates under `{dir}/`")
    };
    let message = if name == ".clippy.toml" {
        format!(
            "`{path}` is a .clippy.toml, which clippy reads instead of a clippy.toml beside it or \
             above it, so it replaces the root {ROOT}'s bans, and a core crate's list, for {under}"
        )
    } else {
        format!(
            "`{path}` is a clippy.toml other than the root one and those core-io writes for the \
             crates whose row bans fs, process or any I/O; clippy reads only the nearest one, so it \
             replaces the root {ROOT}'s bans for {under}"
        )
    };
    violation(
        path,
        1,
        message,
        format!(
            "delete `{path}`: clippy settings go in the root {ROOT}, which every crate reads, and \
             core-io prints the file each crate whose row bans fs, process or any I/O needs"
        ),
    )
}

/// The violation for `package`'s manifest at `path`, which does not inherit the workspace lints:
/// it has a `[lints]` table without `workspace = true` (`has_table`, at `line`) or none.
pub(super) fn lints_not_inherited(
    path: &str,
    line: u32,
    package: &str,
    has_table: bool,
) -> Violation {
    let (message, fix) = if has_table {
        (
            format!(
                "`{path}` sets {package}'s own lints instead of `[lints] workspace = true`, so it \
                 can turn off clippy's disallowed_methods and disallowed_types, which ban I/O here"
            ),
            "replace the table with `[lints]` and `workspace = true` (cargo takes no other lint \
             key beside it); an exception is `#[expect(clippy::disallowed_methods, reason = \"..\")]` \
             at the call",
        )
    } else {
        (
            format!(
                "`{path}` has no `[lints] workspace = true`, so {package} does not get the \
                 workspace's lint levels, which keep clippy's disallowed_methods and disallowed_types \
                 reporting I/O here"
            ),
            "add `[lints]` with `workspace = true`",
        )
    };
    violation(path, line, message, fix)
}

/// The violation for the root manifest's `[workspace.lints.clippy]` entry `name`, at `line`,
/// whose level `level` does not report.
pub(super) fn workspace_lint_level(path: &str, line: u32, name: &str, level: &str) -> Violation {
    let lint = name.replace('-', "_");
    violation(
        path,
        line,
        format!(
            "`[workspace.lints.clippy]` sets `{name}` to `{level}`, so clippy does not report \
             the calls {LIST} bans in the crates whose row bans I/O"
        ),
        format!(
            "set `{name}` to \"warn\" (CI denies warnings) or \"deny\", or remove it; an \
             exception is `#[expect(clippy::{lint}, reason = \"..\")]` at the call"
        ),
    )
}

/// The violation for `flag` in `key` of the cargo config at `path`, at `line`, which names
/// one of the lints the list feeds.
pub(super) fn rustflag_names_a_ban(path: &str, line: u32, key: &str, flag: &str) -> Violation {
    violation(
        path,
        line,
        format!(
            "`{key}` passes `{flag}` to rustc, which sets the level of a lint {LIST} feeds for \
             every crate, past [workspace.lints]"
        ),
        "remove the flag: the lint levels live in the root Cargo.toml's [workspace.lints.clippy], \
         and an exception is `#[expect(clippy::disallowed_methods, reason = \"..\")]` at the call",
    )
}
