//! The planted cases of the dependencies check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). Their builders are in `tests/repos.rs`.

use super::{host_move, protocol_move};
use crate::checks::crates_table;
use crate::checks::dependencies::Fix;
use crate::checks::planted::fails;

crate::checks::planted::cases! {
    /// A crate that depends on tauri in `[dependencies]` fails at that line.
    direct: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        protocol_move("tauri", "tauri"),
    )], inspects 1 => super::repos::direct,
    /// tauri reached through two path crates fails at the direct dependency, naming the chain.
    transitive: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri through mid and inner, which ARCHITECTURE.md forbids",
        protocol_move("mid", "tauri"),
    )], inspects 1 => super::repos::transitive,
    /// tauri as a build dependency fails.
    build_dependency: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        protocol_move("tauri", "tauri"),
    )], inspects 1 => super::repos::build_dependency,
    /// tauri under another key (`t = { package = "tauri", ... }`) fails at that key.
    renamed: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        protocol_move("t", "tauri"),
    )], inspects 1 => super::repos::renamed,
    /// tauri for Windows only fails on any host.
    target_windows: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        protocol_move("tauri", "tauri"),
    )], inspects 1 => super::repos::target_windows,
    /// `tauri.workspace = true` fails at the crate's own line.
    inherited: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        protocol_move("tauri", "tauri"),
    )], inspects 1 => super::repos::inherited,
    /// An optional tauri behind a feature fails, and the fix names the feature items too.
    optional_feature: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        Fix::MoveFromManifest {
            keys: "`tauri`".to_owned(),
            package: "tauri".to_owned(),
            manifest: "crates/protocol/Cargo.toml".to_owned(),
            features: true,
            row: 7,
            krate: "clauseo-protocol".to_owned(),
        },
    )], inspects 1 => super::repos::optional_feature,
    /// A workspace crate with no row fails at the table's last row.
    no_row: Fails[fails(
        "ARCHITECTURE.md:7",
        "workspace crate `clauseo-store` (crates/store) has no row in the Crates table",
        Fix::AddRow {
            name: "store".to_owned(),
        },
    )], inspects 1 => super::repos::no_row,
    /// host depending on our agents crate, which its row forbids, fails.
    forbidden_workspace_crate: Fails[fails(
        "crates/host/Cargo.toml:7",
        "clauseo-host depends on clauseo-agents, which ARCHITECTURE.md forbids",
        host_move("clauseo-agents", "clauseo-agents"),
    )], inspects 2 => super::repos::forbidden_workspace_crate,
    /// A comment after the `[dependencies]` header still finds the entry's line.
    comment_after_header: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        protocol_move("tauri", "tauri"),
    )], inspects 1 => super::repos::comment_after_header,
    /// A quoted key (`'tauri' = ...`) still finds the entry's line.
    quoted_key: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        protocol_move("tauri", "tauri"),
    )], inspects 1 => super::repos::quoted_key,
    /// A `[dependencies.tauri]` table points at its header.
    table_header: Fails[fails(
        "crates/protocol/Cargo.toml:6",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        protocol_move("tauri", "tauri"),
    )], inspects 1 => super::repos::table_header,
    /// A package with a `-` and no row gets a row name the table accepts (`_`, not `-`).
    dash_package_without_row: Fails[fails(
        "ARCHITECTURE.md:7",
        "workspace crate `clauseo-fake-agent` (crates/fake_agent) has no row in the Crates table",
        Fix::AddRow {
            name: "fake_agent".to_owned(),
        },
    )], inspects 1 => super::repos::dash_package_without_row,
    /// A ban on our `transport_ws` catches the package spelled `clauseo-transport-ws`.
    workspace_crate_spelled_with_dash: Fails[fails(
        "crates/host/Cargo.toml:7",
        "clauseo-host depends on clauseo-transport-ws, which ARCHITECTURE.md forbids",
        host_move("clauseo-transport-ws", "clauseo-transport-ws"),
    )], inspects 2 => super::repos::workspace_crate_spelled_with_dash,
    /// A ban on `async-process` catches the package spelled `async_process`.
    crates_io_spelled_with_underscore: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on async_process, which ARCHITECTURE.md forbids",
        protocol_move("async_process", "async_process"),
    )], inspects 1 => super::repos::crates_io_spelled_with_underscore,
    /// A standard library module in the Never column fails at its row.
    std_module_in_never: Fails[fails(
        "ARCHITECTURE.md:7",
        "`net` in the Never depends on column of `protocol` names a standard library module, not \
        a crate, so nothing would enforce it",
        crates_table::Fix::StdModule {
            entry: "net".to_owned(),
        },
    )], inspects 0 => super::repos::std_module_in_never,
    /// tauri as a dev-dependency passes.
    dev_dependency_passes: Passes, inspects 1 => super::repos::dev_dependency_passes,
    /// A planned row whose crate does not exist is skipped.
    planned_row_without_crate_is_skipped: Passes, inspects 1
        => super::repos::planned_row_without_crate_is_skipped,
    /// A workspace without a forbidden dependency passes.
    clean_workspace_passes: Passes, inspects 1 => super::repos::clean_workspace_passes,
    /// A feature that only desktop turns on (`spec/tauri`) is no edge of protocol's graph.
    feature_turned_on_by_another_crate_passes: Passes, inspects 2
        => super::repos::feature_turned_on_by_another_crate_passes,
    /// protocol turning on `spec/tauri` itself fails at its `spec` line.
    feature_turned_on_by_the_crate_itself: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri through spec, which ARCHITECTURE.md forbids",
        protocol_move("spec", "tauri"),
    )], inspects 2 => super::repos::feature_turned_on_by_the_crate_itself,
    /// A row without a leading pipe is a row, as GitHub shows it: host's ban on agents, written
    /// in such a row, still fails host's dependency on clauseo-agents.
    row_without_leading_pipe: Fails[fails(
        "crates/host/Cargo.toml:7",
        "clauseo-host depends on clauseo-agents, which ARCHITECTURE.md forbids",
        Fix::MoveFromManifest {
            keys: "`clauseo-agents`".to_owned(),
            package: "clauseo-agents".to_owned(),
            manifest: "crates/host/Cargo.toml".to_owned(),
            features: false,
            row: 8,
            krate: "clauseo-host".to_owned(),
        },
    )], inspects 1 => super::repos::row_without_leading_pipe,
    /// Prose with a `|` in a code span after the table is not a row.
    prose_with_a_pipe_after_the_table_passes: Passes, inspects 1
        => super::repos::prose_with_a_pipe_after_the_table_passes,
    /// `agent`, one edit from the row `agents`, is no crates.io ban.
    misspelt_row_in_never: Fails[fails(
        "ARCHITECTURE.md:7",
        "`agent` in the Never depends on column of `host` is one edit from the row `agents`, so \
        it reads as a typo of it, and as written it would ban a crates.io package `agent` \
        instead of our `clauseo-agents`",
        crates_table::Fix::TypoBan {
            ban: "agents".to_owned(),
            package: "clauseo-agents".to_owned(),
            entry: "agent".to_owned(),
        },
    )], inspects 0 => super::repos::misspelt_row_in_never,
    /// `clauseo-agents` bans the row `agents`'s crate.
    clauseo_prefix_names_a_row: Fails[fails(
        "crates/host/Cargo.toml:7",
        "clauseo-host depends on clauseo-agents, which ARCHITECTURE.md forbids",
        host_move("clauseo-agents", "clauseo-agents"),
    )], inspects 1 => super::repos::clauseo_prefix_names_a_row,
    /// `clauseo-process` names the row `process`, and host does not depend on it.
    clauseo_prefixed_row_passes: Passes, inspects 1 => super::repos::clauseo_prefixed_row_passes,
    /// `clauseo-` followed by no row fails at the row.
    clauseo_prefix_without_row: Fails[fails(
        "ARCHITECTURE.md:7",
        "`clauseo-nosuchrow` in the Never depends on column of `host` starts with `clauseo-`, but \
        `nosuchrow` is no row of the Crates table, so it names none of our crates",
        crates_table::Fix::NoRow {
            near: Vec::new(),
            row: "nosuchrow".to_owned(),
            entry: "clauseo-nosuchrow".to_owned(),
        },
    )], inspects 0 => super::repos::clauseo_prefix_without_row,
    /// A crates.io name written with `_` fails at the row.
    underscore_in_crates_io_name: Fails[fails(
        "ARCHITECTURE.md:7",
        "`serde_json` in the Never depends on column of `host` holds `_` but is no row of the \
        Crates table, and only our row names are written with `_`",
        crates_table::Fix::Underscore {
            dashed: "serde-json".to_owned(),
        },
    )], inspects 0 => super::repos::underscore_in_crates_io_name,
    /// An `exists` row with no workspace crate fails at the row.
    exists_row_without_crate: Fails[fails(
        "ARCHITECTURE.md:8",
        "the row `store` says `exists`, but no workspace crate is its package `clauseo-store`",
        Fix::SetPlanned,
    )], inspects 1 => super::repos::exists_row_without_crate,
    /// A `planned` row whose crate is a workspace member fails at the row.
    planned_row_with_crate: Fails[fails(
        "ARCHITECTURE.md:7",
        "the row `protocol` says `planned`, but its crate `clauseo-protocol` (crates/protocol) is \
        a workspace member",
        Fix::SetExists,
    )], inspects 1 => super::repos::planned_row_with_crate,
    /// tauri reached through two direct dependencies fails at each (`--no-dedupe` keeps both).
    two_paths_each_fail: Fails[
        fails(
            "crates/protocol/Cargo.toml:7",
            "clauseo-protocol depends on tauri through left and inner, which ARCHITECTURE.md \
            forbids",
            protocol_move("left", "tauri"),
        ),
        fails(
            "crates/protocol/Cargo.toml:8",
            "clauseo-protocol depends on tauri through right and inner, which ARCHITECTURE.md \
            forbids",
            protocol_move("right", "tauri"),
        ),
    ], inspects 1 => super::repos::two_paths_each_fail,
    /// A package built for the host with a feature its target build lacks is two nodes: the
    /// package that feature pulls in is blamed on the build dependency only.
    host_build_feature_blames_only_its_path: Fails[fails(
        "crates/protocol/Cargo.toml:10",
        "clauseo-protocol depends on b through y and foo, which ARCHITECTURE.md forbids",
        protocol_move("y", "b"),
    )], inspects 1 => super::repos::host_build_feature_blames_only_its_path,
    /// `s` declared twice, with `f1` in `[dependencies]` and `f2` in `[build-dependencies]`, where
    /// only `f2` brings b in, blames the build line.
    declared_twice_blames_the_building_line: Fails[fails(
        "crates/protocol/Cargo.toml:10",
        "clauseo-protocol depends on b through s, which ARCHITECTURE.md forbids",
        protocol_move("s", "b"),
    )], inspects 1 => super::repos::declared_twice_blames_the_building_line,
    /// `s` declared twice, where both declarations bring b in, names both lines in the message
    /// and the fix, since removing one would leave b in.
    declared_twice_names_both_lines: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on b through s, which ARCHITECTURE.md forbids. `s` is declared \
        at lines 7 and 10 of crates/protocol/Cargo.toml, and each declaration brings `b` in",
        Fix::MoveFromLines {
            keys: "`s`".to_owned(),
            package: "b".to_owned(),
            lines: "7 and 10".to_owned(),
            manifest: "crates/protocol/Cargo.toml".to_owned(),
            features: false,
            row: 7,
            krate: "clauseo-protocol".to_owned(),
        },
    )], inspects 1 => super::repos::declared_twice_names_both_lines,
    /// `s` declared in `[dependencies]` with `f1` and in a `[target.'cfg(windows)'.dependencies]`
    /// table with `f3`, where only `f3` brings b in. Cargo builds both lines as one package, so
    /// the violation says one or more of them brings b in and names both lines in the fix.
    declared_twice_in_one_section: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on b through s, which ARCHITECTURE.md forbids. `s` is declared \
        at lines 7 and 10 of crates/protocol/Cargo.toml. Cargo builds these declarations as one \
        package, and one or more of them brings `b` in",
        Fix::MoveFromLines {
            keys: "`s`".to_owned(),
            package: "b".to_owned(),
            lines: "7 and 10".to_owned(),
            manifest: "crates/protocol/Cargo.toml".to_owned(),
            features: false,
            row: 7,
            krate: "clauseo-protocol".to_owned(),
        },
    )], inspects 1 => super::repos::declared_twice_in_one_section,
    /// `s1` and `s2` declare two path packages named `s`, and only `s` 2.0.0, at line 10, brings
    /// b in. Each declaration is matched to its own package by its path, so only line 10 is
    /// blamed, by its key `s2`.
    two_paths_of_one_name_blame_the_one_that_brings_it_in: Fails[fails(
        "crates/protocol/Cargo.toml:10",
        "clauseo-protocol depends on b through s, which ARCHITECTURE.md forbids",
        protocol_move("s2", "b"),
    )], inspects 1 => super::repos::two_paths_of_one_name_blame_the_one_that_brings_it_in,
    /// Both packages named `s` bring b in, so the violation names both keys and both lines.
    two_paths_of_one_name_both_bring_it_in: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on b through s, which ARCHITECTURE.md forbids. `s1` and `s2` \
        are declared at lines 7 and 10 of crates/protocol/Cargo.toml, and each declaration \
        brings `b` in",
        Fix::MoveFromLines {
            keys: "`s1` and `s2`".to_owned(),
            package: "b".to_owned(),
            lines: "7 and 10".to_owned(),
            manifest: "crates/protocol/Cargo.toml".to_owned(),
            features: false,
            row: 7,
            krate: "clauseo-protocol".to_owned(),
        },
    )], inspects 1 => super::repos::two_paths_of_one_name_both_bring_it_in,
    /// tauri reached through another member, whose row bans nothing, fails at that member's line.
    through_another_member: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri through clauseo-util, which ARCHITECTURE.md forbids",
        protocol_move("clauseo-util", "tauri"),
    )], inspects 2 => super::repos::through_another_member,
    /// A registry dependency `[patch.crates-io]` swaps for a crate that brings tauri in fails.
    patched_registry_dependency: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri through serde, which ARCHITECTURE.md forbids",
        protocol_move("serde", "tauri"),
    )], inspects 1 => super::repos::patched_registry_dependency,
    crates_io_marker_passes: Passes, inspects 1 => super::repos::crates_io_marker_passes,
    crates_io_marker_bans_the_package: Fails[fails(
        "crates/host/Cargo.toml:7",
        "clauseo-host depends on rmcp, which ARCHITECTURE.md forbids",
        host_move("rmcp", "rmcp"),
    )], inspects 1 => super::repos::crates_io_marker_bans_the_package,
    crates_io_name_one_edit_from_a_row: Fails[fails(
        "ARCHITECTURE.md:7",
        "`rmcp` in the Never depends on column of `host` is one edit from the row `mcp`, so it \
        reads as a typo of it, and as written it would ban a crates.io package `rmcp` instead of \
        our `clauseo-mcp`",
        crates_table::Fix::TypoBan {
            ban: "mcp".to_owned(),
            package: "clauseo-mcp".to_owned(),
            entry: "rmcp".to_owned(),
        },
    )], inspects 0 => super::repos::crates_io_name_one_edit_from_a_row,
    row_name_in_capitals: Fails[fails(
        "ARCHITECTURE.md:7",
        "`AGENTS` in the Never depends on column of `host` is the row `agents` written with other \
        letter case, and as written it would ban a crates.io package `AGENTS` instead of our \
        `clauseo-agents`",
        crates_table::Fix::TypoBan {
            ban: "agents".to_owned(),
            package: "clauseo-agents".to_owned(),
            entry: "AGENTS".to_owned(),
        },
    )], inspects 0 => super::repos::row_name_in_capitals,
    package_name_in_other_case: Fails[fails(
        "ARCHITECTURE.md:7",
        "`Clauseo-agents` in the Never depends on column of `host` is the package \
        `clauseo-agents` written with other letter case",
        crates_table::Fix::OwnPackage {
            package: "clauseo-agents".to_owned(),
            row: Some("agents".to_owned()),
            entry: "Clauseo-agents".to_owned(),
        },
    )], inspects 0 => super::repos::package_name_in_other_case,
    row_name_with_two_letters_swapped: Fails[fails(
        "ARCHITECTURE.md:7",
        "`agnets` in the Never depends on column of `host` is one edit from the row `agents`, so \
        it reads as a typo of it, and as written it would ban a crates.io package `agnets` \
        instead of our `clauseo-agents`",
        crates_table::Fix::TypoBan {
            ban: "agents".to_owned(),
            package: "clauseo-agents".to_owned(),
            entry: "agnets".to_owned(),
        },
    )], inspects 0 => super::repos::row_name_with_two_letters_swapped,
    clauseo_before_a_row_whose_package_has_no_prefix: Fails[fails(
        "ARCHITECTURE.md:7",
        "`clauseo-xtask` in the Never depends on column of `host` is not our package: the row \
        `xtask`'s package is `xtask`",
        crates_table::Fix::Ban {
            ban: "xtask".to_owned(),
            package: "xtask".to_owned(),
            entry: "clauseo-xtask".to_owned(),
        },
    )], inspects 0 => super::repos::clauseo_before_a_row_whose_package_has_no_prefix,
    /// `Tauri` bans the package `tauri`: crates.io names are unique ignoring case.
    crates_io_name_in_capitals_bans_the_package: Fails[fails(
        "crates/protocol/Cargo.toml:7",
        "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids",
        protocol_move("tauri", "tauri"),
    )], inspects 1 => super::repos::crates_io_name_in_capitals_bans_the_package,
    /// A typo of the row `process` is told to write `clauseo-process`: a bare `process` is the
    /// I/O word.
    typo_of_a_row_named_like_an_io_word: Fails[fails(
        "ARCHITECTURE.md:7",
        "`procesz` in the Never depends on column of `host` is one edit from the row `process`, \
        so it reads as a typo of it, and as written it would ban a crates.io package `procesz` \
        instead of our `clauseo-process`",
        crates_table::Fix::TypoBan {
            ban: "clauseo-process".to_owned(),
            package: "clauseo-process".to_owned(),
            entry: "procesz".to_owned(),
        },
    )], inspects 0 => super::repos::typo_of_a_row_named_like_an_io_word,
    /// `std` or a std module in other letter case (`Std`, `NET`) is no crates.io ban either.
    std_module_in_other_case: Fails[
        fails(
            "ARCHITECTURE.md:7",
            "`Std` in the Never depends on column of `host` names a standard library module, not \
            a crate, so nothing would enforce it",
            crates_table::Fix::StdModule {
                entry: "Std".to_owned(),
            },
        ),
        fails(
            "ARCHITECTURE.md:7",
            "`NET` in the Never depends on column of `host` names a standard library module, not \
            a crate, so nothing would enforce it",
            crates_table::Fix::StdModule {
                entry: "NET".to_owned(),
            },
        ),
    ], inspects 0 => super::repos::std_module_in_other_case,
    /// `clauseo_` with nothing after it names no row.
    clauseo_prefix_alone: Fails[fails(
        "ARCHITECTURE.md:7",
        "`clauseo_` in the Never depends on column of `host` is `clauseo_` with no row name after \
        it, so it names none of our crates",
        crates_table::Fix::NoRowName {
            entry: "clauseo_".to_owned(),
        },
    )], inspects 0 => super::repos::clauseo_prefix_alone,
    /// The package of row `transport_ws` is `clauseo-transport_ws`, and the fix names it so.
    underscore_row_package_after_clauseo_underscore: Fails[fails(
        "ARCHITECTURE.md:7",
        "`clauseo_transport_ws` in the Never depends on column of `host` is the package \
        `clauseo-transport_ws` written with `_` after `clauseo`",
        crates_table::Fix::OwnPackage {
            package: "clauseo-transport_ws".to_owned(),
            row: Some("transport_ws".to_owned()),
            entry: "clauseo_transport_ws".to_owned(),
        },
    )], inspects 0 => super::repos::underscore_row_package_after_clauseo_underscore,
    /// An I/O word in other letter case (`FS`, `Clock`, `Any I/O`) is no crates.io ban.
    io_word_in_other_case: Fails[
        fails(
            "ARCHITECTURE.md:7",
            "`FS` in the Never depends on column of `host` is the I/O word `fs` written with \
            other letter case, and as written it would ban a crates.io package instead",
            crates_table::Fix::IoWordCase {
                exact: "fs",
                entry: Some("FS".to_owned()),
            },
        ),
        fails(
            "ARCHITECTURE.md:7",
            "`Clock` in the Never depends on column of `host` is the I/O word `clock` written \
            with other letter case, and as written it would ban a crates.io package instead",
            crates_table::Fix::IoWordCase {
                exact: "clock",
                entry: Some("Clock".to_owned()),
            },
        ),
        fails(
            "ARCHITECTURE.md:7",
            "`Any I/O` in the Never depends on column of `host` is the I/O word `any I/O` written \
            with other letter case",
            crates_table::Fix::IoWordCase {
                exact: "any I/O",
                entry: None,
            },
        ),
    ], inspects 0 => super::repos::io_word_in_other_case,
}
