//! The fix lines of the core-io check, one variant per failure kind. Each text is written once,
//! in `Display`; `fix/tests.rs` pins every variant's text. A variant carries only the values its
//! own text prints; a value that only the message prints stays in `report.rs`.

use super::clippy_toml::{LIST, ROOT};
use super::roots::forbid_line;
use std::fmt;

/// A fix line of the core-io check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A held crate's `clippy.toml` is missing, parses to other keys, or is not TOML: write
    /// `path` with exactly `text`, then clean `package`, since cargo does not lint it again.
    WriteCrateFile {
        path: String,
        package: String,
        text: String,
    },
    /// The root `clippy.toml` or the list is missing at `rel`.
    RestoreInput { rel: String },
    /// The list does not parse as TOML, or as its own shape.
    ListNotToml,
    /// The root `clippy.toml` does not parse as TOML.
    RootNotToml,
    /// A cargo file (a manifest or the cargo config) does not parse as TOML.
    CargoNotToml,
    /// The root file's `key` is not an array, so the list's entries cannot be appended to it.
    RootNotArray { key: String },
    /// A list entry has an empty reason.
    EmptyReason,
    /// A list entry names a path an earlier entry already names.
    ListedTwice,
    /// A clippy config at `path` that core-io does not write for any crate.
    StrayConfig { path: String },
    /// A manifest sets its own `[lints]` table instead of `workspace = true`.
    ReplaceLintsTable,
    /// A manifest has no `[lints] workspace = true`.
    AddLints,
    /// The cargo config sets a `rustflags` key.
    RemoveRustflags,
    /// The cargo config sets `include`.
    RemoveInclude { path: String },
    /// The cargo config sets a compiler key (`key`) under `[build]`.
    RemoveCompiler { key: String },
    /// The cargo config sets a `CLIPPY_` entry under `[env]`.
    RemoveClippyEnv,
    /// The cargo config defines an `[alias]` named `name`, an external cargo command.
    RemoveAlias { name: String },
    /// The legacy cargo config `legacy` exists beside `config`.
    MoveLegacyConfig { legacy: String, config: String },
    /// A target root lacks the forbid line outside any `cfg_attr`.
    AddForbid { path: String },
    /// A target root of a held crate lies outside the crate's folder.
    MoveRoot {
        manifest: String,
        path: String,
        dir: String,
    },
    /// A target root syn cannot parse.
    RootNotRust { path: String },
    /// A symlink under a held crate's folder.
    ReplaceSymlink,
    /// A file of a held crate does not lex as Rust.
    SourceNotLexed,
    /// A held crate has a build script `path`.
    DeleteBuildScript { path: String },
    /// A held crate's `[package]` sets `build`.
    DeleteBuildKey,
    /// A cfg predicate of a form the rule does not allow, described as `form`.
    CfgPredicate { form: String },
    /// An attribute whose name is a metavariable.
    CfgMetavariableAttribute,
    /// A macro call whose name is a metavariable.
    CfgMetavariableMacro,
    /// A `#` followed by neither brackets nor `!` and brackets.
    CfgHashWithoutBrackets,
    /// A `path` attribute.
    CfgPathAttribute,
    /// The name `cfg_select`.
    CfgSelect,
    /// The name `tt`.
    CfgTt,
    /// A `cfg` inside a `cfg_attr`.
    CfgInCfgAttr,
    /// A call of `include!`.
    CfgInclude,
}

