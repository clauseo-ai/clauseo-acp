//! The violation of every core-io finding, so each message sits beside its fix kind. The fix
//! texts live in `fix.rs`.

use super::clippy_toml::{LIST, ROOT};
use super::fix::Fix;
use super::roots::forbid_line;
use crate::checks::{crates_table, violation};
use crate::violation::Violation;

mod cfg;
pub(super) use cfg::cfg_rule;

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
        write_crate_file(path, package, text),
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
        write_crate_file(path, package, text),
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
        write_crate_file(path, package, text),
    )
}

/// The fix for `package`'s `clippy.toml` at `path`, which says what to write and how to make
/// clippy read it.
fn write_crate_file(path: &str, package: &str, text: &str) -> Fix {
    Fix::WriteCrateFile {
        path: path.to_owned(),
        package: package.to_owned(),
        text: text.to_owned(),
    }
}

/// The violation for the root file or the list, missing at `rel`.
pub(super) fn missing_input(rel: &str) -> Violation {
    violation(
        rel,
        1,
        format!("`{rel}` is missing, and core-io builds each core crate's clippy.toml from it"),
        Fix::RestoreInput {
            rel: rel.to_owned(),
        },
    )
}

/// The violation for the root file or the list at `rel`, which `toml` could not read.
pub(super) fn not_toml(rel: &str, line: u32, error: &toml::de::Error) -> Violation {
    let first = error.message().lines().next().unwrap_or_default().trim();
    let fix = if rel == LIST {
        Fix::ListNotToml
    } else if rel == ROOT {
        Fix::RootNotToml
    } else {
        Fix::CargoNotToml
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
        Fix::RootNotArray {
            key: key.to_owned(),
        },
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
        Fix::EmptyReason,
    )
}

/// The violation for an entry of the list's `key` whose path an earlier entry already names.
pub(super) fn listed_twice(key: &str, path: &str, line: u32) -> Violation {
    violation(
        LIST,
        line,
        format!("`{path}` is listed twice in `{key}`"),
        Fix::ListedTwice,
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
    let message = if name.eq_ignore_ascii_case(".clippy.toml") {
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
        Fix::StrayConfig {
            path: path.to_owned(),
        },
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
                "`{path}` sets {package}'s own lints instead of `[lints] workspace = true`; lint \
                 levels live only in the root [workspace.lints], so that no crate can turn off the \
                 bans of the root {ROOT} or {LIST}"
            ),
            Fix::ReplaceLintsTable,
        )
    } else {
        (
            format!(
                "`{path}` has no `[lints] workspace = true`, so {package} does not get the lint \
                 levels of the root [workspace.lints], the one place they are set"
            ),
            Fix::AddLints,
        )
    };
    violation(path, line, message, fix)
}

/// The violation for the `rustflags` key `key` of the cargo config at `path`, at `line`.
pub(super) fn rustflags_in_config(path: &str, line: u32, key: &str) -> Violation {
    violation(
        path,
        line,
        format!(
            "`{path}` sets `{key}`, where `-A`, `--cap-lints` or `-A clippy::style` would \
             silence the bans of {ROOT} and {LIST} for every crate; lint levels live only in the \
             root [workspace.lints]"
        ),
        Fix::RemoveRustflags,
    )
}

/// The violation for the `include` key of the cargo config at `path`, at `line`.
pub(super) fn include_in_config(path: &str, line: u32) -> Violation {
    violation(
        path,
        line,
        format!(
            "`{path}` sets `include`, so cargo loads other config files that core-io does not \
             read, where `rustflags`, an alias or a `CLIPPY_` entry could turn off the bans of \
             {ROOT} and {LIST}"
        ),
        Fix::RemoveInclude {
            path: path.to_owned(),
        },
    )
}

/// The violation for the compiler key `key` (`build.rustc` or a wrapper) of the cargo config at
/// `path`, at `line`.
pub(super) fn compiler_in_config(path: &str, line: u32, key: &str) -> Violation {
    violation(
        path,
        line,
        format!(
            "`{path}` sets `{key}`, which replaces or wraps the compiler clippy runs, so it can \
             add `--cap-lints allow` and turn off the bans of {ROOT} and {LIST} past the forbid \
             line"
        ),
        Fix::RemoveCompiler {
            key: key.to_owned(),
        },
    )
}

/// The violation for the `[env]` entry `name` of the cargo config at `path`, at `line`.
pub(super) fn clippy_env_in_config(path: &str, line: u32, name: &str) -> Violation {
    violation(
        path,
        line,
        format!(
            "`{path}` sets `{name}` in [env], which cargo passes to every clippy-driver it starts; \
             `CLIPPY_CONF_DIR` there points clippy away from the {ROOT} files core-io checks"
        ),
        Fix::RemoveClippyEnv,
    )
}

