//! The `dependencies` check: every workspace crate has a row in the Crates table of
//! `ARCHITECTURE.md`, and no crate depends on what its row lists under "Never depends on".
//!
//! A crate matches its row through `CratesTable::row_for_package`; one without a row fails at the
//! table's last row, where the new row goes. The count is the crates matched to a row. A row whose
//! crate does not exist is never looked at, whatever its status; a crate that exists is checked
//! whatever its status says.
//!
//! The Never entries that name a crate (ours, `clauseo-<name>`, or a crates.io package) are looked
//! up in the graph cargo resolved, from one `cargo metadata --all-features` run with deps (see
//! `graph.rs`): normal and build edges, for every target, optional dependencies included, directly
//! or through other crates. The I/O words belong to the core-io check.

mod graph;

use crate::checks::crates_table::{self, CratesTable};
use crate::checks::{self, Inspection, Noun, StaticCheck, violation};
use crate::violation::Violation;
use crate::workspace::{Context, Member};

/// Fails on a workspace crate without a row, and on a dependency its row forbids.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "dependencies",
    counts: Noun {
        one: "workspace crate with a Crates table row",
        many: "workspace crates with a Crates table row",
    },
    place: "in the Crates table of ARCHITECTURE.md",
    source: "xtask/src/checks/crates_table.rs",
    run,
};

/// `--locked`, so the check never rewrites `Cargo.lock`; `--offline`, so it never downloads.
/// `cargo xtask check` runs `deny` first, which fetches every target's crates; elsewhere a missing
/// crate fails with a fix that says to fetch it.
fn run(ctx: &Context) -> Inspection {
    check(ctx, Some(&["--locked", "--offline"]))
}

/// The check without the graph walk, for the real-repo test: that graph needs every target's
/// crates downloaded, and tests use no network. `tests.rs` covers the walk on scratch workspaces.
#[cfg(test)]
pub(crate) fn check_without_graph(ctx: &Context) -> Inspection {
    check(ctx, None)
}

/// The check, reading the resolved graph with `cargo metadata <graph_flags>` (after
/// `--format-version 1 --all-features`), or matching rows only when `graph_flags` is `None`.
fn check(ctx: &Context, graph_flags: Option<&[&str]>) -> Inspection {
    let table = match crates_table::load(&ctx.root) {
        Ok(table) => table,
        Err(error) => return Inspection::new(0, error.into_violations()),
    };
    let mut found = Vec::new();
    let mut matched = Vec::new();
    for member in &ctx.members {
        match table.row_for_package(&member.name) {
            Some(row) => matched.push((member, row)),
            None => found.push(no_row(&table, member)),
        }
    }
    let needs_graph = matched
        .iter()
        .any(|(_, row)| row.never.iter().any(|entry| entry.package().is_some()));
    if let (Some(flags), true) = (graph_flags, needs_graph) {
        match graph::Graph::load(&ctx.root, flags) {
            Ok(graph) => {
                for (member, row) in &matched {
                    let manifest = checks::join(&member.dir, "Cargo.toml");
                    match checks::read_repo_file(&ctx.root, &manifest) {
                        Ok(text) => found.extend(graph.violations(&graph::Crate {
                            package: &member.name,
                            manifest: &manifest,
                            text: text.as_deref().unwrap_or_default(),
                            row,
                        })),
                        Err(unreadable) => found.push(unreadable),
                    }
                }
            }
            Err(failed) => found.push(failed),
        }
    }
    found.sort_by(|a, b| (&a.path, a.line, &a.message).cmp(&(&b.path, b.line, &b.message)));
    found.dedup();
    Inspection::new(matched.len(), found)
}

/// The violation for `member`, which has no row: at the table's last row, where the new row goes.
fn no_row(table: &CratesTable, member: &Member) -> Violation {
    let line = table.rows.last().map_or(1, |row| row.line);
    let fix = match crates_table::row_name(&member.name).map(|name| name.replace('-', "_")) {
        Some(name) => format!(
            "add a row `| `{name}` | <what it owns> | <crates it may depend on> | <crates it never depends on> | exists |` to the Crates table in ARCHITECTURE.md"
        ),
        None => "rename the package to `clauseo-` followed by its row name (the table maps row `<name>` to package `clauseo-<name>`), then add that row to the Crates table in ARCHITECTURE.md with its five columns: Crate | Owns | May depend on | Never depends on | Status".to_owned(),
    };
    violation(
        crates_table::FILE,
        line,
        format!(
            "workspace crate `{}` ({}) has no row in the Crates table",
            member.name,
            if member.dir.is_empty() {
                "at the repo root"
            } else {
                member.dir.as_str()
            }
        ),
        fix,
    )
}

