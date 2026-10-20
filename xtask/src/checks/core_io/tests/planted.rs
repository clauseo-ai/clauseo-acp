//! The planted cases of the core-io check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module. The forms clippy must catch are in `tests/clippy.rs`.

/// The `crates/protocol/clippy.toml` the scratch repo needs: its root file plus its list.
#[cfg(test)]
macro_rules! crate_file {
    () => {
        "# Generated: the root clippy.toml with the arrays of xtask/core-io-bans.toml appended, root\n\
         # entries first, since clippy reads only the nearest clippy.toml. `cargo xtask check`\n\
         # (core-io) fails when this file parses to anything else and prints the file to write.\n\
         msrv = \"1.99\"\n\
         disallowed-methods = [\n  \
         { path = \"std::time::Instant::now\", reason = \"the core takes time as input\" },\n  \
         { path = \"std::fs::read\", reason = \"no files\" },\n\
         ]\n\
         disallowed-types = [\n  \
         { allow-invalid = true, path = \"std::fs::File\", reason = \"no files\" },\n\
         ]\n"
    };
}

/// The fix of every violation about `crates/protocol/clippy.toml`.
#[cfg(test)]
macro_rules! write_crate_file {
    () => {
        concat!(
            "    fix: write `crates/protocol/clippy.toml` with exactly the text below, the root \
             clippy.toml with the arrays of xtask/core-io-bans.toml appended (clippy reads only \
             the nearest clippy.toml), then run `cargo clean -p clauseo-protocol`: cargo does not \
             lint a crate again when its clippy.toml appears after it last linted it\n",
            crate_file!()
        )
    };
}

/// The zero-inspected failure after a problem with the root file or the list.
#[cfg(test)]
macro_rules! stopped {
    () => {
        "0 Crates table rows inspected in the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first"
    };
}

/// The violation for the stray clippy config `$path`, a `clippy.toml` in `$dir`.
#[cfg(test)]
macro_rules! stray {
    ($path:literal, $dir:literal) => {
        concat!(
            $path, ":1: `", $path, "` is a clippy.toml other than the root one and those core-io \
             writes for the crates whose row bans fs, process or any I/O; clippy reads only the \
             nearest one, so it replaces the root clippy.toml's bans for the crates under `", $dir,
            "/`\n    fix: delete `", $path, "`: clippy settings go in the root clippy.toml, which \
             every crate reads, and core-io prints the file each crate whose row bans fs, process \
             or any I/O needs"
        )
    };
}

/// The violation for the target root `$path` of `clauseo-protocol`, a `$kind` target, without
/// the forbid line.
#[cfg(test)]
macro_rules! no_forbid {
    ($path:literal, $kind:literal) => {
        concat!(
            $path, ":1: `", $path, "`, the root of clauseo-protocol's ", $kind, " target, does \
             not carry `#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]` outside \
             any `cfg_attr`, so an `allow` or `expect` in the target can turn off the bans of \
             clippy.toml and xtask/core-io-bans.toml\n    fix: add this line to the inner \
             attributes at the top of `", $path, "`, not under `cfg_attr`\n\
             #![forbid(clippy::disallowed_methods, clippy::disallowed_types)]"
        )
    };
}

/// The violation for `crates/protocol/Cargo.toml`, which gives a `$kind` target the root
/// `$path`, outside the crate's folder.
#[cfg(test)]
macro_rules! root_outside {
    ($kind:literal, $path:literal) => {
        concat!(
            "crates/protocol/Cargo.toml:1: `crates/protocol/Cargo.toml` gives clauseo-protocol's ",
            $kind,
            " target the root `",
            $path,
            "`, outside `crates/protocol/`, and core-io checks \
             the forbid line and the cfgs of clauseo-protocol only under its folder\n    \
             fix: move `",
            $path,
            "` under `crates/protocol/` and point the target's `path` in \
             `crates/protocol/Cargo.toml` at it"
        )
    };
}

/// The violation for a cfg with `$form` at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! cfg_form {
    ($line:literal, $form:literal) => {
        concat!(
            "crates/protocol/src/hidden.rs:",
            $line,
            ": `crates/protocol/src/hidden.rs` has a cfg \
             with ",
            $form,
            "; clippy lints clauseo-protocol once per target kind with every \
             feature on, so only a cfg built from `test` and positive features holds in one of \
             its runs, and code behind any other can be left out of every clippy run\n    \
             fix: remove ",
            $form,
            " from this cfg; the cfgs of a crate whose row bans I/O use \
             only `test` and features, joined by `all`, `any` and `not`, with no feature under \
             a `not`"
        )
    };
}

