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

#[cfg(test)]
mod tests;