impl fmt::Display for Fix {
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per fix kind, so each text is written once beside its variant"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::WriteCrateFile {
                path,
                package,
                text,
            } => write!(
                f,
                "write `{path}` with exactly the text below, the root {ROOT} with the arrays of \
                 {LIST} appended (clippy reads only the nearest clippy.toml), then run `cargo \
                 clean -p {package}`: cargo does not lint a crate again when its clippy.toml \
                 appears after it last linted it\n{}",
                text.trim_end_matches('\n')
            ),
            Fix::RestoreInput { rel } => write!(f, "restore `{rel}` from git"),
            Fix::ListNotToml => f.write_str(
                "fix the line above: the file holds only `disallowed-methods` and \
                 `disallowed-types`, arrays of `{ path, reason }` tables, each with an optional \
                 `replacement` and `allow-invalid`",
            ),
            Fix::RootNotToml => {
                f.write_str("fix the line above so clippy and core-io can read the file")
            }
            Fix::CargoNotToml => {
                f.write_str("fix the line above so cargo and core-io can read the file")
            }
            Fix::RootNotArray { key } => {
                write!(
                    f,
                    "write `{key}` as an array of `{{ path, reason }}` tables"
                )
            }
            Fix::EmptyReason => {
                f.write_str("say why a core crate may not use the item, and what to do instead")
            }
            Fix::ListedTwice => f.write_str("delete this entry"),
            Fix::StrayConfig { path } => write!(
                f,
                "delete `{path}`: clippy settings go in the root {ROOT}, which every crate reads, \
                 and core-io prints the file each crate whose row bans fs, process or any I/O \
                 needs"
            ),
            Fix::ReplaceLintsTable => f.write_str(
                "replace the table with `[lints]` and `workspace = true`; cargo takes no other \
                 lint key beside it",
            ),
            Fix::AddLints => f.write_str("add `[lints]` with `workspace = true`"),
            Fix::RemoveRustflags => f.write_str(
                "remove the key; set lint levels in the root Cargo.toml's [workspace.lints]",
            ),
            Fix::RemoveInclude { path } => write!(
                f,
                "remove `include` and move the settings it loads into `{path}` itself"
            ),
            Fix::RemoveCompiler { key } => write!(
                f,
                "remove `{key}`; clippy runs the pinned toolchain's compiler as it is"
            ),
            Fix::RemoveClippyEnv => {
                f.write_str("remove the entry; clippy settings go in the root clippy.toml")
            }
            Fix::RemoveAlias { name } => write!(
                f,
                "remove the `{name}` alias; an alias may not take the name of an external cargo \
                 command xtask runs"
            ),
            Fix::MoveLegacyConfig { legacy, config } => write!(
                f,
                "move what `{legacy}` holds into `{config}` and delete `{legacy}`"
            ),
            Fix::AddForbid { path } => write!(
                f,
                "add this line to the inner attributes at the top of `{path}`, not under \
                 `cfg_attr`\n{}",
                forbid_line()
            ),
            Fix::MoveRoot {
                manifest,
                path,
                dir,
            } => write!(
                f,
                "move `{path}` under `{dir}/` and point the target's `path` in `{manifest}` at it"
            ),
            Fix::RootNotRust { path } => write!(
                f,
                "fix the syntax error, and make sure this line is among the inner attributes at \
                 the top of `{path}`\n{}",
                forbid_line()
            ),
            Fix::ReplaceSymlink => {
                f.write_str("replace the symlink with the file or folder it points to")
            }
            Fix::SourceNotLexed => f.write_str(
                "fix the Rust syntax that starts at this line, such as a bracket or a string \
                 left open",
            ),
            Fix::DeleteBuildScript { path } => write!(
                f,
                "delete `{path}`; a crate whose row bans I/O has no build script"
            ),
            Fix::DeleteBuildKey => {
                f.write_str("delete the `build` key, and the script it names if there is one")
            }
            Fix::CfgPredicate { form } => write!(
                f,
                "remove {form} from this cfg; the cfgs of a crate whose row bans I/O use only \
                 `test`, `doc` and features, joined by `all`, `any` and `not`, with no feature \
                 under a `not`"
            ),
            Fix::CfgMetavariableAttribute => f.write_str(
                "write the attribute's name in the macro body itself; an outer attribute may \
                 instead come whole from the caller, bound as `#[$m:meta]` in the arm's matcher \
                 and written as `#[$m]`, in a `macro_rules!` outside every other macro's body \
                 and arguments",
            ),
            Fix::CfgMetavariableMacro => {
                f.write_str("write the called macro's name in the macro body itself")
            }
            Fix::CfgHashWithoutBrackets => f.write_str(
                "write the attribute's brackets right after its `#`, as `#[..]` or `#![..]`; to \
                 pass an attribute to a macro, pass it whole, with its `#` and its brackets",
            ),
            Fix::CfgPathAttribute => f.write_str(
                "give `#[path = \"..\"]` only to an out-of-line `mod name;` at the top level of a \
                 file, outside every brace, bracket and parenthesis, with a string literal that \
                 names, relative to this file's folder and with `/` between components, a `.rs` \
                 file under the crate's folder that git lists and that is not a symlink, without \
                 leaving that folder on the way; a `path` in a `cfg_attr`, on an inline module, \
                 inside a module block or with a metavariable value always fails",
            ),
            Fix::CfgSelect => f.write_str(
                "rename a field, variable or import named `cfg_select`; for the macro, give each \
                 arm's code its own `#[cfg(..)]` built from `test`, `doc` and positive features, \
                 or use `if cfg!(..)`, which compiles both branches",
            ),
            Fix::CfgTt => f.write_str(
                "give the macros of a crate whose row bans I/O no `tt` fragment and use a typed \
                 fragment such as `item`, `expr`, `ident`, `ty` or `meta`; rename an identifier \
                 named `tt`",
            ),
            Fix::CfgInCfgAttr => f.write_str(
                "write the condition as one `#[cfg(..)]` outside any `cfg_attr`, built from \
                 `test`, `doc` and positive features",
            ),
            Fix::CfgInclude => f.write_str(
                "move the code into a module at its default path; use `include_str!` or \
                 `include_bytes!` for data",
            ),
        }
    }
}

#[cfg(test)]
mod tests;
