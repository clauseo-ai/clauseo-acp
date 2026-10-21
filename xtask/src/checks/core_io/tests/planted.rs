//! The planted cases of the core-io check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! `tests/repos.rs` and its topic files. The forbid forms clippy must catch are in
//! `tests/clippy.rs`, and the macros that spell out the lines several cases print are in
//! `tests/expected.rs`.

use crate::checks::core_io::Fix;
use crate::checks::core_io::fix::CfgFix;
use crate::checks::planted::fails;

crate::checks::planted::cases! {
    /// A held crate without `clippy.toml` fails at its row, with the file to write.
    missing_crate_file: Fails[fails(
        "ARCHITECTURE.md:8",
        "clauseo-protocol's row bans I/O (Never depends on: fs, process), but \
         `crates/protocol/clippy.toml` is missing, so clippy does not ban the standard library's \
         I/O in it",
        write_crate_file!(),
    )], inspects 3 => super::repos::missing_crate_file,
    /// A file that is the root alone lacks the list's entries.
    crate_file_differs: Fails[fails(
        "crates/protocol/clippy.toml:1",
        "`crates/protocol/clippy.toml` of clauseo-protocol is not the root clippy.toml plus \
         xtask/core-io-bans.toml: it differs in `disallowed-methods`, `disallowed-types`",
        write_crate_file!(),
    )], inspects 3 => super::repos::crate_file_differs,
    crate_file_not_toml: Fails[fails(
        "crates/protocol/clippy.toml:2",
        "`crates/protocol/clippy.toml` is not valid TOML (unclosed array, expected `]`)",
        write_crate_file!(),
    )], inspects 3 => super::repos::crate_file_not_toml,
    /// The generated file passes, and so does one with the same tables written another way.
    crate_file_matches: Passes, inspects 3 => super::repos::crate_file_matches,
    crate_file_reformatted: Passes, inspects 3 => super::repos::crate_file_reformatted,
    /// A row with `clock` alone needs no file, nor does a crate with no row.
    clock_only_row: Passes, inspects 3 => super::repos::clock_only_row,
    crate_without_row: Passes, inspects 3 => super::repos::crate_without_row,
    /// Problems with the inputs stop the check.
    list_missing: Fails[fails(
        "xtask/core-io-bans.toml:1",
        "`xtask/core-io-bans.toml` is missing, and core-io builds each core crate's clippy.toml \
         from it",
        Fix::RestoreInput {
            rel: "xtask/core-io-bans.toml".to_owned(),
        },
    )], inspects 0 => super::repos::list_missing,
    root_missing: Fails[fails(
        "clippy.toml:1",
        "`clippy.toml` is missing, and core-io builds each core crate's clippy.toml from it",
        Fix::RestoreInput {
            rel: "clippy.toml".to_owned(),
        },
    )], inspects 0 => super::repos::root_missing,
    list_unknown_key: Fails[fails(
        "xtask/core-io-bans.toml:4",
        "`xtask/core-io-bans.toml` is not valid: unknown field `disallowed-macros`, expected \
         `disallowed-methods` or `disallowed-types`",
        Fix::ListNotToml,
    )], inspects 0 => super::repos::list_unknown_key,
    list_entry_problems: Fails[
        fails(
            "xtask/core-io-bans.toml:3",
            "the `disallowed-methods` entry for `std::fs::read` has an empty reason, which clippy \
             prints with each hit",
            Fix::EmptyReason,
        ),
        fails(
            "xtask/core-io-bans.toml:3",
            "`std::fs::read` is listed twice in `disallowed-methods`",
            Fix::ListedTwice,
        ),
    ], inspects 0 => super::repos::list_entry_problems,
    /// A root `clippy.toml` that is not TOML fails at the line the parser stopped on, with the
    /// root's fix.
    root_not_toml: Fails[fails(
        "clippy.toml:2",
        "`clippy.toml` is not valid: unclosed array, expected `]`",
        Fix::RootNotToml,
    )], inspects 0 => super::repos::root_not_toml,
    root_array_is_a_string: Fails[fails(
        "clippy.toml:2",
        "`disallowed-types` in clippy.toml is not an array, so core-io cannot append \
         xtask/core-io-bans.toml's `disallowed-types` to it",
        Fix::RootNotArray {
            key: "disallowed-types".to_owned(),
        },
    )], inspects 0 => super::repos::root_array_is_a_string,
    /// Every target root of a held crate carries the forbid line, outside `cfg_attr`, and the
    /// crate has no build script.
    forbid_missing_in_the_lib: Fails[no_forbid!("crates/protocol/src/lib.rs", "lib")], inspects 3 => super::repos::forbid_missing_in_the_lib,
    forbid_under_cfg_attr: Fails[no_forbid!("crates/protocol/src/lib.rs", "lib")], inspects 3 => super::repos::forbid_under_cfg_attr,
    forbid_in_another_order: Fails[no_forbid!("crates/protocol/src/lib.rs", "lib")], inspects 3 => super::repos::forbid_in_another_order,
    forbid_missing_in_a_bin: Fails[no_forbid!("crates/protocol/src/main.rs", "bin")], inspects 3 => super::repos::forbid_missing_in_a_bin,
    forbid_missing_in_a_test: Fails[no_forbid!("crates/protocol/tests/events.rs", "test")], inspects 3 => super::repos::forbid_missing_in_a_test,
    clock_only_crate_without_forbid: Fails[no_forbid!("crates/protocol/src/lib.rs", "lib")], inspects 3 => super::repos::clock_only_crate_without_forbid,
    forbid_missing_in_an_example: Fails[no_forbid!("crates/protocol/examples/demo.rs", "example")], inspects 3 => super::repos::forbid_missing_in_an_example,
    forbid_missing_in_a_bench: Fails[no_forbid!("crates/protocol/benches/fold.rs", "bench")], inspects 3 => super::repos::forbid_missing_in_a_bench,
    /// Only `forbid` of both lints, at the top of the root, counts.
    forbid_naming_one_lint: Fails[no_forbid!("crates/protocol/src/lib.rs", "lib")], inspects 3 => super::repos::forbid_naming_one_lint,
    deny_in_place_of_forbid: Fails[no_forbid!("crates/protocol/src/lib.rs", "lib")], inspects 3 => super::repos::deny_in_place_of_forbid,
    forbid_inside_a_module: Fails[no_forbid!("crates/protocol/src/lib.rs", "lib")], inspects 3 => super::repos::forbid_inside_a_module,
    forbid_as_an_outer_attribute: Fails[no_forbid!("crates/protocol/src/lib.rs", "lib")], inspects 3 => super::repos::forbid_as_an_outer_attribute,
    root_outside_the_repo: Fails[root_outside!("bin", "/elsewhere/outside.rs")], inspects 3 => super::repos::root_outside_the_repo,
    root_outside_the_repo_through_dot_dot: Fails[root_outside!("lib", "/x.rs")], inspects 3 => super::repos::root_outside_the_repo_through_dot_dot,
    root_outside_the_crate: Fails[root_outside!("lib", "crates/shared/lib.rs")], inspects 3 => super::repos::root_outside_the_crate,
    root_not_rust: Fails[fails(
        "crates/protocol/src/lib.rs:3",
        "`crates/protocol/src/lib.rs`, a target root of clauseo-protocol, does not parse as Rust \
         (expected identifier), so core-io cannot find `#![forbid(clippy::disallowed_methods, \
         clippy::disallowed_types)]` in it",
        Fix::RootNotRust {
            path: "crates/protocol/src/lib.rs".to_owned(),
        },
    )], inspects 3 => super::repos::root_not_rust,
    build_rs_present: Fails[fails(
        "crates/protocol/build.rs:1",
        "clauseo-protocol's row bans I/O, but it has the build script `crates/protocol/build.rs`, \
         whose `cargo::rustc-env` lines can set `CLIPPY_` variables for the clippy that lints it",
        Fix::DeleteBuildScript {
            path: "crates/protocol/build.rs".to_owned(),
        },
    )], inspects 3 => super::repos::build_rs_present,
    package_build_set: Fails[fails(
        "crates/protocol/Cargo.toml:3",
        "`crates/protocol/Cargo.toml` sets `build` under [package], but clauseo-protocol's row \
         bans I/O and such a crate has no build script",
        Fix::DeleteBuildKey,
    )], inspects 3 => super::repos::package_build_set,
    /// A held crate's cfgs use only `test` and positive features, and its folder has no
    /// metavariable attribute or macro name, `path` attribute, `include!` or symlink.
    cfg_not_clippy: Fails[cfg_form!(1, "the name `clippy`")], inspects 3 => super::repos::cfg_not_clippy,
    cfg_not_debug_assertions: Fails[cfg_form!(3, "the name `debug_assertions`")], inspects 3 => super::repos::cfg_not_debug_assertions,
    cfg_attr_clippy: Fails[cfg_form!(1, "the name `clippy`"), path_attribute!(1)], inspects 3 => super::repos::cfg_attr_clippy,
    cfg_in_a_macro_body: Fails[cfg_form!(3, "the name `clippy`")], inspects 3 => super::repos::cfg_in_a_macro_body,
    cfg_not_feature: Fails[cfg_form!(2, "a feature under `not`")], inspects 3 => super::repos::cfg_not_feature,
    cfg_panic_abort: Fails[cfg_form!(1, "the name `panic`")], inspects 3 => super::repos::cfg_panic_abort,
    cfg_unix: Fails[cfg_form!(1, "the name `unix`")], inspects 3 => super::repos::cfg_unix,
    cfg_custom_name: Fails[cfg_form!(1, "the name `hidden`")], inspects 3 => super::repos::cfg_custom_name,
    cfg_select_expression: Fails[cfg_select!(2)], inspects 3 => super::repos::cfg_select_expression,
    cfg_select_items: Fails[cfg_select!(1)], inspects 3 => super::repos::cfg_select_items,
    cfg_select_alias: Fails[cfg_select!(1)], inspects 3 => super::repos::cfg_select_alias,
    cfg_inside_cfg_attr: Fails[fails(
        "crates/protocol/src/hidden.rs:1",
        "`crates/protocol/src/hidden.rs` puts a `cfg` inside a `cfg_attr`, which keeps the item \
         whenever the `cfg_attr`'s predicate is false, so a feature there acts like a \
         `not(feature)` and can leave code out of every clippy run",
        Fix::Cfg(CfgFix::InCfgAttr),
    )], inspects 3 => super::repos::cfg_inside_cfg_attr,
    cfg_attr_not_feature: Fails[cfg_form!(1, "a feature under `not`")], inspects 3 => super::repos::cfg_attr_not_feature,
    cfg_metavariable_in_a_macro_body: Fails[cfg_form!(3, "a metavariable")], inspects 3 => super::repos::cfg_metavariable_in_a_macro_body,
    attribute_name_metavariable: Fails[metavariable_attribute!(3)], inspects 3 => super::repos::attribute_name_metavariable,
    /// A transcriber may write `#[$m]` only when its arm binds `$m` as `#[$m:meta]`.
    forwarded_attributes_pass: Passes, inspects 3 => super::repos::forwarded_attributes_pass,
    forwarded_attribute_as_tt: Fails[tt_fragment!(2), metavariable_attribute!(3)], inspects 3 => super::repos::forwarded_attribute_as_tt,
    forwarded_attribute_as_ident: Fails[metavariable_attribute!(3)], inspects 3 => super::repos::forwarded_attribute_as_ident,
    forwarded_cfg_at_the_call: Fails[cfg_form!(7, "the name `clippy`")], inspects 3 => super::repos::forwarded_cfg_at_the_call,
    /// A definition inside a transcriber or a macro call's arguments gets the full rules.
    forwarded_meta_in_a_nested_definition: Fails[tt_fragment!(2), metavariable_attribute!(4), metavariable_attribute!(4)], inspects 3 => super::repos::forwarded_meta_in_a_nested_definition,
    definition_shape_in_call_arguments: Fails[tt_fragment!(1), tt_fragment!(2), metavariable_attribute!(3)], inspects 3 => super::repos::definition_shape_in_call_arguments,
    /// A `#` followed by neither brackets nor `!` and brackets fails, wherever it stands.
    hash_joins_written_brackets: Fails[tt_fragment!(1), tt_fragment!(1), hash_without_brackets!(2)], inspects 3 => super::repos::hash_joins_written_brackets,
    attribute_built_across_two_macros: Fails[tt_fragment!(1), tt_fragment!(1), tt_fragment!(2), hash_without_brackets!(2)], inspects 3 => super::repos::attribute_built_across_two_macros,
    indexing_after_an_ident_passes: Passes, inspects 3 => super::repos::indexing_after_an_ident_passes,
    indexing_after_an_expr_passes: Passes, inspects 3 => super::repos::indexing_after_an_expr_passes,
    /// The name `tt` fails wherever it stands, since only a `tt` fragment can take a lone `#`.
    indexing_after_a_tt: Fails[tt_fragment!(1)], inspects 3 => super::repos::indexing_after_a_tt,
    hash_split_from_a_whole_attribute: Fails[tt_fragment!(1), tt_fragment!(1), tt_fragment!(1)], inspects 3 => super::repos::hash_split_from_a_whole_attribute,
    hash_split_from_a_doc_comment: Fails[tt_fragment!(1), tt_fragment!(1), tt_fragment!(1)], inspects 3 => super::repos::hash_split_from_a_doc_comment,
    hash_split_across_two_macros: Fails[tt_fragment!(1), tt_fragment!(1), tt_fragment!(2), tt_fragment!(2), tt_fragment!(2)], inspects 3 => super::repos::hash_split_across_two_macros,
    path_attribute: Fails[path_attribute!(1)], inspects 3 => super::repos::path_attribute,
    path_attribute_in_cfg_attr: Fails[path_attribute!(1)], inspects 3 => super::repos::path_attribute_in_cfg_attr,
    /// A literal `path` on a top-level `mod name;` passes when it names a file core-io lexes.
    path_to_a_lexed_file_passes: Passes, inspects 3 => super::repos::path_to_a_lexed_file_passes,
    path_from_a_macro_value: Fails[path_attribute!(3)], inspects 3 => super::repos::path_from_a_macro_value,
    path_not_a_literal: Fails[path_attribute!(1)], inspects 3 => super::repos::path_not_a_literal,
    path_leaving_the_crate: Fails[path_attribute!(1)], inspects 3 => super::repos::path_leaving_the_crate,
    path_on_an_inline_module: Fails[path_attribute!(1)], inspects 3 => super::repos::path_on_an_inline_module,
    path_inside_a_module_block: Fails[path_attribute!(2)], inspects 3 => super::repos::path_inside_a_module_block,
    include_macro: Fails[fails(
        "crates/protocol/src/hidden.rs:2",
        "`crates/protocol/src/hidden.rs` calls `include!`, which can bring into clauseo-protocol \
         code from a file core-io does not read",
        Fix::Cfg(CfgFix::Include),
    )], inspects 3 => super::repos::include_macro,
    source_not_lexed: Fails[fails(
        "crates/protocol/src/hidden.rs:3",
        "`crates/protocol/src/hidden.rs`, a file of clauseo-protocol, does not lex as Rust from \
         this line, so core-io cannot check its cfgs",
        Fix::SourceNotLexed,
    )], inspects 3 => super::repos::source_not_lexed,
    cfg_rule_passes: Passes, inspects 3 => super::repos::cfg_rule_passes,
    cfg_doc_passes: Passes, inspects 3 => super::repos::cfg_doc_passes,
    /// rustc ignores a shebang first line, so core-io does too.
    shebang_line_passes: Passes, inspects 3 => super::repos::shebang_line_passes,
    attribute_split_across_a_macro: Fails[
        tt_fragment!(2),
        tt_fragment!(2),
        hash_without_brackets!(2),
        cfg_form!(4, "the name `panic`"),
    ], inspects 3 => super::repos::attribute_split_across_a_macro,
    hash_from_a_metavariable: Fails[tt_fragment!(2), tt_fragment!(2), cfg_form!(2, "the name `panic`"), hash_without_brackets!(4)], inspects 3 => super::repos::hash_from_a_metavariable,
    path_with_a_metavariable: Fails[tt_fragment!(2), path_attribute!(3)], inspects 3 => super::repos::path_with_a_metavariable,
    include_with_a_metavariable: Fails[
        tt_fragment!(2),
        fails(
            "crates/protocol/src/hidden.rs:3",
            "`crates/protocol/src/hidden.rs` calls `include!`, which can bring into clauseo-protocol \
             code from a file core-io does not read",
            Fix::Cfg(CfgFix::Include),
        ),
    ], inspects 3 => super::repos::include_with_a_metavariable,
    items_named_cfg: Passes, inspects 3 => super::repos::items_named_cfg,
    macro_name_metavariable: Fails[fails(
        "crates/protocol/src/hidden.rs:3",
        "`crates/protocol/src/hidden.rs` calls a macro whose name is a metavariable, so a macro of \
         clauseo-protocol can call `include!` under a name passed to it",
        Fix::Cfg(CfgFix::MetavariableMacro),
    )], inspects 3 => super::repos::macro_name_metavariable,
    symlinked_folder: Fails[fails(
        "crates/protocol/src/sub:1",
        "`crates/protocol/src/sub` is a symlink under clauseo-protocol's folder, so a `mod` can \
         compile files from elsewhere into clauseo-protocol that core-io does not lex",
        Fix::ReplaceSymlink,
    )], inspects 3 => super::repos::symlinked_folder,
    forbid_in_every_root: Passes, inspects 3 => super::repos::forbid_in_every_root,
    /// An alias named after an external cargo command xtask runs fails; the repo's `xtask` alias
    /// and one named after a built-in command, which cargo ignores, pass.
    alias_of_a_command_xtask_runs: Fails[alias!(3, "clippy")], inspects 3 => super::repos::alias_of_a_command_xtask_runs,
    /// Any other clippy config fails: clippy reads only the nearest one, and `.clippy.toml` first.
    dot_clippy_toml_beside_the_crate_file: Fails[fails(
        "crates/protocol/.clippy.toml:1",
        "`crates/protocol/.clippy.toml` is a .clippy.toml, which clippy reads instead of a \
         clippy.toml beside it or above it, so it replaces the root clippy.toml's bans, and a core \
         crate's list, for the crates under `crates/protocol/`",
        Fix::StrayConfig {
            path: "crates/protocol/.clippy.toml".to_owned(),
        },
    )], inspects 3 => super::repos::dot_clippy_toml_beside_the_crate_file,
    dot_clippy_toml_at_the_root: Fails[fails(
        ".clippy.toml:1",
        "`.clippy.toml` is a .clippy.toml, which clippy reads instead of a clippy.toml beside it \
         or above it, so it replaces the root clippy.toml's bans, and a core crate's list, for \
         every crate in the repo",
        Fix::StrayConfig {
            path: ".clippy.toml".to_owned(),
        },
    )], inspects 3 => super::repos::dot_clippy_toml_at_the_root,
    /// A filesystem that ignores case opens `Clippy.toml` as `clippy.toml`.
    clippy_toml_in_another_case: Fails[stray!("crates/Clippy.toml", "crates")], inspects 3 => super::repos::clippy_toml_in_another_case,
    dot_clippy_toml_in_another_case: Fails[fails(
        ".CLIPPY.toml:1",
        "`.CLIPPY.toml` is a .clippy.toml, which clippy reads instead of a clippy.toml beside it \
         or above it, so it replaces the root clippy.toml's bans, and a core crate's list, for \
         every crate in the repo",
        Fix::StrayConfig {
            path: ".CLIPPY.toml".to_owned(),
        },
    )], inspects 3 => super::repos::dot_clippy_toml_in_another_case,
    clippy_toml_in_crates: Fails[stray!("crates/clippy.toml", "crates")], inspects 3 => super::repos::clippy_toml_in_crates,
    clippy_toml_in_xtask: Fails[stray!("xtask/clippy.toml", "xtask")], inspects 3 => super::repos::clippy_toml_in_xtask,
    clippy_toml_of_a_crate_without_a_row: Fails[stray!("crates/extra/clippy.toml", "crates/extra")], inspects 3 => super::repos::clippy_toml_of_a_crate_without_a_row,
    clippy_toml_of_a_clock_only_crate: Fails[stray!("crates/protocol/clippy.toml", "crates/protocol")], inspects 3 => super::repos::clippy_toml_of_a_clock_only_crate,
    /// Every member inherits the workspace lints, held or not (review D5).
    crate_lints_not_inherited: Fails[no_lints!("crates/protocol/Cargo.toml", "clauseo-protocol")], inspects 3 => super::repos::crate_lints_not_inherited,
    crate_lints_of_its_own: Fails[own_lints!("crates/protocol/Cargo.toml", 4, "clauseo-protocol")], inspects 3 => super::repos::crate_lints_of_its_own,
    clock_only_crate_lints_of_its_own: Fails[own_lints!("crates/protocol/Cargo.toml", 4, "clauseo-protocol")], inspects 3 => super::repos::clock_only_crate_lints_of_its_own,
    crate_without_a_row_lints_not_inherited: Fails[no_lints!("crates/extra/Cargo.toml", "clauseo-extra")], inspects 3 => super::repos::crate_without_a_row_lints_not_inherited,
    crate_lints_workspace_false: Fails[own_lints!("crates/protocol/Cargo.toml", 4, "clauseo-protocol")], inspects 3 => super::repos::crate_lints_workspace_false,
    /// A legacy `.cargo/config`, which cargo reads instead (review D4).
    legacy_cargo_config: Fails[fails(
        ".cargo/config:1",
        "`.cargo/config` is the legacy name of `.cargo/config.toml`; cargo reads it instead when \
         both exist, and core-io checks only `.cargo/config.toml`",
        Fix::MoveLegacyConfig {
            legacy: ".cargo/config".to_owned(),
            config: ".cargo/config.toml".to_owned(),
        },
    )], inspects 3 => super::repos::legacy_cargo_config,
    /// Any `rustflags` key in the cargo config, whatever its flags (review D2).
    rustflags_in_build: Fails[rustflags!(5, "build.rustflags")], inspects 3 => super::repos::rustflags_in_build,
    rustflags_in_a_target: Fails[rustflags!(2, "target.cfg(unix).rustflags")], inspects 3 => super::repos::rustflags_in_a_target,
    rustflags_that_cap_lints: Fails[rustflags!(2, "build.rustflags")], inspects 3 => super::repos::rustflags_that_cap_lints,
    /// Dotted keys and inline tables are the same keys.
    rustflags_as_a_dotted_key: Fails[rustflags!(5, "target.x86_64-unknown-linux-gnu.rustflags")], inspects 3 => super::repos::rustflags_as_a_dotted_key,
    rustflags_in_an_inline_table: Fails[rustflags!(1, "build.rustflags")], inspects 3 => super::repos::rustflags_in_an_inline_table,
    compiler_key_as_a_dotted_key: Fails[compiler!(1, "build.rustc-wrapper")], inspects 3 => super::repos::compiler_key_as_a_dotted_key,
    alias_as_a_dotted_key: Fails[alias!(2, "clippy")], inspects 3 => super::repos::alias_as_a_dotted_key,
    clippy_env_in_an_inline_table: Fails[clippy_env!(1, "CLIPPY_CONF_DIR")], inspects 3 => super::repos::clippy_env_in_an_inline_table,
    /// `include`, and the keys that replace or wrap the compiler.
    include_in_cargo_config: Fails[fails(
        ".cargo/config.toml:1",
        "`.cargo/config.toml` sets `include`, so cargo loads other config files that core-io does \
         not read, where `rustflags`, an alias or a `CLIPPY_` entry could turn off the bans of \
         clippy.toml and xtask/core-io-bans.toml",
        Fix::RemoveInclude {
            path: ".cargo/config.toml".to_owned(),
        },
    )], inspects 3 => super::repos::include_in_cargo_config,
    compiler_keys_in_cargo_config: Fails[
        compiler!(3, "build.rustc-wrapper"),
        compiler!(4, "build.rustc"),
        compiler!(5, "build.rustc-workspace-wrapper"),
    ], inspects 3 => super::repos::compiler_keys_in_cargo_config,
    /// An `[env]` entry starting with `CLIPPY_` (review D1).
    clippy_env_in_cargo_config: Fails[
        clippy_env!(3, "CLIPPY_CONF_DIR"),
        clippy_env!(4, "CLIPPY_ARGS"),
    ], inspects 3 => super::repos::clippy_env_in_cargo_config,
    /// Windows reads environment names ignoring case.
    clippy_env_in_another_case: Fails[clippy_env!(2, "Clippy_Conf_Dir")], inspects 3 => super::repos::clippy_env_in_another_case,
    cargo_config_without_lint_settings: Passes, inspects 3 => super::repos::cargo_config_without_lint_settings,
}
