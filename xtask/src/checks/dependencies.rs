//! The `dependencies` check: every workspace crate has a row in the Crates table of
//! `ARCHITECTURE.md`, and no crate depends on what its row lists under "Never depends on".
//!
//! A crate matches its row through `CratesTable::row_for_package`; one without a row fails at the
//! table's last row, where the new row goes. The count is the crates matched to a row. The Status
//! column must be true: an `exists` row with no workspace crate, and a `planned` row whose crate
//! is a workspace member, each fail at the row. A crate that exists is checked whatever its
//! status says.
//!
//! The Never entries that name a crate (ours, `clauseo-<name>`, or a crates.io package) are looked
//! up in each checked crate's own graph, from one `cargo tree -p <crate> --all-features` run per
//! crate whose row bans a crate (see `graph.rs`): normal and build edges, for every target, the
//! crate's optional dependencies included, directly or through other crates, and no feature that
//! only another member turns on. The I/O words belong to the core-io check.

mod graph;

use crate::checks::crates_table::{self, CrateRow, CratesTable, Status};
use crate::checks::{self, Inspection, Noun, StaticCheck, violation};
use crate::violation::Violation;
use crate::workspace::{self, Context, Member, PackageDependencies};
use std::collections::BTreeMap;
use std::path::Path;

/// Fails on a workspace crate without a row, and on a dependency its row forbids.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "dependencies",
    counts: Noun {
        one: "workspace crate with a Crates table row",
        many: "workspace crates with a Crates table row",
    },
    place: || format!("in the Crates table of {}", crates_table::FILE),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// `--locked`, so the check never rewrites `Cargo.lock`; `--offline`, so it never downloads.
/// `cargo xtask check` runs `deny` first, which fetches every target's crates; elsewhere a missing
/// crate fails with a fix that says to fetch it. The graph walk runs only when
/// `ctx.load_resolved_graph` allows it; `tests.rs` covers it on scratch workspaces.
fn run(ctx: &Context) -> Inspection {
    check(
        ctx,
        ctx.load_resolved_graph
            .then_some(&["--locked", "--offline"][..]),
    )
}

/// The check, reading each checked crate's graph with `cargo tree` (`graph::tree_args`, then
/// `graph_flags`), or matching rows only when `graph_flags` is `None`.
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
    found.extend(status_violations(&table, &matched));
    let walked: Vec<_> = matched
        .iter()
        .filter(|(_, row)| !graph::forbidden(row).is_empty())
        .collect();
    if let (Some(flags), false) = (graph_flags, walked.is_empty()) {
        match declared_dependencies(&ctx.root) {
            Ok(declared) => {
                for (member, row) in walked {
                    found.extend(walk(
                        &ctx.root,
                        member,
                        row,
                        declared.get(&member.name),
                        flags,
                    ));
                }
            }
            Err(failed) => found.push(failed),
        }
    }
    found.sort_by(|a, b| (&a.path, a.line, &a.message).cmp(&(&b.path, b.line, &b.message)));
    found.dedup();
    Inspection::new(matched.len(), found)
}

/// The dependencies of every workspace member, by package, from `cargo metadata --no-deps`.
fn declared_dependencies(root: &Path) -> Result<BTreeMap<String, PackageDependencies>, Violation> {
    let args = ["metadata", "--format-version", "1", "--no-deps"];
    let stdout = checks::graph::cargo_stdout(root, &args)?;
    workspace::parse_workspace_dependencies(&stdout).map_err(|error| {
        violation(
            checks::graph::ROOT_MANIFEST,
            1,
            format!("could not read the output of `cargo {}`: {error}", args.join(" ")),
            "update xtask/src/workspace.rs to the JSON this cargo writes: this check reads each crate's dependencies from it",
        )
    })
}

/// The violations in the graph of `member`, whose row is `row` and whose declared dependencies
/// are `deps`: its `cargo tree` run with `flags`, or the failure to read it.
fn walk(
    root: &Path,
    member: &Member,
    row: &CrateRow,
    deps: Option<&PackageDependencies>,
    flags: &[&str],
) -> Vec<Violation> {
    let manifest = checks::join(&member.dir, "Cargo.toml");
    let text = match checks::read_repo_file(root, &manifest) {
        Ok(text) => text.unwrap_or_default(),
        Err(unreadable) => return vec![unreadable],
    };
    match graph::load(root, &member.name, flags) {
        Ok(lines) => graph::violations(
            &graph::Crate {
                package: &member.name,
                manifest: &manifest,
                text: &text,
                row,
                deps,
            },
            &lines,
        ),
        Err(failed) => vec![failed],
    }
}

/// One violation per row whose Status is wrong: `exists` with no workspace crate, or `planned`
/// for a crate of `matched`, the members with the row each matched.
fn status_violations(table: &CratesTable, matched: &[(&Member, &CrateRow)]) -> Vec<Violation> {
    let mut found = Vec::new();
    for row in &table.rows {
        let member = matched
            .iter()
            .find(|(_, matched_row)| matched_row.line == row.line)
            .map(|(member, _)| *member);
        match (row.status, member) {
            (Status::Exists, None) => found.push(violation(
                crates_table::FILE,
                row.line,
                format!(
                    "the row `{}` says `exists`, but no workspace crate is its package `{}`",
                    row.name,
                    crates_table::package_of(&row.name)
                ),
                "change the Status to `planned`: a row is `exists` only while its crate is a workspace member",
            )),
            (Status::Planned, Some(member)) => found.push(violation(
                crates_table::FILE,
                row.line,
                format!(
                    "the row `{}` says `planned`, but its crate `{}` ({}) is a workspace member",
                    row.name,
                    member.name,
                    place(member)
                ),
                "change the Status to `exists`: the crate exists",
            )),
            _ => {}
        }
    }
    found
}

/// Where `member` lives, for a message: its directory, or `at the repo root`.
fn place(member: &Member) -> &str {
    if member.dir.is_empty() {
        "at the repo root"
    } else {
        member.dir.as_str()
    }
}

/// The violation for `member`, which has no row: at the table's last row, where the new row goes.
fn no_row(table: &CratesTable, member: &Member) -> Violation {
    let line = table.rows.last().map_or(1, |row| row.line);
    let fix = match crates_table::row_name(&member.name).map(crates_table::fold) {
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
            place(member)
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
