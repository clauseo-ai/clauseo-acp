//! The planted cases of the core-io check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

/// The text of the violation for a banned path at `crates/protocol/src/<file>:<line>`.
#[cfg(test)]
macro_rules! uses {
    ($file:literal, $line:literal, $banned:literal) => {
        concat!(
            "crates/protocol/src/",
            $file,
            ":",
            $line,
            ": clauseo-protocol uses ",
            $banned,
            ", which ARCHITECTURE.md forbids in this crate\n    fix: take what ",
            $banned,
            " gives as a parameter, or move this code to a crate that may do I/O, or add \
             `crates/protocol/src/",
            $file,
            " <reason>` to xtask/core-io-exceptions.txt"
        )
    };
}

/// The text of the violation for a glob import of `$prefix` in `lib.rs`.
#[cfg(test)]
macro_rules! glob {
    ($line:literal, $prefix:literal, $reaches:expr) => {
        concat!(
            "crates/protocol/src/lib.rs:",
            $line,
            ": clauseo-protocol glob-imports ",
            $prefix,
            "::*, which reaches ",
            $reaches,
            "; ARCHITECTURE.md forbids that in this crate\n    \
             fix: import the items you use from ",
            $prefix,
            " by name instead, or add \
             `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt"
        )
    };
}

/// The text of the violation for a path built from a metavariable in `lib.rs`.
#[cfg(test)]
macro_rules! metavariable {
    ($line:literal, $written:literal, $reaches:expr) => {
        concat!(
            "crates/protocol/src/lib.rs:", $line, ": clauseo-protocol builds `", $written, "` from a \
             macro metavariable, which may expand to ", $reaches, "; core-io cannot see the \
             expansion, and ARCHITECTURE.md forbids those in this crate\n    fix: write the path \
             out in full instead of building it from a metavariable, or add \
             `crates/protocol/src/lib.rs <reason>` to xtask/core-io-exceptions.txt"
        )
    };
}

/// Every banned path, as a glob of `std` lists them.
#[cfg(test)]
macro_rules! all_bans {
    () => {
        "std::fs, std::net, std::process, std::env, std::os::*::fs, std::os::*::net, \
         std::os::*::process, std::os::*::prelude, std::time::Instant::now, \
         std::time::SystemTime::now"
    };
}

