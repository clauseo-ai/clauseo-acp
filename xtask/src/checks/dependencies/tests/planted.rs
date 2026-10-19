//! The planted cases of the dependencies check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A crate that depends on tauri in `[dependencies]` fails at that line.
    direct: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::direct,
    /// tauri reached through two path crates fails at the direct dependency, naming the chain.
    transitive: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri through mid and inner, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `mid` into a crate whose row allows tauri, then remove `mid` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::transitive,
    /// tauri as a build dependency fails.
    build_dependency: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::build_dependency,
    /// tauri under another key (`t = { package = "tauri", ... }`) fails at that key.
    renamed: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `t` into a crate whose row allows tauri, then remove `t` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::renamed,
    /// tauri for Windows only fails on any host.
    target_windows: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::target_windows,
    /// `tauri.workspace = true` fails at the crate's own line.
    inherited: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::inherited,
    /// An optional tauri behind a feature fails, and the fix names the feature items too.
    optional_feature: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` and every `[features]` item that enables it from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::optional_feature,
    /// A workspace crate with no row fails at the table's last row.
    no_row: FailsWith(&[
        "ARCHITECTURE.md:7: workspace crate `clauseo-store` (crates/store) has no row in the Crates table\n    \
         fix: add a row `| `store` | <what it owns> | <crates it may depend on> | <crates it never depends on> | exists |` to the Crates table in ARCHITECTURE.md",
    ]) => super::no_row,
    /// host depending on our agents crate, which its row forbids, fails.
    forbidden_workspace_crate: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on clauseo-agents, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `clauseo-agents` into a crate whose row allows clauseo-agents, then remove `clauseo-agents` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-host never depends on clauseo-agents",
    ]) => super::forbidden_workspace_crate,
    /// A comment after the `[dependencies]` header still finds the entry's line.
    comment_after_header: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::comment_after_header,
    /// A quoted key (`'tauri' = ...`) still finds the entry's line.
    quoted_key: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::quoted_key,
    /// A `[dependencies.tauri]` table points at its header.
    table_header: FailsWith(&[
        "crates/protocol/Cargo.toml:6: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::table_header,
    /// A package with a `-` and no row gets a row name the table accepts (`_`, not `-`).
    dash_package_without_row: FailsWith(&[
        "ARCHITECTURE.md:7: workspace crate `clauseo-fake-agent` (crates/fake_agent) has no row in the Crates table\n    \
         fix: add a row `| `fake_agent` | <what it owns> | <crates it may depend on> | <crates it never depends on> | exists |` to the Crates table in ARCHITECTURE.md",
    ]) => super::dash_package_without_row,
    /// A ban on our `transport_ws` catches the package spelled `clauseo-transport-ws`.
    workspace_crate_spelled_with_dash: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on clauseo-transport-ws, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `clauseo-transport-ws` into a crate whose row allows clauseo-transport-ws, then remove `clauseo-transport-ws` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-host never depends on clauseo-transport-ws",
    ]) => super::workspace_crate_spelled_with_dash,
    /// A ban on `async-process` catches the package spelled `async_process`.
    crates_io_spelled_with_underscore: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on async_process, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `async_process` into a crate whose row allows async_process, then remove `async_process` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on async_process",
    ]) => super::crates_io_spelled_with_underscore,
    /// A standard library module in the Never column fails at its row.
    std_module_in_never: FailsWith(&[
        "ARCHITECTURE.md:7: `net` in the Never depends on column of `protocol` names a standard library module, not a crate, so nothing would enforce it\n    \
         fix: write `any I/O` (or `fs` or `process`) instead of `net`: each bans the standard library's file, network, process, environment and clock APIs; `clock` alone bans only Instant::now and SystemTime::now",
        "0 workspace crates with a Crates table row inspected in the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::std_module_in_never,
    /// tauri as a dev-dependency passes.
    dev_dependency_passes: Passes => super::dev_dependency_passes,
    /// A planned row whose crate does not exist is skipped.
    planned_row_without_crate_is_skipped: Passes => super::planned_row_without_crate_is_skipped,
    /// A workspace without a forbidden dependency passes.
    clean_workspace_passes: Passes => super::clean_workspace_passes,
    /// A feature that only desktop turns on (`spec/tauri`) is no edge of protocol's graph.
    feature_turned_on_by_another_crate_passes: Passes => super::feature_turned_on_by_another_crate_passes,
    /// protocol turning on `spec/tauri` itself fails at its `spec` line.
    feature_turned_on_by_the_crate_itself: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri through spec, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `spec` into a crate whose row allows tauri, then remove `spec` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::feature_turned_on_by_the_crate_itself,
    /// A row without a leading pipe ends the table; the rows below it would be dropped.
    row_without_leading_pipe: FailsWith(&[
        "ARCHITECTURE.md:8: the Crates table ends at line 7, but line 8 after it holds a `|`, so the rows from there on are not read\n    \
         fix: start line 8 with `|` if it is a row (Markdown reads a row without a leading pipe, xtask/src/checks/crates_table.rs does not), else write it without a `|`",
        "0 workspace crates with a Crates table row inspected in the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::table::row_without_leading_pipe,
    /// `agent`, one edit from the row `agents`, is no crates.io ban.
    misspelt_row_in_never: FailsWith(&[
        "ARCHITECTURE.md:7: `agent` in the Never depends on column of `host` is one edit from the row `agents`, so it reads as a typo of it, and as written it would ban a crates.io package `agent` instead of our `clauseo-agents`\n    \
         fix: write `agents` to ban our crate `clauseo-agents`: xtask/src/checks/crates_table.rs rejects a name within one edit of a row name once `-` is read as `_`",
        "0 workspace crates with a Crates table row inspected in the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::table::misspelt_row_in_never,
    /// `clauseo-agents` bans the row `agents`'s crate.
    clauseo_prefix_names_a_row: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on clauseo-agents, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `clauseo-agents` into a crate whose row allows clauseo-agents, then remove `clauseo-agents` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-host never depends on clauseo-agents",
    ]) => super::table::clauseo_prefix_names_a_row,
    /// `clauseo-process` names the row `process`, and host does not depend on it.
    clauseo_prefixed_row_passes: Passes => super::table::clauseo_prefixed_row_passes,
    /// `clauseo-` followed by no row fails at the row.
    clauseo_prefix_without_row: FailsWith(&[
        "ARCHITECTURE.md:7: `clauseo-nosuchrow` in the Never depends on column of `host` starts with `clauseo-`, but `nosuchrow` is no row of the Crates table, so it names none of our crates\n    \
         fix: write the row name of the crate you mean (a name in the Crate column), or add a row `nosuchrow` for the crate",
        "0 workspace crates with a Crates table row inspected in the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::table::clauseo_prefix_without_row,
    /// A crates.io name written with `_` fails at the row.
    underscore_in_crates_io_name: FailsWith(&[
        "ARCHITECTURE.md:7: `serde_json` in the Never depends on column of `host` holds `_` but is no row of the Crates table, and only our row names are written with `_`\n    \
         fix: write `serde-json` for the crates.io package (cargo reads `-` and `_` in a package name the same), or the row name of the crate you mean if it is ours",
        "0 workspace crates with a Crates table row inspected in the Crates table of ARCHITECTURE.md\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::table::underscore_in_crates_io_name,
    /// An `exists` row with no workspace crate fails at the row.
    exists_row_without_crate: FailsWith(&[
        "ARCHITECTURE.md:8: the row `store` says `exists`, but no workspace crate is its package `clauseo-store`\n    \
         fix: change the Status to `planned`: a row is `exists` only while its crate is a workspace member",
    ]) => super::table::exists_row_without_crate,
    /// A `planned` row whose crate is a workspace member fails at the row.
    planned_row_with_crate: FailsWith(&[
        "ARCHITECTURE.md:7: the row `protocol` says `planned`, but its crate `clauseo-protocol` (crates/protocol) is a workspace member\n    \
         fix: change the Status to `exists`: the crate exists",
    ]) => super::table::planned_row_with_crate,
    /// tauri reached through two direct dependencies fails at each (`--no-dedupe` keeps both).
    two_paths_each_fail: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri through left and inner, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `left` into a crate whose row allows tauri, then remove `left` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
        "crates/protocol/Cargo.toml:8: clauseo-protocol depends on tauri through right and inner, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `right` into a crate whose row allows tauri, then remove `right` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => super::tree::two_paths_each_fail,
}
