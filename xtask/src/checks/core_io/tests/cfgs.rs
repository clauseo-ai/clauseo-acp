//! The core-io planted cases about the cfg rule: a held crate's cfgs use only `test` and positive
//! features, and its files have no metavariable attribute or macro name, `path` attribute,
//! `include!` or symlink.

use super::{FORBIDDING_ROOT, LIB_ROOT, Repo, expected, run_repo};
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

/// A name and a `path` attribute in one `cfg_attr`.
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

/// The review's case: `--all-features` lints only the positive side of a feature.
pub(in crate::checks::core_io) fn cfg_not_feature() -> Inspection {
    with_file(
        "cfg-not-feature",
        "pub fn f() {}\n#[cfg(not(feature = \"x\"))]\npub fn g() {}\n",
    )
}

/// The review's case: clippy lints an unwinding profile, and the release profile aborts.
pub(in crate::checks::core_io) fn cfg_panic_abort() -> Inspection {
    with_file("cfg-panic", "#[cfg(panic = \"abort\")]\npub fn g() {}\n")
}

pub(in crate::checks::core_io) fn cfg_unix() -> Inspection {
    with_file("cfg-unix", "#[cfg(any(test, unix))]\npub fn g() {}\n")
}

pub(in crate::checks::core_io) fn cfg_custom_name() -> Inspection {
    with_file(
        "cfg-custom",
        "#[cfg(all(feature = \"a\", hidden))]\npub fn g() {}\n",
    )
}

pub(in crate::checks::core_io) fn cfg_macro_debug_assertions() -> Inspection {
    with_file(
        "cfg-macro-debug",
        "pub fn g() -> bool {\n    cfg!(debug_assertions)\n}\n",
    )
}

pub(in crate::checks::core_io) fn cfg_attr_not_feature() -> Inspection {
    with_file(
        "cfg-attr-not-feature",
        "#[cfg_attr(not(feature = \"x\"), derive(Debug))]\npub struct S;\n",
    )
}

/// A predicate a macro takes from its caller.
pub(in crate::checks::core_io) fn cfg_metavariable_in_a_macro_body() -> Inspection {
    with_file(
        "cfg-metavariable",
        "macro_rules! m {\n    ($p:meta) => {\n        #[cfg($p)]\n        pub fn g() {}\n    \
         };\n}\n",
    )
}

/// An attribute name a macro takes from its caller, which `m!(cfg)` makes a cfg.
pub(in crate::checks::core_io) fn attribute_name_metavariable() -> Inspection {
    with_file(
        "attr-metavariable",
        "macro_rules! m {\n    ($a:ident) => {\n        #[$a(not(clippy))]\n        pub fn g() {}\n    \
         };\n}\nm!(cfg);\n",
    )
}

/// The review's case: a module file that is not `.rs`, which core-io would not lex.
pub(in crate::checks::core_io) fn path_attribute() -> Inspection {
    with_file("path-attr", "#[path = \"x.txt\"]\nmod x;\n")
}

pub(in crate::checks::core_io) fn path_attribute_in_cfg_attr() -> Inspection {
    with_file(
        "path-cfg-attr",
        "#[cfg_attr(test, path = \"y.rs\")]\nmod y;\n",
    )
}

pub(in crate::checks::core_io) fn include_macro() -> Inspection {
    with_file("include-macro", "pub fn f() {}\ninclude!(\"x.rs\");\n")
}

/// The review's case: a symlinked folder brings another crate's module into the held crate,
/// where `mod sub;` compiles it. git lists the symlink as one entry.
pub(in crate::checks::core_io) fn symlinked_folder() -> Inspection {
    let crate_file = expected("symlink").text;
    let lib = format!("{FORBIDDING_ROOT}mod sub;\n");
    run_repo(
        "symlink",
        &Repo {
            crate_file: Some(&crate_file),
            files: &[
                (LIB_ROOT, &lib),
                (
                    "crates/extra/src/sub/mod.rs",
                    "#[cfg(panic = \"abort\")]\npub fn g() {}\n",
                ),
            ],
            folder_links: &[("crates/protocol/src/sub", "../../extra/src/sub")],
            ..Repo::default()
        },
    )
}

/// A macro that calls `include!` under a name passed to it.
pub(in crate::checks::core_io) fn macro_name_metavariable() -> Inspection {
    with_file(
        "macro-metavariable",
        "macro_rules! m {\n    ($i:ident) => {\n        $i!(\"x.txt\");\n    };\n}\nm!(include);\n",
    )
}

/// Items named `cfg` and `cfg_attr` are not cfgs.
pub(in crate::checks::core_io) fn items_named_cfg() -> Inspection {
    with_file(
        "items-named-cfg",
        "pub fn cfg(x: u8) -> u8 {\n    x\n}\npub fn cfg_attr() -> u8 {\n    cfg(1)\n}\n",
    )
}

pub(in crate::checks::core_io) fn source_not_lexed() -> Inspection {
    with_file("not-lexed", "pub fn f() {}\n\npub fn g() {\n")
}

/// `test`, positive features, `include_str!`, and `clippy` in lint paths, comments and strings,
/// pass.
pub(in crate::checks::core_io) fn cfg_rule_passes() -> Inspection {
    with_file(
        "cfg-rule-passes",
        "//! Not behind cfg(clippy).\n\
         #![forbid(clippy::disallowed_methods, clippy::disallowed_types)]\n\
         #[cfg(test)]\nmod tests {}\n#[cfg(not(test))]\npub fn f() {}\n\
         #[cfg_attr(feature = \"specta\", derive(Debug))]\npub struct S;\n\
         #[cfg(all(test, any(feature = \"a\", feature = \"b\")))]\npub fn g() {}\n\
         pub const DATA: &str = include_str!(\"data.txt\");\n\
         #[expect(clippy :: needless_return, reason = \"x\")]\npub fn h() -> &'static str {\n    \
         return \"clippy\";\n}\n",
    )
}