/// `a`, `a and b`, `a, b and c`.
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

crate::checks::planted::cases! {
    /// A crate that depends on tauri in `[dependencies]` fails at that line.
    direct: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::direct,
    /// tauri reached through two path crates fails at the direct dependency, naming the chain.
    transitive: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri through mid and inner, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `mid` into a crate whose row allows tauri, then remove `mid` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::transitive,
    /// tauri as a build dependency fails.
    build_dependency: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::build_dependency,
    /// tauri under another key (`t = { package = "tauri", ... }`) fails at that key.
    renamed: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `t` into a crate whose row allows tauri, then remove `t` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::renamed,
    /// tauri for Windows only fails on any host.
    target_windows: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::target_windows,
    /// `tauri.workspace = true` fails at the crate's own line.
    inherited: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::inherited,
    /// An optional tauri behind a feature fails, and the fix names the feature items too.
    optional_feature: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` and every `[features]` item that enables it from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::optional_feature,
    /// A workspace crate with no row fails at the table's last row.
    no_row: FailsWith(&[
        "ARCHITECTURE.md:7: workspace crate `clauseo-store` (crates/store) has no row in the Crates table\n    \
         fix: add a row `| `store` | <what it owns> | <crates it may depend on> | <crates it never depends on> | exists |` to the Crates table in ARCHITECTURE.md",
    ]) => tests::no_row,
    /// host depending on our agents crate, which its row forbids, fails.
    forbidden_workspace_crate: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on clauseo-agents, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `clauseo-agents` into a crate whose row allows clauseo-agents, then remove `clauseo-agents` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-host never depends on clauseo-agents",
    ]) => tests::forbidden_workspace_crate,
    /// A comment after the `[dependencies]` header still finds the entry's line.
    comment_after_header: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::comment_after_header,
    /// A quoted key (`'tauri' = ...`) still finds the entry's line.
    quoted_key: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::quoted_key,
    /// A `[dependencies.tauri]` table points at its header.
    table_header: FailsWith(&[
        "crates/protocol/Cargo.toml:6: clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `tauri` into a crate whose row allows tauri, then remove `tauri` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on tauri",
    ]) => tests::table_header,
    /// A package with a `-` and no row gets a row name the table accepts (`_`, not `-`).
    dash_package_without_row: FailsWith(&[
        "ARCHITECTURE.md:7: workspace crate `clauseo-fake-agent` (crates/fake_agent) has no row in the Crates table\n    \
         fix: add a row `| `fake_agent` | <what it owns> | <crates it may depend on> | <crates it never depends on> | exists |` to the Crates table in ARCHITECTURE.md",
    ]) => tests::dash_package_without_row,
    /// A ban on our `transport_ws` catches the package spelled `clauseo-transport-ws`.
    workspace_crate_spelled_with_dash: FailsWith(&[
        "crates/host/Cargo.toml:7: clauseo-host depends on clauseo-transport-ws, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `clauseo-transport-ws` into a crate whose row allows clauseo-transport-ws, then remove `clauseo-transport-ws` from crates/host/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-host never depends on clauseo-transport-ws",
    ]) => tests::workspace_crate_spelled_with_dash,
    /// A ban on `async_process` catches the package spelled `async-process`.
    crates_io_spelled_with_dash: FailsWith(&[
        "crates/protocol/Cargo.toml:7: clauseo-protocol depends on async-process, which ARCHITECTURE.md forbids\n    \
         fix: move the code that uses `async-process` into a crate whose row allows async-process, then remove `async-process` from crates/protocol/Cargo.toml: the row at ARCHITECTURE.md:7 says clauseo-protocol never depends on async-process",
    ]) => tests::crates_io_spelled_with_dash,
    /// A standard library module in the Never column fails at its row.
    std_module_in_never: FailsWith(&[
        "ARCHITECTURE.md:7: `net` in the Never depends on column of `protocol` names a standard library module, not a crate, so nothing would enforce it\n    \
         fix: write `any I/O` (or `fs` or `process`) instead of `net`: each bans the standard library's file, network, process, environment and clock APIs; `clock` alone bans only Instant::now and SystemTime::now",
    ]) => tests::std_module_in_never,
    /// tauri as a dev-dependency passes.
    dev_dependency_passes: Passes => tests::dev_dependency_passes,
    /// A planned row whose crate does not exist is skipped.
    planned_row_without_crate_is_skipped: Passes => tests::planned_row_without_crate_is_skipped,
    /// A workspace without a forbidden dependency passes.
    clean_workspace_passes: Passes => tests::clean_workspace_passes,
}

#[cfg(test)]
mod tests;