/// The violation for a `path` attribute at line `$line` of `crates/protocol/src/hidden.rs`.
#[cfg(test)]
macro_rules! path_attribute {
    ($line:literal) => {
        concat!(
            "crates/protocol/src/hidden.rs:", $line, ": `crates/protocol/src/hidden.rs` gives a \
             module a `path` attribute, which can bring into clauseo-protocol a file core-io does \
             not read\n    fix: remove the `path` attribute and move the module to its default \
             path"
        )
    };
}

/// The violation for the `[alias]` key `$name` of `.cargo/config.toml` at `$line`.
#[cfg(test)]
macro_rules! alias {
    ($line:literal, $name:literal) => {
        concat!(
            ".cargo/config.toml:",
            $line,
            ": `.cargo/config.toml` defines the alias `",
            $name,
            "`, so `cargo ",
            $name,
            "` runs the alias instead of the external command xtask runs, \
             and cargo only warns about it\n    fix: remove the `",
            $name,
            "` alias; an alias may \
             not take the name of an external cargo command xtask runs"
        )
    };
}

/// The violation for the `rustflags` key `$key` of `.cargo/config.toml` at `$line`.
#[cfg(test)]
macro_rules! rustflags {
    ($line:literal, $key:literal) => {
        concat!(
            ".cargo/config.toml:", $line, ": `.cargo/config.toml` sets `", $key, "`, where `-A`, \
             `--cap-lints` or `-A clippy::style` would silence the bans of clippy.toml and \
             xtask/core-io-bans.toml for every crate; lint levels live only in the root \
             [workspace.lints]\n    fix: remove the key; set lint levels in the root Cargo.toml's \
             [workspace.lints]"
        )
    };
}

/// The violation for the compiler key `$key` of `.cargo/config.toml` at `$line`.
#[cfg(test)]
macro_rules! compiler {
    ($line:literal, $key:literal) => {
        concat!(
            ".cargo/config.toml:", $line, ": `.cargo/config.toml` sets `", $key, "`, which \
             replaces or wraps the compiler clippy runs, so it can add `--cap-lints allow` and \
             turn off the bans of clippy.toml and xtask/core-io-bans.toml past the forbid line\n    \
             fix: remove `", $key, "`; clippy runs the pinned toolchain's compiler as it is"
        )
    };
}

/// The violation for the `[env]` entry `$name` of `.cargo/config.toml` at `$line`.
#[cfg(test)]
macro_rules! clippy_env {
    ($line:literal, $name:literal) => {
        concat!(
            ".cargo/config.toml:", $line, ": `.cargo/config.toml` sets `", $name, "` in [env], \
             which cargo passes to every clippy-driver it starts; `CLIPPY_CONF_DIR` there points \
             clippy away from the clippy.toml files core-io checks\n    fix: remove the entry; \
             clippy settings go in the root clippy.toml"
        )
    };
}

/// The violation for a member's manifest `$path`, at `$line`, with its own `[lints]` table.
#[cfg(test)]
macro_rules! own_lints {
    ($path:literal, $line:literal, $package:literal) => {
        concat!(
            $path, ":", $line, ": `", $path, "` sets ", $package, "'s own lints instead of \
             `[lints] workspace = true`; lint levels live only in the root [workspace.lints], so \
             that no crate can turn off the bans of the root clippy.toml or \
             xtask/core-io-bans.toml\n    fix: replace the table with `[lints]` and \
             `workspace = true`; cargo takes no other lint key beside it"
        )
    };
}

/// The violation for a member's manifest `$path` with no `[lints]` table.
#[cfg(test)]
macro_rules! no_lints {
    ($path:literal, $package:literal) => {
        concat!(
            $path,
            ":1: `",
            $path,
            "` has no `[lints] workspace = true`, so ",
            $package,
            " does not get the lint levels of the root [workspace.lints], the one place they are \
             set\n    fix: add `[lints]` with `workspace = true`"
        )
    };
}

