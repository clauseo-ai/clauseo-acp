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
    /// A ban on `async_process` catches the package spelled `async-process`.
    crates_io_spelled_with_dash: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on async-process, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `async-process` into a crate whose row allows async-process, then remove `async-process` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on async-process",
    ]) => super::crates_io_spelled_with_dash,
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
}
