//! The planted cases of the dependencies check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A crate that depends on tauri in `[dependencies]` fails at that line.
    direct: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::direct,
    /// tauri reached through two path crates fails at the direct dependency, naming the chain.
    transitive: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri through mid and inner, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `mid` into a crate whose row allows tauri, then remove `mid` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::transitive,
    /// tauri as a build dependency fails.
    build_dependency: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::build_dependency,
    /// tauri under another key (`t = { package = "tauri", ... }`) fails at that key.
    renamed: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `t` into a crate whose row allows tauri, then remove `t` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::renamed,
    /// tauri for Windows only fails on any host.
    target_windows: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::target_windows,
    /// `tauri.workspace = true` fails at the crate's own line.
    inherited: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::inherited,
    /// An optional tauri behind a feature fails, and the fix names the feature items too.
    optional_feature: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` and every `[features]` item that enables it from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::optional_feature,
    /// A workspace crate with no row fails at the table's last row.
    no_row: FailsWith(&[
        "ARCHITECTURE.md:7: workspace crate `clauseo-store` (crates/store) has no row in the Crates table\n    \
         fix: add this row to the Crates table in ARCHITECTURE.md: | `store` | <what it owns> | <crates it may depend on> | <crates it never depends on> | exists |",
    ]), inspects 1 => super::no_row,
    /// host depending on our agents crate, which its row forbids, fails.
    forbidden_workspace_crate: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on clauseo-agents, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `clauseo-agents` into a crate whose row allows clauseo-agents, then remove `clauseo-agents` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-host never depends on clauseo-agents",
    ]), inspects 2 => super::forbidden_workspace_crate,
    /// A comment after the `[dependencies]` header still finds the entry's line.
    comment_after_header: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::comment_after_header,
    /// A quoted key (`'tauri' = ...`) still finds the entry's line.
    quoted_key: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::quoted_key,
    /// A `[dependencies.tauri]` table points at its header.
    table_header: FailsWith(&[
        "crates/protocol/Cargo.toml:6: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::table_header,
    /// A package with a `-` and no row gets a row name the table accepts (`_`, not `-`).
    dash_package_without_row: FailsWith(&[
        "ARCHITECTURE.md:7: workspace crate `clauseo-fake-agent` (crates/fake_agent) has no row in the Crates table\n    \
         fix: add this row to the Crates table in ARCHITECTURE.md: | `fake_agent` | <what it owns> | <crates it may depend on> | <crates it never depends on> | exists |",
    ]), inspects 1 => super::dash_package_without_row,
    /// A ban on our `transport_ws` catches the package spelled `clauseo-transport-ws`.
    workspace_crate_spelled_with_dash: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on clauseo-transport-ws, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `clauseo-transport-ws` into a crate whose row allows clauseo-transport-ws, then remove `clauseo-transport-ws` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-host never depends on clauseo-transport-ws",
    ]), inspects 2 => super::workspace_crate_spelled_with_dash,
    /// A ban on `async-process` catches the package spelled `async_process`.
    crates_io_spelled_with_underscore: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on async_process, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `async_process` into a crate whose row allows async_process, then remove `async_process` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on async_process",
    ]), inspects 1 => super::crates_io_spelled_with_underscore,
    /// A standard library module in the Never column fails at its row.
    std_module_in_never: FailsWith(&[
        "ARCHITECTURE.md:7: `net` in the Never depends on column of `protocol` names a standard library module, not a crate, so nothing would enforce it\n    \
         fix: write `any I/O` (or `fs` or `process`) instead of `net`: each bans the standard library's file, network, process, environment and clock APIs; `clock` alone bans only Instant::now and SystemTime::now",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::std_module_in_never,
    /// tauri as a dev-dependency passes.
    dev_dependency_passes: Passes, inspects 1 => super::dev_dependency_passes,
    /// A planned row whose crate does not exist is skipped.
    planned_row_without_crate_is_skipped: Passes, inspects 1
        => super::planned_row_without_crate_is_skipped,
    /// A workspace without a forbidden dependency passes.
    clean_workspace_passes: Passes, inspects 1 => super::clean_workspace_passes,
    /// A feature that only desktop turns on (`spec/tauri`) is no edge of protocol's graph.
    feature_turned_on_by_another_crate_passes: Passes, inspects 2
        => super::feature_turned_on_by_another_crate_passes,
    /// protocol turning on `spec/tauri` itself fails at its `spec` line.
    feature_turned_on_by_the_crate_itself: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri through spec, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `spec` into a crate whose row allows tauri, then remove `spec` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 2 => super::feature_turned_on_by_the_crate_itself,
    /// A row without a leading pipe is a row, as GitHub shows it: host's ban on agents, written
    /// in such a row, still fails host's dependency on clauseo-agents.
    row_without_leading_pipe: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on clauseo-agents, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `clauseo-agents` into a crate whose row allows clauseo-agents, then remove `clauseo-agents` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:8 says clauseo-host never depends on clauseo-agents",
    ]), inspects 1 => super::table::row_without_leading_pipe,
    /// Prose with a `|` in a code span after the table is not a row.
    prose_with_a_pipe_after_the_table_passes: Passes, inspects 1
        => super::table::prose_with_a_pipe_after_the_table_passes,
    /// `agent`, one edit from the row `agents`, is no crates.io ban.
    misspelt_row_in_never: FailsWith(&[
        "ARCHITECTURE.md:7: `agent` in the Never depends on column of `host` is one edit from the row `agents`, so it reads as a typo of it, and as written it would ban a crates.io package `agent` instead of our `clauseo-agents`\n    \
         fix: write `agents` to ban our crate `clauseo-agents`, or `agent (crates.io)` if you mean the crates.io package: the module doc of xtask/src/checks/crates_table.rs lists which names read as ours",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::table::misspelt_row_in_never,
    /// `clauseo-agents` bans the row `agents`'s crate.
    clauseo_prefix_names_a_row: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on clauseo-agents, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `clauseo-agents` into a crate whose row allows clauseo-agents, then remove `clauseo-agents` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-host never depends on clauseo-agents",
    ]), inspects 1 => super::table::clauseo_prefix_names_a_row,
    /// `clauseo-process` names the row `process`, and host does not depend on it.
    clauseo_prefixed_row_passes: Passes, inspects 1 => super::table::clauseo_prefixed_row_passes,
    /// `clauseo-` followed by no row fails at the row.
    clauseo_prefix_without_row: FailsWith(&[
        "ARCHITECTURE.md:7: `clauseo-nosuchrow` in the Never depends on column of `host` starts with `clauseo-`, but `nosuchrow` is no row of the Crates table, so it names none of our crates\n    \
         fix: write the name that bans the crate you mean (a name in the Crate column), add a row `nosuchrow` for the crate, or write `clauseo-nosuchrow (crates.io)` if you mean the crates.io package",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::table::clauseo_prefix_without_row,
    /// A crates.io name written with `_` fails at the row.
    underscore_in_crates_io_name: FailsWith(&[
        "ARCHITECTURE.md:7: `serde_json` in the Never depends on column of `host` holds `_` but is no row of the Crates table, and only our row names are written with `_`\n    \
         fix: write `serde-json` for the crates.io package (this check reads `-` and `_` the same), or the row name of the crate you mean if it is ours",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::table::underscore_in_crates_io_name,
    /// An `exists` row with no workspace crate fails at the row.
    exists_row_without_crate: FailsWith(&[
        "ARCHITECTURE.md:8: the row `store` says `exists`, but no workspace crate is its package `clauseo-store`\n    \
         fix: change the Status to `planned`: a row is `exists` only while its crate is a workspace member",
    ]), inspects 1 => super::table::exists_row_without_crate,
    /// A `planned` row whose crate is a workspace member fails at the row.
    planned_row_with_crate: FailsWith(&[
        "ARCHITECTURE.md:7: the row `protocol` says `planned`, but its crate `clauseo-protocol` (crates/protocol) is a workspace member\n    \
         fix: change the Status to `exists`: the crate exists",
    ]), inspects 1 => super::table::planned_row_with_crate,
    /// tauri reached through two direct dependencies fails at each (`--no-dedupe` keeps both).
    two_paths_each_fail: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri through left and inner, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `left` into a crate whose row allows tauri, then remove `left` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
        "crates/protocol/Cargo.toml:8: clauseo-protocol depends on tauri through right and inner, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `right` into a crate whose row allows tauri, then remove `right` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::tree::two_paths_each_fail,
    crates_io_marker_passes: Passes, inspects 1 => super::names::crates_io_marker_passes,
    crates_io_marker_bans_the_package: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on rmcp, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `rmcp` into a crate whose row allows rmcp, then remove `rmcp` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-host never depends on rmcp",
    ]), inspects 1 => super::names::crates_io_marker_bans_the_package,
    crates_io_name_one_edit_from_a_row: FailsWith(&[
        "ARCHITECTURE.md:7: `rmcp` in the Never depends on column of `host` is one edit from the row `mcp`, so it reads as a typo of it, and as written it would ban a crates.io package `rmcp` instead of our `clauseo-mcp`\n    \
         fix: write `mcp` to ban our crate `clauseo-mcp`, or `rmcp (crates.io)` if you mean the crates.io package: the module doc of xtask/src/checks/crates_table.rs lists which names read as ours",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::names::crates_io_name_one_edit_from_a_row,
    row_name_in_capitals: FailsWith(&[
        "ARCHITECTURE.md:7: `AGENTS` in the Never depends on column of `host` is the row `agents` written with other letter case, and as written it would ban a crates.io package `AGENTS` instead of our `clauseo-agents`\n    \
         fix: write `agents` to ban our crate `clauseo-agents`, or `AGENTS (crates.io)` if you mean the crates.io package: the module doc of xtask/src/checks/crates_table.rs lists which names read as ours",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::names::row_name_in_capitals,
    package_name_in_other_case: FailsWith(&[
        "ARCHITECTURE.md:7: `Clauseo-agents` in the Never depends on column of `host` is the package `clauseo-agents` written with other letter case\n    \
         fix: write `clauseo-agents` (or the row name `agents`) to ban our crate, or `Clauseo-agents (crates.io)` if you mean the crates.io package",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::names::package_name_in_other_case,
    row_name_with_two_letters_swapped: FailsWith(&[
        "ARCHITECTURE.md:7: `agnets` in the Never depends on column of `host` is one edit from the row `agents`, so it reads as a typo of it, and as written it would ban a crates.io package `agnets` instead of our `clauseo-agents`\n    \
         fix: write `agents` to ban our crate `clauseo-agents`, or `agnets (crates.io)` if you mean the crates.io package: the module doc of xtask/src/checks/crates_table.rs lists which names read as ours",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::names::row_name_with_two_letters_swapped,
    clauseo_before_a_row_whose_package_has_no_prefix: FailsWith(&[
        "ARCHITECTURE.md:7: `clauseo-xtask` in the Never depends on column of `host` is not our package: the row `xtask`'s package is `xtask`\n    \
         fix: write `xtask` to ban our crate `xtask`, or `clauseo-xtask (crates.io)` if you mean the crates.io package",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::names::clauseo_before_a_row_whose_package_has_no_prefix,
    /// `Tauri` bans the package `tauri`: crates.io names are unique ignoring case.
    crates_io_name_in_capitals_bans_the_package: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]), inspects 1 => super::names::crates_io_name_in_capitals_bans_the_package,
    /// A typo of the row `process` is told to write `clauseo-process`: a bare `process` is the
    /// I/O word.
    typo_of_a_row_named_like_an_io_word: FailsWith(&[
        "ARCHITECTURE.md:7: `procesz` in the Never depends on column of `host` is one edit from the row `process`, so it reads as a typo of it, and as written it would ban a crates.io package `procesz` instead of our `clauseo-process`\n    \
         fix: write `clauseo-process` to ban our crate `clauseo-process`, or `procesz (crates.io)` if you mean the crates.io package: the module doc of xtask/src/checks/crates_table.rs lists which names read as ours",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::names::typo_of_a_row_named_like_an_io_word,
    /// An I/O word in other letter case (`FS`, `Clock`, `Any I/O`) is no crates.io ban.
    io_word_in_other_case: FailsWith(&[
        "ARCHITECTURE.md:7: `FS` in the Never depends on column of `host` is the I/O word `fs` written with other letter case, and as written it would ban a crates.io package instead\n    \
         fix: write `fs` to ban the standard library's APIs it names, or `FS (crates.io)` if you mean the crates.io package",
        "ARCHITECTURE.md:7: `Clock` in the Never depends on column of `host` is the I/O word `clock` written with other letter case, and as written it would ban a crates.io package instead\n    \
         fix: write `clock` to ban the standard library's APIs it names, or `Clock (crates.io)` if you mean the crates.io package",
        "ARCHITECTURE.md:7: `Any I/O` in the Never depends on column of `host` is the I/O word `any I/O` written with other letter case\n    \
         fix: write `any I/O` to ban the standard library's APIs it names",
        "0 workspace crates with a Crates table row inspected among the workspace members and the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]), inspects 0 => super::names::io_word_in_other_case,
}