crate::checks::planted::cases! {
    /// A held crate without `clippy.toml` fails at its row, with the file to write.
    missing_crate_file: FailsWith(&[concat!(
        "ARCHITECTURE.md:8: clauseo-protocol's row bans I/O (Never depends on: fs, process), but \
         `crates/protocol/clippy.toml` is missing, so clippy does not ban the standard library's \
         I/O in it\n",
        write_crate_file!()
    )]), inspects 3 => super::missing_crate_file,
    /// A file that is the root alone lacks the list's entries.
    crate_file_differs: FailsWith(&[concat!(
        "crates/protocol/clippy.toml:1: `crates/protocol/clippy.toml` of clauseo-protocol is not \
         the root clippy.toml plus xtask/core-io-bans.toml: it differs in `disallowed-methods`, \
         `disallowed-types`\n",
        write_crate_file!()
    )]), inspects 3 => super::crate_file_differs,
    crate_file_not_toml: FailsWith(&[concat!(
        "crates/protocol/clippy.toml:2: `crates/protocol/clippy.toml` is not valid TOML \
         (unclosed array, expected `]`)\n",
        write_crate_file!()
    )]), inspects 3 => super::crate_file_not_toml,
    /// The generated file passes, and so does one with the same tables written another way.
    crate_file_matches: Passes, inspects 3 => super::crate_file_matches,
    crate_file_reformatted: Passes, inspects 3 => super::crate_file_reformatted,
    /// A row with `clock` alone needs no file, nor does a crate with no row.
    clock_only_row: Passes, inspects 3 => super::clock_only_row,
    crate_without_row: Passes, inspects 3 => super::crate_without_row,
    /// Problems with the inputs stop the check.
    list_missing: FailsWith(&[
        "xtask/core-io-bans.toml:1: `xtask/core-io-bans.toml` is missing, and core-io builds \
         each core crate's clippy.toml from it\n    fix: restore `xtask/core-io-bans.toml` from git",
        stopped!(),
    ]), inspects 0 => super::list_missing,
    root_missing: FailsWith(&[
        "clippy.toml:1: `clippy.toml` is missing, and core-io builds each core crate's \
         clippy.toml from it\n    fix: restore `clippy.toml` from git",
        stopped!(),
    ]), inspects 0 => super::root_missing,
    list_unknown_key: FailsWith(&[
        "xtask/core-io-bans.toml:4: `xtask/core-io-bans.toml` is not valid: unknown field \
         `disallowed-macros`, expected `disallowed-methods` or `disallowed-types`\n    \
         fix: fix the line above: the file holds only `disallowed-methods` and \
         `disallowed-types`, arrays of `{ path, reason }` tables, each with an optional \
         `replacement` and `allow-invalid`",
        stopped!(),
    ]), inspects 0 => super::list_unknown_key,
    list_entry_problems: FailsWith(&[
        "xtask/core-io-bans.toml:3: the `disallowed-methods` entry for `std::fs::read` has an \
         empty reason, which clippy prints with each hit\n    \
         fix: say why a core crate may not use the item, and what to do instead",
        "xtask/core-io-bans.toml:3: `std::fs::read` is listed twice in `disallowed-methods`\n    \
         fix: delete this entry",
        stopped!(),
    ]), inspects 0 => super::list_entry_problems,
    root_array_is_a_string: FailsWith(&[
        "clippy.toml:2: `disallowed-types` in clippy.toml is not an array, so core-io cannot \
         append xtask/core-io-bans.toml's `disallowed-types` to it\n    \
         fix: write `disallowed-types` as an array of `{ path, reason }` tables",
        stopped!(),
    ]), inspects 0 => super::root_array_is_a_string,
    /// Every target root of a held crate carries the forbid line, outside `cfg_attr`, and the
    /// crate has no build script.
    forbid_missing_in_the_lib: FailsWith(&[no_forbid!("crates/protocol/src/lib.rs", "lib")]), inspects 3 => super::forbid_missing_in_the_lib,
    forbid_under_cfg_attr: FailsWith(&[no_forbid!("crates/protocol/src/lib.rs", "lib")]), inspects 3 => super::forbid_under_cfg_attr,
    forbid_in_another_order: FailsWith(&[no_forbid!("crates/protocol/src/lib.rs", "lib")]), inspects 3 => super::forbid_in_another_order,
    forbid_missing_in_a_bin: FailsWith(&[no_forbid!("crates/protocol/src/main.rs", "bin")]), inspects 3 => super::forbid_missing_in_a_bin,
    forbid_missing_in_a_test: FailsWith(&[no_forbid!("crates/protocol/tests/events.rs", "test")]), inspects 3 => super::forbid_missing_in_a_test,
    clock_only_crate_without_forbid: FailsWith(&[no_forbid!("crates/protocol/src/lib.rs", "lib")]), inspects 3 => super::clock_only_crate_without_forbid,
    root_outside_the_repo: FailsWith(&[root_outside!("bin", "/elsewhere/outside.rs")]), inspects 3 => super::root_outside_the_repo,
    root_outside_the_repo_through_dot_dot: FailsWith(&[root_outside!("lib", "/x.rs")]), inspects 3 => super::root_outside_the_repo_through_dot_dot,
    root_outside_the_crate: FailsWith(&[root_outside!("lib", "crates/shared/lib.rs")]), inspects 3 => super::root_outside_the_crate,
    root_not_rust: FailsWith(&[
        "crates/protocol/src/lib.rs:3: `crates/protocol/src/lib.rs`, a target root of \
         clauseo-protocol, does not parse as Rust (expected identifier), so core-io cannot find \
         `#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]` in it\n    \
         fix: fix the syntax error, and make sure this line is among the inner attributes at the \
         top of `crates/protocol/src/lib.rs`\n\
         #![forbid(clippy::disallowed_methods, clippy::disallowed_types)]",
    ]), inspects 3 => super::root_not_rust,
    build_rs_present: FailsWith(&[
        "crates/protocol/build.rs:1: clauseo-protocol's row bans I/O, but it has the build script \
         `crates/protocol/build.rs`, whose `cargo::rustc-env` lines can set `CLIPPY_` variables \
         for the clippy that lints it\n    \
         fix: delete `crates/protocol/build.rs`; a crate whose row bans I/O has no build script",
    ]), inspects 3 => super::build_rs_present,
    package_build_set: FailsWith(&[
        "crates/protocol/Cargo.toml:3: `crates/protocol/Cargo.toml` sets `build` under \
         [package], but clauseo-protocol's row bans I/O and such a crate has no build script\n    \
         fix: delete the `build` key, and the script it names if there is one",
    ]), inspects 3 => super::package_build_set,
    /// A held crate's cfgs use only `test` and positive features, and its folder has no
    /// metavariable attribute or macro name, `path` attribute, `include!` or symlink.
    cfg_not_clippy: FailsWith(&[cfg_form!(1, "the name `clippy`")]), inspects 3 => super::cfg_not_clippy,
    cfg_not_debug_assertions: FailsWith(&[cfg_form!(3, "the name `debug_assertions`")]), inspects 3 => super::cfg_not_debug_assertions,
    cfg_attr_clippy: FailsWith(&[cfg_form!(1, "the name `clippy`"), path_attribute!(1)]), inspects 3 => super::cfg_attr_clippy,
    cfg_in_a_macro_body: FailsWith(&[cfg_form!(3, "the name `clippy`")]), inspects 3 => super::cfg_in_a_macro_body,
    cfg_not_feature: FailsWith(&[cfg_form!(2, "a feature under `not`")]), inspects 3 => super::cfg_not_feature,
    cfg_panic_abort: FailsWith(&[cfg_form!(1, "the name `panic`")]), inspects 3 => super::cfg_panic_abort,
    cfg_unix: FailsWith(&[cfg_form!(1, "the name `unix`")]), inspects 3 => super::cfg_unix,
    cfg_custom_name: FailsWith(&[cfg_form!(1, "the name `hidden`")]), inspects 3 => super::cfg_custom_name,
    cfg_macro_debug_assertions: FailsWith(&[cfg_form!(2, "the name `debug_assertions`")]), inspects 3 => super::cfg_macro_debug_assertions,
    cfg_attr_not_feature: FailsWith(&[cfg_form!(1, "a feature under `not`")]), inspects 3 => super::cfg_attr_not_feature,
    cfg_metavariable_in_a_macro_body: FailsWith(&[cfg_form!(3, "a metavariable")]), inspects 3 => super::cfg_metavariable_in_a_macro_body,
    attribute_name_metavariable: FailsWith(&[
        "crates/protocol/src/hidden.rs:3: `crates/protocol/src/hidden.rs` writes an attribute whose name is a metavariable, so a macro \
         of clauseo-protocol can build a cfg that core-io does not see\n    \
         fix: write the attribute's name in the macro body itself",
    ]), inspects 3 => super::attribute_name_metavariable,
    path_attribute: FailsWith(&[path_attribute!(1)]), inspects 3 => super::path_attribute,
    path_attribute_in_cfg_attr: FailsWith(&[path_attribute!(1)]), inspects 3 => super::path_attribute_in_cfg_attr,
    include_macro: FailsWith(&[
        "crates/protocol/src/hidden.rs:2: `crates/protocol/src/hidden.rs` calls `include!`, which can bring into clauseo-protocol code \
         from a file core-io does not read\n    \
         fix: move the code into a module at its default path; use `include_str!` or \
         `include_bytes!` for data",
    ]), inspects 3 => super::include_macro,
    source_not_lexed: FailsWith(&[
        "crates/protocol/src/hidden.rs:3: `crates/protocol/src/hidden.rs`, a file of clauseo-protocol, does not lex as Rust from this \
         line, so core-io cannot check its cfgs\n    \
         fix: fix the Rust syntax that starts at this line, such as a bracket or a string left open",
    ]), inspects 3 => super::source_not_lexed,
    cfg_rule_passes: Passes, inspects 3 => super::cfg_rule_passes,
    attribute_split_across_a_macro: FailsWith(&[
        "crates/protocol/src/hidden.rs:2: `crates/protocol/src/hidden.rs` writes an attribute whose name is a metavariable, so a macro \
         of clauseo-protocol can build a cfg that core-io does not see\n    \
         fix: write the attribute's name in the macro body itself",
        cfg_form!(4, "the name `panic`"),
    ]), inspects 3 => super::attribute_split_across_a_macro,
    hash_from_a_metavariable: FailsWith(&[cfg_form!(2, "the name `panic`")]), inspects 3 => super::hash_from_a_metavariable,
    items_named_cfg: Passes, inspects 3 => super::items_named_cfg,
    macro_name_metavariable: FailsWith(&[
        "crates/protocol/src/hidden.rs:3: `crates/protocol/src/hidden.rs` calls a macro whose name is a metavariable, so a macro of \
         clauseo-protocol can call `include!` or `cfg!` under a name passed to it\n    \
         fix: write the called macro's name in the macro body itself",
    ]), inspects 3 => super::macro_name_metavariable,
    symlinked_folder: FailsWith(&[
        "crates/protocol/src/sub:1: `crates/protocol/src/sub` is a symlink under \
         clauseo-protocol's folder, so a `mod` can compile files from elsewhere into \
         clauseo-protocol that core-io does not lex\n    \
         fix: replace the symlink with the file or folder it points to",
    ]), inspects 3 => super::symlinked_folder,
    forbid_in_every_root: Passes, inspects 3 => super::forbid_in_every_root,
    /// An alias named after an external cargo command xtask runs fails; the repo's `xtask` alias
    /// and one named after a built-in command, which cargo ignores, pass.
    alias_of_a_command_xtask_runs: FailsWith(&[alias!(3, "clippy")]), inspects 3 => super::alias_of_a_command_xtask_runs,
    /// Any other clippy config fails: clippy reads only the nearest one, and `.clippy.toml` first.
    dot_clippy_toml_beside_the_crate_file: FailsWith(&[
        "crates/protocol/.clippy.toml:1: `crates/protocol/.clippy.toml` is a .clippy.toml, which \
         clippy reads instead of a clippy.toml beside it or above it, so it replaces the root \
         clippy.toml's bans, and a core crate's list, for the crates under `crates/protocol/`\n    \
         fix: delete `crates/protocol/.clippy.toml`: clippy settings go in the root clippy.toml, \
         which every crate reads, and core-io prints the file each crate whose row bans fs, \
         process or any I/O needs",
    ]), inspects 3 => super::dot_clippy_toml_beside_the_crate_file,
    dot_clippy_toml_at_the_root: FailsWith(&[
        ".clippy.toml:1: `.clippy.toml` is a .clippy.toml, which clippy reads instead of a \
         clippy.toml beside it or above it, so it replaces the root clippy.toml's bans, and a core \
         crate's list, for every crate in the repo\n    \
         fix: delete `.clippy.toml`: clippy settings go in the root clippy.toml, which every \
         crate reads, and core-io prints the file each crate whose row bans fs, process or any \
         I/O needs",
    ]), inspects 3 => super::dot_clippy_toml_at_the_root,
    clippy_toml_in_crates: FailsWith(&[stray!("crates/clippy.toml", "crates")]), inspects 3 => super::clippy_toml_in_crates,
    clippy_toml_in_xtask: FailsWith(&[stray!("xtask/clippy.toml", "xtask")]), inspects 3 => super::clippy_toml_in_xtask,
    clippy_toml_of_a_crate_without_a_row: FailsWith(&[stray!("crates/extra/clippy.toml", "crates/extra")]), inspects 3 => super::clippy_toml_of_a_crate_without_a_row,
    clippy_toml_of_a_clock_only_crate: FailsWith(&[stray!("crates/protocol/clippy.toml", "crates/protocol")]), inspects 3 => super::clippy_toml_of_a_clock_only_crate,
    /// Every member inherits the workspace lints, held or not (review D5).
    crate_lints_not_inherited: FailsWith(&[no_lints!("crates/protocol/Cargo.toml", "clauseo-protocol")]), inspects 3 => super::crate_lints_not_inherited,
    crate_lints_of_its_own: FailsWith(&[own_lints!("crates/protocol/Cargo.toml", 4, "clauseo-protocol")]), inspects 3 => super::crate_lints_of_its_own,
    clock_only_crate_lints_of_its_own: FailsWith(&[own_lints!("crates/protocol/Cargo.toml", 4, "clauseo-protocol")]), inspects 3 => super::clock_only_crate_lints_of_its_own,
    crate_without_a_row_lints_not_inherited: FailsWith(&[no_lints!("crates/extra/Cargo.toml", "clauseo-extra")]), inspects 3 => super::crate_without_a_row_lints_not_inherited,
    /// A legacy `.cargo/config`, which cargo reads instead (review D4).
    legacy_cargo_config: FailsWith(&[
        ".cargo/config:1: `.cargo/config` is the legacy name of `.cargo/config.toml`; cargo \
         reads it instead when both exist, and core-io checks only `.cargo/config.toml`\n    \
         fix: move what `.cargo/config` holds into `.cargo/config.toml` and delete \
         `.cargo/config`",
    ]), inspects 3 => super::legacy_cargo_config,
    /// Any `rustflags` key in the cargo config, whatever its flags (review D2).
    rustflags_in_build: FailsWith(&[rustflags!(5, "build.rustflags")]), inspects 3 => super::rustflags_in_build,
    rustflags_in_a_target: FailsWith(&[rustflags!(2, "target.cfg(unix).rustflags")]), inspects 3 => super::rustflags_in_a_target,
    rustflags_that_cap_lints: FailsWith(&[rustflags!(2, "build.rustflags")]), inspects 3 => super::rustflags_that_cap_lints,
    /// `include`, and the keys that replace or wrap the compiler.
    include_in_cargo_config: FailsWith(&[
        ".cargo/config.toml:1: `.cargo/config.toml` sets `include`, so cargo loads other config \
         files that core-io does not read, where `rustflags`, an alias or a `CLIPPY_` entry could \
         turn off the bans of clippy.toml and xtask/core-io-bans.toml\n    \
         fix: remove `include` and move the settings it loads into `.cargo/config.toml` itself",
    ]), inspects 3 => super::include_in_cargo_config,
    compiler_keys_in_cargo_config: FailsWith(&[
        compiler!(3, "build.rustc-wrapper"),
        compiler!(4, "build.rustc"),
        compiler!(5, "build.rustc-workspace-wrapper"),
    ]), inspects 3 => super::compiler_keys_in_cargo_config,
    /// An `[env]` entry starting with `CLIPPY_` (review D1).
    clippy_env_in_cargo_config: FailsWith(&[
        clippy_env!(3, "CLIPPY_CONF_DIR"),
        clippy_env!(4, "CLIPPY_ARGS"),
    ]), inspects 3 => super::clippy_env_in_cargo_config,
    /// Windows reads environment names ignoring case.
    clippy_env_in_another_case: FailsWith(&[clippy_env!(2, "Clippy_Conf_Dir")]), inspects 3 => super::clippy_env_in_another_case,
    cargo_config_without_lint_settings: Passes, inspects 3 => super::cargo_config_without_lint_settings,
}
