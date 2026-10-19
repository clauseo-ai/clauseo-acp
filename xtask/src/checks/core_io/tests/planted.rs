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

/// The violation for the root manifest's `$name` at `$line`, set to `$level`.
#[cfg(test)]
macro_rules! lint_level {
    ($line:literal, $name:literal, $level:literal) => {
        concat!(
            "Cargo.toml:",
            $line,
            ": `[workspace.lints.clippy]` sets `",
            $name,
            "` to `",
            $level,
            "`, which can stop clippy reporting the calls clippy.toml and xtask/core-io-bans.toml \
             ban\n    fix: set `",
            $name,
            "` to \"warn\" or \"deny\", or remove it; to let one call \
             through, put `#[expect(clippy::disallowed_methods, reason = \"..\")]` on it"
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
             `workspace = true` (cargo takes no other lint key beside it); to let one call \
             through, put `#[expect(clippy::disallowed_methods, reason = \"..\")]` on it"
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
    /// `[workspace.lints.clippy]` may not turn off the two lints or the groups that hold them,
    /// at any priority (review D3).
    workspace_lint_allowed: FailsWith(&[
        lint_level!(6, "disallowed_types", "allow"),
        lint_level!(7, "disallowed-methods", "expect"),
    ]), inspects 3 => super::workspace_lint_allowed,
    workspace_lint_group_allowed: FailsWith(&[
        lint_level!(5, "style", "allow"),
        lint_level!(6, "all", "allow"),
    ]), inspects 3 => super::workspace_lint_group_allowed,
    workspace_lints_that_report: Passes, inspects 3 => super::workspace_lints_that_report,
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
    /// An `[env]` entry starting with `CLIPPY_` (review D1).
    clippy_env_in_cargo_config: FailsWith(&[
        clippy_env!(3, "CLIPPY_CONF_DIR"),
        clippy_env!(4, "CLIPPY_ARGS"),
    ]), inspects 3 => super::clippy_env_in_cargo_config,
    cargo_config_without_lint_settings: Passes, inspects 3 => super::cargo_config_without_lint_settings,
}
