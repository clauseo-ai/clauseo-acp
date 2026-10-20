//! The core-io planted cases about the cfg rule: a held crate's cfgs use only `test` and positive
//! features, and its files have no metavariable attribute or macro name, `path` attribute,
//! `include!` or symlink.

use super::{FORBIDDING_ROOT, LIB_ROOT, Repo, expected, run_repo};
use crate::checks::planted::Repo as ScratchRepo;

/// A file of `clauseo-protocol` beside its library root.
const FILE: &str = "crates/protocol/src/hidden.rs";

/// The repo with the generated `crates/protocol/clippy.toml` and `text` as `FILE`.
pub(super) fn with_file(case: &str, text: &str) -> ScratchRepo {
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
pub(in crate::checks::core_io) fn cfg_not_clippy() -> ScratchRepo {
    with_file(
        "cfg-not-clippy",
        "#[cfg(not(clippy))]\npub fn g() {\n    let _ = std::fs::read(\"x\");\n}\n",
    )
}

/// The review's case: code clippy never sees, since it lints in the debug profile.
pub(in crate::checks::core_io) fn cfg_not_debug_assertions() -> ScratchRepo {
    with_file(
        "cfg-not-debug",
        "pub fn f() {}\n\n#[cfg(not(debug_assertions))]\npub fn g() {}\n",
    )
}

/// A name and a `path` attribute in one `cfg_attr`.
pub(in crate::checks::core_io) fn cfg_attr_clippy() -> ScratchRepo {
    with_file(
        "cfg-attr-clippy",
        "#[cfg_attr(clippy, path = \"other.rs\")]\nmod inner;\n",
    )
}

/// The cfg in a `macro_rules!` body, and a raw identifier.
pub(in crate::checks::core_io) fn cfg_in_a_macro_body() -> ScratchRepo {
    with_file(
        "cfg-macro",
        "macro_rules! m {\n    () => {\n        #[cfg(not(r#clippy))]\n        pub fn g() {}\n    \
         };\n}\nm!();\n",
    )
}

/// The review's case: `--all-features` lints only the positive side of a feature.
pub(in crate::checks::core_io) fn cfg_not_feature() -> ScratchRepo {
    with_file(
        "cfg-not-feature",
        "pub fn f() {}\n#[cfg(not(feature = \"x\"))]\npub fn g() {}\n",
    )
}

/// The review's case: clippy lints an unwinding profile, and the release profile aborts.
pub(in crate::checks::core_io) fn cfg_panic_abort() -> ScratchRepo {
    with_file("cfg-panic", "#[cfg(panic = \"abort\")]\npub fn g() {}\n")
}

pub(in crate::checks::core_io) fn cfg_unix() -> ScratchRepo {
    with_file("cfg-unix", "#[cfg(any(test, unix))]\npub fn g() {}\n")
}

pub(in crate::checks::core_io) fn cfg_custom_name() -> ScratchRepo {
    with_file(
        "cfg-custom",
        "#[cfg(all(feature = \"a\", hidden))]\npub fn g() {}\n",
    )
}

/// The review's case: `cfg_select!` as an expression, where clippy's build takes the `clippy`
/// arm and rustc builds the `_` arm.
pub(in crate::checks::core_io) fn cfg_select_expression() -> ScratchRepo {
    with_file(
        "cfg-select-expression",
        "pub fn g() {\n    std::cfg_select! {\n        clippy => {}\n        _ => {\n            \
         let _ = std::fs::read(\"x\");\n        }\n    }\n}\n",
    )
}

/// The review's case: `cfg_select!` at item level.
pub(in crate::checks::core_io) fn cfg_select_items() -> ScratchRepo {
    with_file(
        "cfg-select-items",
        "cfg_select! {\n    clippy => {}\n    _ => {\n        pub fn g() {}\n    }\n}\n",
    )
}

/// The review's case: `cfg_select` under another name, which fails at the `use`.
pub(in crate::checks::core_io) fn cfg_select_alias() -> ScratchRepo {
    with_file(
        "cfg-select-alias",
        "use std::cfg_select as pick;\npick! {\n    clippy => {}\n    _ => {\n        \
         pub fn g() {}\n    }\n}\n",
    )
}

/// The review's case: a `cfg` inside a `cfg_attr` keeps the fn without the feature and drops it
/// under `--all-features`, the only build clippy sees.
pub(in crate::checks::core_io) fn cfg_inside_cfg_attr() -> ScratchRepo {
    with_file(
        "cfg-in-cfg-attr",
        "#[cfg_attr(feature = \"specta\", cfg(any()))]\npub fn g() {\n    \
         let _ = std::fs::read(\"x\");\n}\n",
    )
}

pub(in crate::checks::core_io) fn cfg_attr_not_feature() -> ScratchRepo {
    with_file(
        "cfg-attr-not-feature",
        "#[cfg_attr(not(feature = \"x\"), derive(Debug))]\npub struct S;\n",
    )
}

/// A predicate a macro takes from its caller.
pub(in crate::checks::core_io) fn cfg_metavariable_in_a_macro_body() -> ScratchRepo {
    with_file(
        "cfg-metavariable",
        "macro_rules! m {\n    ($p:meta) => {\n        #[cfg($p)]\n        pub fn g() {}\n    \
         };\n}\n",
    )
}

/// An attribute name a macro takes from its caller, which `m!(cfg)` makes a cfg.
pub(in crate::checks::core_io) fn attribute_name_metavariable() -> ScratchRepo {
    with_file(
        "attr-metavariable",
        "macro_rules! m {\n    ($a:ident) => {\n        #[$a(not(clippy))]\n        pub fn g() {}\n    \
         };\n}\nm!(cfg);\n",
    )
}

/// The review's case: a module file that is not `.rs`, which core-io would not lex.
pub(in crate::checks::core_io) fn path_attribute() -> ScratchRepo {
    with_file("path-attr", "#[path = \"x.txt\"]\nmod x;\n")
}

/// A `path` in a `cfg_attr` fails even when it names a file core-io lexes.
pub(in crate::checks::core_io) fn path_attribute_in_cfg_attr() -> ScratchRepo {
    with_file(
        "path-cfg-attr",
        "#[cfg_attr(test, path = \"lib.rs\")]\nmod y;\n",
    )
}

/// The review's case: the protocol crate's tests share a helper and split a test crate with a
/// literal `path` on a top-level `mod name;`, to files core-io lexes.
pub(in crate::checks::core_io) fn path_to_a_lexed_file_passes() -> ScratchRepo {
    let crate_file = expected("path-passes").text;
    let events = "#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]\n\
                  #[cfg(test)]\n#[path = \"support/fixtures.rs\"]\npub mod fixtures;\n\n\
                  #[cfg(test)]\n#[path = \"events/rules.rs\"]\nmod rules;\n\n\
                  #[path = \"../src/./hidden.rs\"]\npub(crate) mod hidden;\n";
    run_repo(
        "path-passes",
        &Repo {
            crate_file: Some(&crate_file),
            files: &[
                ("crates/protocol/tests/events.rs", events),
                (
                    "crates/protocol/tests/support/fixtures.rs",
                    "pub fn f() {}\n",
                ),
                (
                    "crates/protocol/tests/events/rules.rs",
                    "#[test]\nfn t() {}\n",
                ),
                (FILE, "pub fn g() {}\n"),
            ],
            targets: &[("test", "crates/protocol/tests/events.rs")],
            ..Repo::default()
        },
    )
}

/// A `path` whose value a macro takes from its caller, after a literal `=`.
pub(in crate::checks::core_io) fn path_from_a_macro_value() -> ScratchRepo {
    with_file(
        "path-macro-value",
        "macro_rules! p {\n    ($x:literal) => {\n        #[path = $x]\n        mod x;\n    \
         };\n}\np!(\"lib.rs\");\n",
    )
}

pub(in crate::checks::core_io) fn path_not_a_literal() -> ScratchRepo {
    with_file(
        "path-not-literal",
        "#[path = concat!(\"lib\", \".rs\")]\nmod x;\n",
    )
}

/// A path out of the crate's folder, to another crate's file that git lists.
pub(in crate::checks::core_io) fn path_leaving_the_crate() -> ScratchRepo {
    with_file(
        "path-leaving",
        "#[path = \"../../extra/src/lib.rs\"]\nmod x;\n",
    )
}

/// rustc resolves a `path` on an inline module, or on a module inside one, against another
/// folder, so both fail even when the literal names a file core-io lexes from here.
pub(in crate::checks::core_io) fn path_on_an_inline_module() -> ScratchRepo {
    with_file(
        "path-inline",
        "#[path = \"lib.rs\"]\nmod x {\n    pub fn f() {}\n}\n",
    )
}

pub(in crate::checks::core_io) fn path_inside_a_module_block() -> ScratchRepo {
    with_file(
        "path-module-block",
        "mod outer {\n    #[path = \"lib.rs\"]\n    mod inner;\n}\n",
    )
}

pub(in crate::checks::core_io) fn include_macro() -> ScratchRepo {
    with_file("include-macro", "pub fn f() {}\ninclude!(\"x.rs\");\n")
}

/// The review's case: a symlinked folder brings another crate's module into the held crate,
/// where `mod sub;` compiles it. git lists the symlink as one entry.
pub(in crate::checks::core_io) fn symlinked_folder() -> ScratchRepo {
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
pub(in crate::checks::core_io) fn macro_name_metavariable() -> ScratchRepo {
    with_file(
        "macro-metavariable",
        "macro_rules! m {\n    ($i:ident) => {\n        $i!(\"x.txt\");\n    };\n}\nm!(include);\n",
    )
}

/// The review's case: `#` and its brackets come into the macro body as separate tokens, so the
/// `#` in the transcriber fails.
pub(in crate::checks::core_io) fn attribute_split_across_a_macro() -> ScratchRepo {
    with_file(
        "attr-split",
        "macro_rules! a {\n    ($x:tt $($t:tt)*) => { #$x $($t)* };\n}\n\
         a! { [cfg(panic = \"abort\")] pub fn f() {} }\n",
    )
}

/// The review's case: the `#` comes in as a metavariable before the brackets, so the `#` in
/// the call fails.
pub(in crate::checks::core_io) fn hash_from_a_metavariable() -> ScratchRepo {
    with_file(
        "attr-hash-metavariable",
        "macro_rules! b {\n    ($h:tt $($t:tt)*) => { $h[cfg(panic = \"abort\")] $($t)* };\n}\n\
         b! { # pub fn f() {} }\n",
    )
}

/// The review's case: the `=` of a `path` attribute comes in as a metavariable.
pub(in crate::checks::core_io) fn path_with_a_metavariable() -> ScratchRepo {
    with_file(
        "path-metavariable",
        "macro_rules! p {\n    ($e:tt) => {\n        #[path $e \"x.txt\"]\n        mod x;\n    \
         };\n}\np!(=);\n",
    )
}

/// The review's case: the `!` of `include!` comes in as a metavariable.
pub(in crate::checks::core_io) fn include_with_a_metavariable() -> ScratchRepo {
    with_file(
        "include-metavariable",
        "macro_rules! i {\n    ($b:tt) => {\n        include $b (\"y.txt\");\n    };\n}\ni!(!);\n",
    )
}

/// Items named `cfg` and `cfg_attr` are not cfgs.
pub(in crate::checks::core_io) fn items_named_cfg() -> ScratchRepo {
    with_file(
        "items-named-cfg",
        "pub fn cfg(x: u8) -> u8 {\n    x\n}\npub fn cfg_attr() -> u8 {\n    cfg(1)\n}\n",
    )
}

/// The review's case: `doc` passes where `test` does, `not(doc)` included, as the usual import
/// for a doc link uses it.
pub(in crate::checks::core_io) fn cfg_doc_passes() -> ScratchRepo {
    with_file(
        "cfg-doc",
        "#[cfg(doc)]\nuse crate::FileRefId;\n#[cfg(not(doc))]\npub fn f() {}\n\
         #[cfg_attr(doc, derive(Debug))]\npub struct S;\n",
    )
}

/// A shebang first line, which rustc ignores, before code that passes.
pub(in crate::checks::core_io) fn shebang_line_passes() -> ScratchRepo {
    with_file(
        "shebang",
        "#!/usr/bin/env rust-script\n#[cfg(test)]\nmod tests {}\n",
    )
}

pub(in crate::checks::core_io) fn source_not_lexed() -> ScratchRepo {
    with_file("not-lexed", "pub fn f() {}\n\npub fn g() {\n")
}

/// `test`, positive features, `include_str!`, and `clippy` in lint paths, comments and strings,
/// pass.
pub(in crate::checks::core_io) fn cfg_rule_passes() -> ScratchRepo {
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
