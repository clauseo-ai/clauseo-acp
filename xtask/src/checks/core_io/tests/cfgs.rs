//! The core-io planted cases about the cfgs clippy always sets: `clippy` and `debug_assertions`
//! in a held crate's files.

use super::{Repo, expected, run_repo};
use crate::checks::Inspection;

/// A file of `clauseo-protocol` beside its library root.
const FILE: &str = "crates/protocol/src/hidden.rs";

/// Runs the check with the generated `crates/protocol/clippy.toml` and `text` as `FILE`.
fn with_file(case: &str, text: &str) -> Inspection {
    let crate_file = expected(case).text;
    run_repo(
        case,
        &Repo {
            crate_file: Some(&crate_file),
            files: &[(FILE, text)],
            ..Repo::default()
        },
    )
}

/// The review's case: a banned call clippy never sees.
pub(in crate::checks::core_io) fn cfg_not_clippy() -> Inspection {
    with_file(
        "cfg-not-clippy",
        "#[cfg(not(clippy))]\npub fn g() {\n    let _ = std::fs::read(\"x\");\n}\n",
    )
}

/// The review's case: code clippy never sees, since it lints in the debug profile.
pub(in crate::checks::core_io) fn cfg_not_debug_assertions() -> Inspection {
    with_file(
        "cfg-not-debug",
        "pub fn f() {}\n\n#[cfg(not(debug_assertions))]\npub fn g() {}\n",
    )
}

pub(in crate::checks::core_io) fn cfg_attr_clippy() -> Inspection {
    with_file(
        "cfg-attr-clippy",
        "#[cfg_attr(clippy, path = \"other.rs\")]\nmod inner;\n",
    )
}

/// The cfg in a `macro_rules!` body, and a raw identifier.
pub(in crate::checks::core_io) fn cfg_in_a_macro_body() -> Inspection {
    with_file(
        "cfg-macro",
        "macro_rules! m {\n    () => {\n        #[cfg(not(r#clippy))]\n        pub fn g() {}\n    \
         };\n}\nm!();\n",
    )
}

pub(in crate::checks::core_io) fn source_not_lexed() -> Inspection {
    with_file("not-lexed", "pub fn f() {}\n\npub fn g() {\n")
}

/// `clippy::` in lint paths, and `clippy` in comments and strings, passes.
pub(in crate::checks::core_io) fn clippy_only_in_lint_paths() -> Inspection {
    with_file(
        "clippy-lint-paths",
        "//! Not behind cfg(clippy).\n#![expect(clippy::too_many_lines, reason = \"a table\")]\n\
         #[expect(clippy :: needless_return, reason = \"x\")]\npub fn f() -> &'static str {\n    \
         return \"clippy\";\n}\n",
    )
}