/// The violation for the `[alias]` key `name` of the cargo config at `path`, at `line`, which
/// is an external cargo command xtask runs.
pub(super) fn alias_shadows_command(path: &str, line: u32, name: &str) -> Violation {
    violation(
        path,
        line,
        format!(
            "`{path}` defines the alias `{name}`, so `cargo {name}` runs the alias instead of \
             the external command xtask runs, and cargo only warns about it"
        ),
        Fix::RemoveAlias {
            name: name.to_owned(),
        },
    )
}

/// The violation for the legacy cargo config `legacy`, which cargo reads instead of `config`.
pub(super) fn legacy_cargo_config(legacy: &str, config: &str) -> Violation {
    violation(
        legacy,
        1,
        format!(
            "`{legacy}` is the legacy name of `{config}`; cargo reads it instead when both exist, \
             and core-io checks only `{config}`"
        ),
        Fix::MoveLegacyConfig {
            legacy: legacy.to_owned(),
            config: config.to_owned(),
        },
    )
}

/// The violation for the target root `path` of `package`, a target of kind `kinds`, whose
/// inner attributes lack `forbid_line()`.
pub(super) fn missing_forbid(path: &str, package: &str, kinds: &str) -> Violation {
    violation(
        path,
        1,
        format!(
            "`{path}`, the root of {package}'s {kinds} target, does not carry `{}` \
             outside any `cfg_attr`, so an `allow` or `expect` in the target can turn off the \
             bans of {ROOT} and {LIST}",
            forbid_line()
        ),
        Fix::AddForbid {
            path: path.to_owned(),
        },
    )
}

/// The violation for `package`'s manifest `manifest`, which gives a target of kind `kinds` the
/// root `path`, outside the crate's folder `dir`.
pub(super) fn root_outside(
    manifest: &str,
    package: &str,
    kinds: &str,
    path: &str,
    dir: &str,
) -> Violation {
    violation(
        manifest,
        1,
        format!(
            "`{manifest}` gives {package}'s {kinds} target the root `{path}`, outside `{dir}/`, \
             and core-io checks the forbid line and the cfgs of {package} only under its folder"
        ),
        Fix::MoveRoot {
            manifest: manifest.to_owned(),
            path: path.to_owned(),
            dir: dir.to_owned(),
        },
    )
}

/// The violation for the target root `path` of `package`, which syn cannot parse (the error
/// `error` at `line`).
pub(super) fn root_not_rust(path: &str, package: &str, line: u32, error: &syn::Error) -> Violation {
    violation(
        path,
        line,
        format!(
            "`{path}`, a target root of {package}, does not parse as Rust ({error}), so core-io \
             cannot find `{}` in it",
            forbid_line()
        ),
        Fix::RootNotRust {
            path: path.to_owned(),
        },
    )
}

/// The violation for `path`, a symlink under the folder of `package`.
pub(super) fn symlink(path: &str, package: &str) -> Violation {
    violation(
        path,
        1,
        format!(
            "`{path}` is a symlink under {package}'s folder, so a `mod` can compile files from \
             elsewhere into {package} that core-io does not lex"
        ),
        Fix::ReplaceSymlink,
    )
}

/// The violation for `path`, a file of `package` that does not lex as Rust from `line`.
pub(super) fn source_not_lexed(path: &str, line: u32, package: &str) -> Violation {
    violation(
        path,
        line,
        format!(
            "`{path}`, a file of {package}, does not lex as Rust from this line, so core-io \
             cannot check its cfgs"
        ),
        Fix::SourceNotLexed,
    )
}

/// The violation for the build script `path` of `package`, whose row bans I/O.
pub(super) fn build_script_file(path: &str, package: &str) -> Violation {
    violation(
        path,
        1,
        format!(
            "{package}'s row bans I/O, but it has the build script `{path}`, whose \
             `cargo::rustc-env` lines can set `CLIPPY_` variables for the clippy that lints it"
        ),
        Fix::DeleteBuildScript {
            path: path.to_owned(),
        },
    )
}

/// The violation for the `build` key of `[package]` in `package`'s manifest `path`, at `line`.
pub(super) fn build_key(path: &str, line: u32, package: &str) -> Violation {
    violation(
        path,
        line,
        format!(
            "`{path}` sets `build` under [package], but {package}'s row bans I/O and such a \
             crate has no build script"
        ),
        Fix::DeleteBuildKey,
    )
}
