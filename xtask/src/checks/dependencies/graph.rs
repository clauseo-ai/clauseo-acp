//! The graph walk of the `dependencies` check: is a package that a crate's row forbids anywhere
//! in that crate's own normal or build dependency graph?
//!
//! Each checked crate's graph comes from its own `cargo tree` run (see `tree_args`):
//! `-p <crate> --all-features`, so the crate's optional dependencies count but a feature that
//! another workspace member turns on does not (one workspace-wide resolve would merge them, and
//! blame the crate for an edge only the other member's build has); `--target all`, so every
//! target's dependencies count; `-e normal,build`, since dev-dependencies never reach the built
//! crate; `--no-dedupe`, so a package reached twice prints in full both times instead of as
//! `(*)`. With `--prefix depth --format {p}` each line is `<depth><name> v<version>`, then the
//! source in parentheses for a path or git package and `(proc-macro)` for a proc macro; a line
//! in any other shape fails the check. The depth-1 line above a forbidden package is the direct
//! dependency that brings it in. `cargo metadata --no-deps` gives that dependency's key, renames
//! and workspace inheritance resolved, so a violation points at the line of the crate's
//! `Cargo.toml` that declares it.

use super::and_list;
use crate::checks::crates_table::{self, CrateRow};
use crate::checks::graph::{ROOT_MANIFEST, cargo_stdout};
use crate::checks::toml_lines::{BUILT, Manifest};
use crate::checks::violation;
use crate::violation::Violation;
use crate::workspace::PackageDependencies;
use std::collections::BTreeMap;
use std::path::Path;

/// One workspace crate to walk from, with its manifest, its row and its declared dependencies.
pub(super) struct Crate<'a> {
    pub package: &'a str,
    /// The repo-relative path of its `Cargo.toml`.
    pub manifest: &'a str,
    /// The text of that manifest, read only for line numbers.
    pub text: &'a str,
    pub row: &'a CrateRow,
    /// Its dependencies from `cargo metadata --no-deps`, `None` when cargo listed none for it.
    pub deps: Option<&'a PackageDependencies>,
}

/// One line of the `cargo tree` output: a package at a depth, the crate itself at 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TreeLine {
    pub depth: usize,
    pub name: String,
}

/// The arguments of the `cargo tree` run for `package`, then `flags`.
pub(super) fn tree_args<'a>(package: &'a str, flags: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "tree",
        "-p",
        package,
        "-e",
        "normal,build",
        "--all-features",
        "--target",
        "all",
        "--prefix",
        "depth",
        "--no-dedupe",
        "--format",
        "{p}",
    ];
    args.extend_from_slice(flags);
    args
}

/// The dependency tree of the workspace crate `package`, from `cargo tree` run in `root` with
/// `flags` after `tree_args`. `Err` is the violation to report, at `Cargo.toml:1`.
pub(super) fn load(root: &Path, package: &str, flags: &[&str]) -> Result<Vec<TreeLine>, Violation> {
    let args = tree_args(package, flags);
    let stdout = cargo_stdout(root, &args)?;
    parse_tree(&stdout, package).map_err(|error| {
        violation(
            ROOT_MANIFEST,
            1,
            format!("could not read the output of `cargo {}`: {error}", args.join(" ")),
            "update `parse_tree` in xtask/src/checks/dependencies/graph.rs to the lines this cargo prints: the check reads each crate's dependency graph from them",
        )
    })
}

/// Reads `cargo tree --prefix depth --format {p}` output for `package`: its first line is
/// `package` at depth 0, and each later line at most one deeper than the line before it.
pub(super) fn parse_tree(stdout: &str, package: &str) -> Result<Vec<TreeLine>, String> {
    let mut lines: Vec<TreeLine> = Vec::new();
    for (index, text) in stdout.lines().enumerate() {
        let line = parse_line(text)
            .ok_or_else(|| format!("line {} `{text}` is not `<depth><name> v<version>` followed by `(<source>)` or `(proc-macro)`", index + 1))?;
        let deepest = lines.last().map_or(0, |last| last.depth + 1);
        let expected_root = lines.is_empty() && (line.depth != 0 || line.name != package);
        if expected_root || (!lines.is_empty() && (line.depth == 0 || line.depth > deepest)) {
            return Err(format!(
                "line {} `{text}` breaks the tree: the first line is `{package}` at depth 0, and each later line is one to {deepest} deep",
                index + 1
            ));
        }
        lines.push(line);
    }
    if lines.is_empty() {
        return Err(format!("it printed no line for `{package}`"));
    }
    Ok(lines)
}

