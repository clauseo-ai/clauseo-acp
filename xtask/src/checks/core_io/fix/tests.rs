//! Pins the text of every `core_io::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::{CfgFix, Fix};
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::WriteCrateFile { .. } => {
            "write `crates/protocol/clippy.toml` with exactly the text below, the root clippy.toml \
             with the arrays of xtask/core-io-bans.toml appended (clippy reads only the nearest \
             clippy.toml), then run `cargo clean -p clauseo-protocol`: cargo does not lint a crate \
             again when its clippy.toml appears after it last linted it\na = 1"
        },
        Fix::RestoreInput { .. } => "restore `xtask/core-io-bans.toml` from git",
        Fix::ListNotToml => {
            "fix the line above: the file holds only `disallowed-methods` and `disallowed-types`, \
             arrays of `{ path, reason }` tables, each with an optional `replacement` and \
             `allow-invalid`"
        },
        Fix::RootNotToml => "fix the line above so clippy and core-io can read the file",
        Fix::CargoNotToml => "fix the line above so cargo and core-io can read the file",
        Fix::RootNotArray { .. } => {
            "write `disallowed-types` as an array of `{ path, reason }` tables"
        },
        Fix::EmptyReason => "say why a core crate may not use the item, and what to do instead",
        Fix::ListedTwice => "delete this entry",
        Fix::StrayConfig { .. } => {
            "delete `crates/protocol/.clippy.toml`: clippy settings go in the root clippy.toml, \
             which every crate reads, and core-io prints the file each crate whose row bans fs, \
             process or any I/O needs"
        },
        Fix::ReplaceLintsTable => {
            "replace the table with `[lints]` and `workspace = true`; cargo takes no other lint \
             key beside it"
        },
        Fix::AddLints => "add `[lints]` with `workspace = true`",
        Fix::RemoveRustflags => {
            "remove the key; set lint levels in the root Cargo.toml's [workspace.lints]"
        },
        Fix::RemoveInclude { .. } => {
            "remove `include` and move the settings it loads into `.cargo/config.toml` itself"
        },
        Fix::RemoveCompiler { .. } => {
            "remove `build.rustc-wrapper`; clippy runs the pinned toolchain's compiler as it is"
        },
        Fix::RemoveClippyEnv => "remove the entry; clippy settings go in the root clippy.toml",
        Fix::RemoveAlias { .. } => {
            "remove the `clippy` alias; an alias may not take the name of an external cargo \
             command xtask runs"
        },
        Fix::MoveLegacyConfig { .. } => {
            "move what `.cargo/config` holds into `.cargo/config.toml` and delete `.cargo/config`"
        },
        Fix::AddForbid { .. } => {
            "add this line to the inner attributes at the top of `crates/protocol/src/lib.rs`, \
             not under `cfg_attr`\n#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]"
        },
        Fix::MoveRoot { .. } => {
            "move `../x/lib.rs` under `crates/protocol/` and point the target's `path` in \
             `crates/protocol/Cargo.toml` at it"
        },
        Fix::RootNotRust { .. } => {
            "fix the syntax error, and make sure this line is among the inner attributes at the \
             top of `crates/protocol/src/lib.rs`\n#![forbid(clippy::disallowed_methods, \
             clippy::disallowed_types)]"
        },
        Fix::ReplaceSymlink => "replace the symlink with the file or folder it points to",
        Fix::SourceNotLexed => {
            "fix the Rust syntax that starts at this line, such as a bracket or a string left open"
        },
        Fix::DeleteBuildScript { .. } => {
            "delete `crates/protocol/build.rs`; a crate whose row bans I/O has no build script"
        },
        Fix::DeleteBuildKey => "delete the `build` key, and the script it names if there is one",
        Fix::Cfg(CfgFix::Predicate { .. }) => {
            "remove the name `unix` from this cfg; the cfgs of a crate whose row bans I/O use only \
             `test`, `doc` and features, joined by `all`, `any` and `not`, with no feature under a \
             `not`"
        },
        Fix::Cfg(CfgFix::MetavariableAttribute) => {
            "write the attribute's name in the macro body itself; an outer attribute may instead \
             come whole from the caller, bound as `#[$m:meta]` in the arm's matcher and written as \
             `#[$m]`, in a `macro_rules!` outside every other macro's body and arguments"
        },
        Fix::Cfg(CfgFix::MetavariableMacro) => {
            "write the called macro's name in the macro body itself"
        },
        Fix::Cfg(CfgFix::HashWithoutBrackets) => {
            "write the attribute's brackets right after its `#`, as `#[..]` or `#![..]`; to pass \
             an attribute to a macro, pass it whole, with its `#` and its brackets"
        },
        Fix::Cfg(CfgFix::PathAttribute) => {
            "give `#[path = \"..\"]` only to an out-of-line `mod name;` at the top level of a file, \
             outside every brace, bracket and parenthesis, with a string literal that names, \
             relative to this file's folder and with `/` between components, a `.rs` file under \
             the crate's folder that git lists and that is not a symlink, without leaving that \
             folder on the way; a `path` in a `cfg_attr`, on an inline module, inside a module \
             block or with a metavariable value always fails"
        },
        Fix::Cfg(CfgFix::Select) => {
            "rename a field, variable or import named `cfg_select`; for the macro, give each arm's \
             code its own `#[cfg(..)]` built from `test`, `doc` and positive features, or use `if \
             cfg!(..)`, which compiles both branches"
        },
        Fix::Cfg(CfgFix::Tt) => {
            "give the macros of a crate whose row bans I/O no `tt` fragment and use a typed \
             fragment such as `item`, `expr`, `ident`, `ty` or `meta`; rename an identifier named \
             `tt`"
        },
        Fix::Cfg(CfgFix::InCfgAttr) => {
            "write the condition as one `#[cfg(..)]` outside any `cfg_attr`, built from `test`, \
             `doc` and positive features"
        },
        Fix::Cfg(CfgFix::Include) => {
            "move the code into a module at its default path; use `include_str!` or \
             `include_bytes!` for data"
        },
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::WriteCrateFile {
            path: "crates/protocol/clippy.toml".to_owned(),
            package: "clauseo-protocol".to_owned(),
            text: "a = 1\n\n".to_owned(),
        },
        Fix::RestoreInput {
            rel: "xtask/core-io-bans.toml".to_owned(),
        },
        Fix::ListNotToml,
        Fix::RootNotToml,
        Fix::CargoNotToml,
        Fix::RootNotArray {
            key: "disallowed-types".to_owned(),
        },
        Fix::EmptyReason,
        Fix::ListedTwice,
        Fix::StrayConfig {
            path: "crates/protocol/.clippy.toml".to_owned(),
        },
        Fix::ReplaceLintsTable,
        Fix::AddLints,
        Fix::RemoveRustflags,
        Fix::RemoveInclude {
            path: ".cargo/config.toml".to_owned(),
        },
        Fix::RemoveCompiler {
            key: "build.rustc-wrapper".to_owned(),
        },
        Fix::RemoveClippyEnv,
        Fix::RemoveAlias {
            name: "clippy".to_owned(),
        },
        Fix::MoveLegacyConfig {
            legacy: ".cargo/config".to_owned(),
            config: ".cargo/config.toml".to_owned(),
        },
        Fix::AddForbid {
            path: "crates/protocol/src/lib.rs".to_owned(),
        },
        Fix::MoveRoot {
            manifest: "crates/protocol/Cargo.toml".to_owned(),
            path: "../x/lib.rs".to_owned(),
            dir: "crates/protocol".to_owned(),
        },
        Fix::RootNotRust {
            path: "crates/protocol/src/lib.rs".to_owned(),
        },
        Fix::ReplaceSymlink,
        Fix::SourceNotLexed,
        Fix::DeleteBuildScript {
            path: "crates/protocol/build.rs".to_owned(),
        },
        Fix::DeleteBuildKey,
        Fix::Cfg(CfgFix::Predicate {
            form: "the name `unix`".to_owned(),
        }),
        Fix::Cfg(CfgFix::MetavariableAttribute),
        Fix::Cfg(CfgFix::MetavariableMacro),
        Fix::Cfg(CfgFix::HashWithoutBrackets),
        Fix::Cfg(CfgFix::PathAttribute),
        Fix::Cfg(CfgFix::Select),
        Fix::Cfg(CfgFix::Tt),
        Fix::Cfg(CfgFix::InCfgAttr),
        Fix::Cfg(CfgFix::Include),
    ];
    assert_texts(&samples, pinned, ARMS);
}
