//! The text of every core-io violation, so each message and its fix live side by side.

use super::clippy_toml::{LIST, ROOT};
use crate::checks::{crates_table, violation};
use crate::violation::Violation;

/// The fix's lead-in before the file to write at `path`.
fn write_this(path: &str, text: &str) -> String {
    format!(
        "write `{path}` with exactly this text, the root {ROOT} with the arrays of {LIST} \
         appended (clippy reads only the nearest clippy.toml):\n{text}"
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
        write_this(path, text),
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
        write_this(path, text),
    )
}

/// The violation for a crate's `clippy.toml` at `path`, which is not TOML (the error at `line`).
pub(super) fn crate_file_not_toml(
    path: &str,
    line: u32,
    error: &toml::de::Error,
    text: &str,
) -> Violation {
    let first = error.message().lines().next().unwrap_or_default().trim();
    violation(
        path,
        line,
        format!("`{path}` is not valid TOML ({first})"),
        write_this(path, text),
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
    } else {
        "fix the line above so clippy and core-io can read the file"
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
