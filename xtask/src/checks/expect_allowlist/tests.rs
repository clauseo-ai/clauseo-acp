//! Tests of the expect-allowlist check: the exact text of each fix, and the fixes the planted
//! cases name. The planted cases are in `tests/planted.rs` and their builders in `tests/repos.rs`.

pub(super) mod planted;
mod repos;

use super::*;

/// The fix of an `expect` at `item` in `path` with no line for `lint` in the allowlist.
fn unlisted(path: &str, item: &str, lint: &str) -> Fix {
    Fix::Unlisted {
        path: path.to_owned(),
        item: item.to_owned(),
        lint: lint.to_owned(),
    }
}

/// The fix of an `allow` or `warn` (`level`) of `lint` at `item` in `path`; `inner` for `#![..]`.
fn lower(level: Level, path: &str, item: &str, lint: &str, inner: bool) -> Fix {
    Fix::Lower {
        level,
        path: path.to_owned(),
        item: item.to_owned(),
        lint: lint.to_owned(),
        inner,
    }
}
