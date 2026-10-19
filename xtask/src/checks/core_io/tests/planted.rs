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
            "    fix: write `crates/protocol/clippy.toml` with exactly this text, the root \
             clippy.toml with the arrays of xtask/core-io-bans.toml appended (clippy reads only \
             the nearest clippy.toml):\n",
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

crate::checks::planted::cases! {
    /// A held crate without `clippy.toml` fails at its row, with the file to write.
    missing_crate_file: FailsWith(&[concat!(
        "ARCHITECTURE.md:8: clauseo-protocol's row bans I/O (Never depends on: fs, process), but \
         `crates/protocol/clippy.toml` is missing, so clippy does not ban the standard library's \
         I/O in it\n",
        write_crate_file!()
    )]) => super::missing_crate_file,
    /// A file that is the root alone lacks the list's entries.
    crate_file_differs: FailsWith(&[concat!(
        "crates/protocol/clippy.toml:1: `crates/protocol/clippy.toml` of clauseo-protocol is not \
         the root clippy.toml plus xtask/core-io-bans.toml: it differs in `disallowed-methods`, \
         `disallowed-types`\n",
        write_crate_file!()
    )]) => super::crate_file_differs,
    crate_file_not_toml: FailsWith(&[concat!(
        "crates/protocol/clippy.toml:2: `crates/protocol/clippy.toml` is not valid TOML \
         (unclosed array, expected `]`)\n",
        write_crate_file!()
    )]) => super::crate_file_not_toml,
    /// The generated file passes, and so does one with the same tables written another way.
    crate_file_matches: Passes => super::crate_file_matches,
    crate_file_reformatted: Passes => super::crate_file_reformatted,
    /// A row with `clock` alone needs no file, nor does a crate with no row.
    clock_only_row: Passes => super::clock_only_row,
    crate_without_row: Passes => super::crate_without_row,
    /// Problems with the inputs stop the check.
    list_missing: FailsWith(&[
        "xtask/core-io-bans.toml:1: `xtask/core-io-bans.toml` is missing, and core-io builds \
         each core crate's clippy.toml from it\n    fix: restore `xtask/core-io-bans.toml` from git",
        stopped!(),
    ]) => super::list_missing,
    root_missing: FailsWith(&[
        "clippy.toml:1: `clippy.toml` is missing, and core-io builds each core crate's \
         clippy.toml from it\n    fix: restore `clippy.toml` from git",
        stopped!(),
    ]) => super::root_missing,
    list_unknown_key: FailsWith(&[
        "xtask/core-io-bans.toml:4: `xtask/core-io-bans.toml` is not valid: unknown field \
         `disallowed-macros`, expected `disallowed-methods` or `disallowed-types`\n    \
         fix: fix the line above: the file holds only `disallowed-methods` and \
         `disallowed-types`, arrays of `{ path, reason }` tables, each with an optional \
         `replacement` and `allow-invalid`",
        stopped!(),
    ]) => super::list_unknown_key,
    list_entry_problems: FailsWith(&[
        "xtask/core-io-bans.toml:3: the `disallowed-methods` entry for `std::fs::read` has an \
         empty reason, which clippy prints with each hit\n    \
         fix: say why a core crate may not use the item, and what to do instead",
        "xtask/core-io-bans.toml:3: `std::fs::read` is listed twice in `disallowed-methods`\n    \
         fix: delete this entry",
        stopped!(),
    ]) => super::list_entry_problems,
    root_array_is_a_string: FailsWith(&[
        "clippy.toml:2: `disallowed-types` in clippy.toml is not an array, so core-io cannot \
         append xtask/core-io-bans.toml's `disallowed-types` to it\n    \
         fix: write `disallowed-types` as an array of `{ path, reason }` tables",
        stopped!(),
    ]) => super::root_array_is_a_string,
}