crate::checks::planted::cases! {
    // Each banned module, reached every way the brief and the reviews list.
    fs_use: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::fs_use,
    fs_group: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::fs_group,
    fs_nested: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::fs_nested,
    fs_std_alias: FailsWith(&[uses!("lib.rs", 4, "std::fs")]) => super::fs_std_alias,
    fs_leading_colon: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => super::fs_leading_colon,
    fs_qualified: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => super::fs_qualified,
    fs_extern_crate: FailsWith(&[uses!("lib.rs", 4, "std::fs")]) => super::fs_extern_crate,
    fs_fn_body: FailsWith(&[uses!("lib.rs", 2, "std::fs"), uses!("lib.rs", 3, "std::fs")]) => super::fs_fn_body,
    fs_inline_mod: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => super::fs_inline_mod,
    fs_glob: FailsWith(&[glob!(1, "std", all_bans!())]) => super::fs_glob,
    fs_macro: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => super::fs_macro,
    fs_self_alias: FailsWith(&[uses!("lib.rs", 4, "std::fs")]) => super::fs_self_alias,
    fs_rename: FailsWith(&[uses!("lib.rs", 1, "std::fs"), uses!("lib.rs", 4, "std::fs")]) => super::fs_rename,
    fs_value: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => super::fs_value,
    fs_raw: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => super::fs_raw,
    net_use: FailsWith(&[uses!("lib.rs", 1, "std::net")]) => super::net_use,
    net_group: FailsWith(&[uses!("lib.rs", 1, "std::net")]) => super::net_group,
    net_nested: FailsWith(&[uses!("lib.rs", 1, "std::net")]) => super::net_nested,
    net_std_alias: FailsWith(&[uses!("lib.rs", 4, "std::net")]) => super::net_std_alias,
    net_leading_colon: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => super::net_leading_colon,
    net_qualified: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => super::net_qualified,
    net_extern_crate: FailsWith(&[uses!("lib.rs", 4, "std::net")]) => super::net_extern_crate,
    net_fn_body: FailsWith(&[uses!("lib.rs", 2, "std::net"), uses!("lib.rs", 3, "std::net")]) => super::net_fn_body,
    net_inline_mod: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => super::net_inline_mod,
    net_glob: FailsWith(&[glob!(1, "std", all_bans!())]) => super::net_glob,
    net_macro: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => super::net_macro,
    net_self_alias: FailsWith(&[uses!("lib.rs", 4, "std::net")]) => super::net_self_alias,
    net_rename: FailsWith(&[uses!("lib.rs", 1, "std::net"), uses!("lib.rs", 4, "std::net")]) => super::net_rename,
    net_value: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => super::net_value,
    net_raw: FailsWith(&[uses!("lib.rs", 2, "std::net")]) => super::net_raw,
    process_use: FailsWith(&[uses!("lib.rs", 1, "std::process")]) => super::process_use,
    process_group: FailsWith(&[uses!("lib.rs", 1, "std::process")]) => super::process_group,
    process_nested: FailsWith(&[uses!("lib.rs", 1, "std::process")]) => super::process_nested,
    process_std_alias: FailsWith(&[uses!("lib.rs", 4, "std::process")]) => super::process_std_alias,
    process_leading_colon: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => super::process_leading_colon,
    process_qualified: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => super::process_qualified,
    process_extern_crate: FailsWith(&[uses!("lib.rs", 4, "std::process")]) => super::process_extern_crate,
    process_fn_body: FailsWith(&[uses!("lib.rs", 2, "std::process"), uses!("lib.rs", 3, "std::process")]) => super::process_fn_body,
    process_inline_mod: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => super::process_inline_mod,
    process_glob: FailsWith(&[glob!(1, "std", all_bans!())]) => super::process_glob,
    process_macro: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => super::process_macro,
    process_self_alias: FailsWith(&[uses!("lib.rs", 4, "std::process")]) => super::process_self_alias,
    process_rename: FailsWith(&[uses!("lib.rs", 1, "std::process"), uses!("lib.rs", 4, "std::process")]) => super::process_rename,
    process_value: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => super::process_value,
    process_raw: FailsWith(&[uses!("lib.rs", 2, "std::process")]) => super::process_raw,
    env_use: FailsWith(&[uses!("lib.rs", 1, "std::env")]) => super::env_use,
    env_group: FailsWith(&[uses!("lib.rs", 1, "std::env")]) => super::env_group,
    env_nested: FailsWith(&[uses!("lib.rs", 1, "std::env")]) => super::env_nested,
    env_std_alias: FailsWith(&[uses!("lib.rs", 4, "std::env")]) => super::env_std_alias,
    env_leading_colon: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => super::env_leading_colon,
    env_qualified: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => super::env_qualified,
    env_extern_crate: FailsWith(&[uses!("lib.rs", 4, "std::env")]) => super::env_extern_crate,
    env_fn_body: FailsWith(&[uses!("lib.rs", 2, "std::env"), uses!("lib.rs", 3, "std::env")]) => super::env_fn_body,
    env_inline_mod: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => super::env_inline_mod,
    env_glob: FailsWith(&[glob!(1, "std", all_bans!())]) => super::env_glob,
    env_macro: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => super::env_macro,
    env_self_alias: FailsWith(&[uses!("lib.rs", 4, "std::env")]) => super::env_self_alias,
    env_rename: FailsWith(&[uses!("lib.rs", 1, "std::env"), uses!("lib.rs", 4, "std::env")]) => super::env_rename,
    env_value: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => super::env_value,
    env_raw: FailsWith(&[uses!("lib.rs", 2, "std::env")]) => super::env_raw,
    // Each banned clock.
    instant_use: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => super::instant_use,
    instant_rename: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => super::instant_rename,
    instant_type_alias: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => super::instant_type_alias,
    instant_qualified: FailsWith(&[uses!("lib.rs", 2, "std::time::Instant::now")]) => super::instant_qualified,
    instant_glob: FailsWith(&[glob!(1, "std::time", "std::time::Instant::now, std::time::SystemTime::now")]) => super::instant_glob,
    instant_self_in_impl: FailsWith(&[uses!("lib.rs", 7, "std::time::Instant::now")]) => super::instant_self_in_impl,
    instant_paren_alias: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => super::instant_paren_alias,
    system_time_use: FailsWith(&[uses!("lib.rs", 4, "std::time::SystemTime::now")]) => super::system_time_use,
    system_time_rename: FailsWith(&[uses!("lib.rs", 4, "std::time::SystemTime::now")]) => super::system_time_rename,
    system_time_type_alias: FailsWith(&[uses!("lib.rs", 4, "std::time::SystemTime::now")]) => super::system_time_type_alias,
    system_time_qualified: FailsWith(&[uses!("lib.rs", 2, "std::time::SystemTime::now")]) => super::system_time_qualified,
    system_time_glob: FailsWith(&[glob!(1, "std::time", "std::time::Instant::now, std::time::SystemTime::now")]) => super::system_time_glob,
    system_time_self_in_impl: FailsWith(&[uses!("lib.rs", 7, "std::time::SystemTime::now")]) => super::system_time_self_in_impl,
    system_time_paren_alias: FailsWith(&[uses!("lib.rs", 4, "std::time::SystemTime::now")]) => super::system_time_paren_alias,
    /// Raw identifiers count without their `r#`.
    raw_std: FailsWith(&[uses!("lib.rs", 2, "std::fs")]) => super::raw_std,
    raw_use: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::raw_use,
    instant_raw_now: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => super::instant_raw_now,
    /// An associated type bound to a clock, through `<Z as K>::C` and `Self::C`.
    associated_type_qualified: FailsWith(&[uses!("lib.rs", 14, "std::time::Instant::now")]) => super::associated_type_qualified,
    associated_type_self: FailsWith(&[uses!("lib.rs", 11, "std::time::Instant::now")]) => super::associated_type_self,
    /// The platform modules under `std::os`.
    os_unix_fs: FailsWith(&[uses!("lib.rs", 2, "std::os::unix::fs")]) => super::os_unix_fs,
    os_unix_net: FailsWith(&[uses!("lib.rs", 1, "std::os::unix::net")]) => super::os_unix_net,
    os_unix_process: FailsWith(&[uses!("lib.rs", 1, "std::os::unix::process")]) => super::os_unix_process,
    os_windows_fs: FailsWith(&[uses!("lib.rs", 2, "std::os::windows::fs")]) => super::os_windows_fs,
    os_glob: FailsWith(&[glob!(1, "std::os", "std::os::*::fs, std::os::*::net, std::os::*::process, std::os::*::prelude")]) => super::os_glob,
    os_unix_glob: FailsWith(&[glob!(1, "std::os::unix", "std::os::unix::fs, std::os::unix::net, std::os::unix::process, std::os::unix::prelude")]) => super::os_unix_glob,
    /// Paths a macro metavariable starts or continues.
    metavariable_module: FailsWith(&[metavariable!(3, "std::$x", all_bans!())]) => super::metavariable_module,
    metavariable_now: FailsWith(&[metavariable!(3, "$t::now", "std::time::Instant::now, std::time::SystemTime::now")]) => super::metavariable_now,
    metavariable_qualified_now: FailsWith(&[metavariable!(3, "$t::now", "std::time::Instant::now, std::time::SystemTime::now")]) => super::metavariable_qualified_now,
    /// Bindings made inside macro bodies that parse.
    macro_block_use: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => super::macro_block_use,
    macro_rules_item: FailsWith(&[uses!("lib.rs", 10, "std::time::Instant::now")]) => super::macro_rules_item,
    /// `extern crate self as me;` names the crate root.
    extern_crate_self: FailsWith(&[uses!("lib.rs", 5, "std::fs")]) => super::extern_crate_self,
    /// Each crate root has its own `crate::`.
    bin_crate_root: FailsWith(&[uses!("bin/x.rs", 5, "std::fs")]) => super::bin_crate_root,
    main_crate_root: FailsWith(&[uses!("m.rs", 2, "std::fs")]) => super::main_crate_root,
    /// Sites across files and lines.
    use_then_call: FailsWith(&[uses!("lib.rs", 1, "std::fs"), uses!("lib.rs", 4, "std::fs")]) => super::use_then_call,
    alias_in_another_file: FailsWith(&[uses!("lib.rs", 4, "std::time::Instant::now")]) => super::alias_in_another_file,
    std_reexported_from_the_root: FailsWith(&[uses!("a.rs", 2, "std::fs")]) => super::std_reexported_from_the_root,
    /// Paths a metavariable feeds into a `type` alias or a `use`.
    metavariable_type_binding: FailsWith(&[metavariable!(5, "$t::now", "std::time::Instant::now, std::time::SystemTime::now")]) => super::metavariable_type_binding,
    metavariable_use_binding: FailsWith(&[metavariable!(5, "$p::fs::read", "std::fs")]) => super::metavariable_use_binding,
    /// `include!` through an alias, and `core::include!`.
    include_alias: FailsWith(&[
        "crates/protocol/src/lib.rs:3: clauseo-protocol pulls in code with include!, which core-io \
         does not parse\n    \
         fix: turn the included file into a module under `crates/protocol/src/` (`mod name;`) \
         instead of include!, so core-io parses it",
    ]) => super::include_alias,
    core_include: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol pulls in code with include!, which core-io \
         does not parse\n    \
         fix: turn the included file into a module under `crates/protocol/src/` (`mod name;`) \
         instead of include!, so core-io parses it",
    ]) => super::core_include,
    /// A row with only `clock` bans the clocks' `now` and nothing else.
    clock_only_row: FailsWith(&[uses!("lib.rs", 3, "std::time::Instant::now")]) => super::clock_only_row,
    /// Code the check would not parse fails on its own.
    path_attribute: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol loads module `io` from `../io.rs` \
         with #[path], and core-io parses only the module files Rust finds without it
    \
         fix: drop #[path] and keep the module as `io.rs` or `io/mod.rs` beside its \
         parent module under `crates/protocol/src/`, so core-io parses it",
    ]) => super::path_attribute,
    include_macro: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol pulls in code with include!, \
         which core-io does not parse\n    \
         fix: turn the included file into a module under \
         `crates/protocol/src/` (`mod name;`) instead of include!, so core-io parses it",
    ]) => super::include_macro,
    lib_path_outside_src: FailsWith(&[
        "crates/protocol/Cargo.toml:5: clauseo-protocol's [lib] path `lib.rs` is outside \
         `crates/protocol/src/`, and core-io parses only that folder\n    \
         fix: move the \
         target's root file under `crates/protocol/src/` and set `path` to it",
    ]) => super::lib_path_outside_src,
    bin_path_outside_src: FailsWith(&[
        "crates/protocol/Cargo.toml:6: clauseo-protocol's [[bin]] path `tools/main.rs` is \
         outside `crates/protocol/src/`, and core-io parses only that folder\n    \
         fix: \
         move the target's root file under `crates/protocol/src/` and set `path` to it",
    ]) => super::bin_path_outside_src,
    /// Problems with the inputs.
    does_not_parse: FailsWith(&[
        "crates/protocol/src/lib.rs:3: `crates/protocol/src/lib.rs` does not parse as \
         Rust (cannot parse string into token stream), so core-io cannot check it
    \
         fix: fix the syntax; if rustc accepts the file, report that syn cannot parse it, \
         so xtask can be fixed",
    ]) => super::does_not_parse,
    no_sources: FailsWith(&[
        "ARCHITECTURE.md:8: clauseo-protocol has no .rs file under \
         `crates/protocol/src/`, so core-io cannot check that it stays free of std I/O \
         (Never depends on: fs, process)\n    \
         fix: keep clauseo-protocol's Rust sources \
         under `crates/protocol/src/`, where core-io parses them",
    ]) => super::no_sources,
    stale_exception: FailsWith(&[
        "xtask/core-io-exceptions.txt:2: `crates/protocol/src/gone.rs` is not a file in \
         this repo, so its core-io exception is stale\n    \
         fix: delete this line, or \
         correct the path if the file moved",
    ]) => super::stale_exception,
    exception_outside_held_crates: FailsWith(&[
        "xtask/core-io-exceptions.txt:1: `xtask/core-io-exceptions.txt` is in no crate \
         whose row bans I/O, so its core-io exception is stale\n    \
         fix: delete this line",
    ]) => super::exception_outside_held_crates,
    /// `std::time::Duration` and `std::io::Error` are allowed.
    allowed_items: Passes => super::allowed_items,
    /// `Instant` named only as a type is allowed.
    instant_as_a_type: Passes => super::instant_as_a_type,
    /// A file listed in the exception file is not checked.
    listed_exception: Passes => super::listed_exception,
    /// A local `fs` module and another crate's `time::Instant` are not std's.
    local_names_like_banned_ones: Passes => super::local_names_like_banned_ones,
    /// The review's forms in `macro_rules!` bodies: a use group, a rename, two metavariables
    /// and `Self`.
    macro_rule_use_group: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::macro_rule_use_group,
    macro_rule_std_alias: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::macro_rule_std_alias,
    macro_rule_two_metavariables: FailsWith(&[metavariable!(1, "$a::$b::read", all_bans!())]) => super::macro_rule_two_metavariables,
    macro_rule_self_now: FailsWith(&[metavariable!(9, "$t::now", "std::time::Instant::now, std::time::SystemTime::now")]) => super::macro_rule_self_now,
    /// A `$vis` before an item keeps the rule body parseable.
    macro_rule_visibility: FailsWith(&[uses!("lib.rs", 4, "std::fs"), uses!("lib.rs", 5, "std::fs")]) => super::macro_rule_visibility,
    /// A repetition `$( ... )*` is read as its inner tokens.
    macro_rule_repetition: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::macro_rule_repetition,
    /// The paths inside `mod $m` are not built from `$m`.
    macro_rule_module_named_by_a_metavariable: Passes => super::macro_rule_module_named_by_a_metavariable,
    /// A glob of a path through std that a metavariable continues.
    metavariable_glob: FailsWith(&[metavariable!(3, "std::$x", all_bans!())]) => super::metavariable_glob,
    /// An associated type or an alias bound to a metavariable fails only when used.
    macro_rule_newtype_deref: Passes => super::macro_rule_newtype_deref,
    macro_rule_ops_two_types: Passes => super::macro_rule_ops_two_types,
    metavariable_alias_unused: Passes => super::metavariable_alias_unused,
    /// An associated type pinned in a bound, and a local type named like an associated one.
    associated_type_in_a_bound: FailsWith(&[uses!("lib.rs", 6, "std::time::Instant::now")]) => super::associated_type_in_a_bound,
    associated_name_like_a_local_type: Passes => super::associated_name_like_a_local_type,
    /// `core::net` is no I/O, and `std::env!` is the compile-time macro.
    core_net: Passes => super::core_net,
    std_env_macro: Passes => super::std_env_macro,
    /// A `cfg_attr` without `path` on a `mod` keeps the module in its crate root.
    cfg_attr_on_a_mod: FailsWith(&[uses!("m.rs", 2, "std::fs")]) => super::cfg_attr_on_a_mod,
    /// `std::os::<any>::prelude` re-exports the platform I/O traits.
    os_prelude_glob: FailsWith(&[glob!(1, "std::os::unix::prelude", "std::os::unix::prelude")]) => super::os_prelude_glob,
    /// Rule bodies that parse only with each metavariable replaced by its fragment kind: a
    /// `block` and a `lifetime`.
    rule_block_use_group: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::rule_block_use_group,
    rule_block_std_alias: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::rule_block_std_alias,
    rule_lifetime: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::rule_lifetime,
    /// A rule body that parses as no code fails closed.
    rule_body_does_not_parse: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol has a macro_rules! rule whose body \
         core-io cannot parse, even with each metavariable replaced by a placeholder of its kind, \
         so core-io cannot check it\n    \
         fix: restructure the rule so its body is items, statements, expressions or a type (move \
         the rest into a helper rule or a function), or add `crates/protocol/src/lib.rs <reason>` \
         to xtask/core-io-exceptions.txt",
    ]) => super::rule_body_does_not_parse,
    /// A repetition joined by `::` builds a path of any length.
    rule_repetition_builds_a_path: FailsWith(&[metavariable!(1, "$p::$p::read", all_bans!())]) => super::rule_repetition_builds_a_path,
    /// `Self::now` in an inherent impl that defines `now` is that method.
    rule_defines_its_own_now: Passes => super::rule_defines_its_own_now,
    /// `T::C` and `Self::C` read the binding in scope, not one of the same name elsewhere.
    associated_type_bound_in_scope: Passes => super::associated_type_bound_in_scope,
    associated_type_self_in_scope: Passes => super::associated_type_self_in_scope,
    /// `#[path]` inside nested `cfg_attr`s.
    cfg_attr_nested_path: FailsWith(&[
        "crates/protocol/src/lib.rs:1: clauseo-protocol loads module `io` from `../io.rs` \
         with #[path], and core-io parses only the module files Rust finds without it\n    \
         fix: drop #[path] and keep the module as `io.rs` or `io/mod.rs` beside its \
         parent module under `crates/protocol/src/`, so core-io parses it",
    ]) => super::cfg_attr_nested_path,
    /// Rule bodies that are a pattern, match arms or named fields are read, not failed.
    rule_pattern_ref: Passes => super::rule_pattern_ref,
    rule_pattern_tuple: Passes => super::rule_pattern_tuple,
    rule_pattern_binding: Passes => super::rule_pattern_binding,
    rule_match_arms: Passes => super::rule_match_arms,
    rule_match_arms_are_read: FailsWith(&[uses!("lib.rs", 1, "std::fs")]) => super::rule_match_arms_are_read,
    rule_struct_fields: Passes => super::rule_struct_fields,
    /// A repetition that starts with `::` builds a path of any length from the crate root.
    rule_leading_colon_repetition: FailsWith(&[metavariable!(1, "$p::$p::read", all_bans!())]) => super::rule_leading_colon_repetition,
}