/// One line, or `None` when it has another shape: `<depth><name> v<version>`, then
/// ` (proc-macro)` for a proc macro, then ` (<source>)` for a package not from crates.io (a
/// path may hold parentheses, so the source runs to the line's last `)`), as cargo 1.99's
/// `{p}` writes them. A trailing `(*)`, a subtree cargo did not repeat, is another shape: with
/// `--no-dedupe` it never prints, and the walk needs every subtree.
fn parse_line(text: &str) -> Option<TreeLine> {
    let name_start = text.find(|c: char| !c.is_ascii_digit())?;
    let depth = text.get(..name_start)?.parse().ok()?;
    let (name, rest) = text.get(name_start..)?.split_once(' ')?;
    let is_name_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if !name.chars().all(is_name_char) {
        return None;
    }
    let (version, rest) = rest.split_once(' ').unwrap_or((rest, ""));
    let version = version.strip_prefix('v')?;
    if !version.starts_with(|c: char| c.is_ascii_digit()) || rest.ends_with("(*)") {
        return None;
    }
    let rest = rest
        .strip_prefix("(proc-macro)")
        .unwrap_or(rest)
        .trim_start();
    if !rest.is_empty() {
        let source = rest.strip_prefix('(')?.strip_suffix(')')?;
        if source.is_empty() {
            return None;
        }
    }
    Some(TreeLine {
        depth,
        name: name.to_owned(),
    })
}

/// One violation per direct dependency of `krate` and package of its row's Never list that the
/// dependency is or brings in, for the crate's tree `lines`, through a shortest path.
pub(super) fn violations(krate: &Crate<'_>, lines: &[TreeLine]) -> Vec<Violation> {
    let forbidden = forbidden(krate.row);
    let mut path: Vec<&str> = Vec::new();
    let mut shortest: BTreeMap<(&str, String), Vec<&str>> = BTreeMap::new();
    for line in lines.iter().skip(1) {
        path.truncate(line.depth.saturating_sub(1));
        path.push(&line.name);
        let folded = crates_table::fold(&line.name);
        let Some(direct) = path.first().copied() else {
            continue;
        };
        if forbidden.contains(&folded) {
            let known = shortest
                .entry((direct, folded))
                .or_insert_with(|| path.clone());
            if known.len() > path.len() {
                known.clone_from(&path);
            }
        }
    }
    shortest
        .values()
        .map(|path| violation_for(krate, path))
        .collect()
}

/// The packages `row` bans, names folded (see `crates_table::fold`), sorted, each once.
pub(super) fn forbidden(row: &CrateRow) -> Vec<String> {
    let mut forbidden: Vec<String> = row
        .never
        .iter()
        .filter_map(crates_table::Never::package)
        .map(|package| crates_table::fold(&package))
        .collect();
    forbidden.sort();
    forbidden.dedup();
    forbidden
}

/// The violation for `names`, a path of package names from a direct dependency of `krate` to a
/// forbidden package.
fn violation_for(krate: &Crate<'_>, names: &[&str]) -> Violation {
    let package = names.last().copied().unwrap_or_default();
    let direct = names.first().copied().unwrap_or(package);
    let entries: Vec<_> = krate
        .deps
        .map(|deps| deps.dependencies.as_slice())
        .unwrap_or_default()
        .iter()
        .filter(|dep| dep.name == direct && !matches!(dep.kind.as_deref(), Some("dev")))
        .collect();
    let mut keys: Vec<String> = entries.iter().map(|dep| dep.key().to_owned()).collect();
    keys.sort();
    keys.dedup();
    if keys.is_empty() {
        keys.push(direct.to_owned());
    }
    let manifest = Manifest::parse(krate.text);
    let declared = keys
        .iter()
        .filter_map(|key| manifest.declaration(key, BUILT))
        .min();
    let through = match names.split_last() {
        Some((_, between)) if !between.is_empty() => {
            let between: Vec<String> = between.iter().map(|name| (*name).to_owned()).collect();
            format!(" through {}", and_list(&between))
        }
        _ => String::new(),
    };
    let quoted: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
    let quoted = and_list(&quoted);
    let unlocated = if declared.is_none() {
        format!(
            " (xtask found no line declaring {quoted} in {}, so this points at line 1)",
            krate.manifest
        )
    } else {
        String::new()
    };
    let features = if entries.iter().any(|dep| dep.optional) {
        " and every `[features]` item that enables it"
    } else {
        ""
    };
    violation(
        krate.manifest,
        declared.unwrap_or(1),
        format!(
            "{} depends on {package}{through}, which ARCHITECTURE.md forbids{unlocated}",
            krate.package
        ),
        format!(
            "move the code that uses {quoted} into a crate whose row allows {package}, then remove {quoted}{features} from {manifest}: the row at ARCHITECTURE.md:{row} says {krate} never depends on {package}",
            manifest = krate.manifest,
            row = krate.row.line,
            krate = krate.package,
        ),
    )
}
