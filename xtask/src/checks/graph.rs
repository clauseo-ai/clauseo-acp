//! A workspace crate's dependency graph, for the checks that read one: `dependencies` walks the
//! graph of each crate whose row bans a crate, `plugins` walks the desktop crate's. A failed
//! cargo run becomes one violation at `Cargo.toml:1`, with a fix for the cause cargo names.
//!
//! The graph comes from the crate's own `cargo tree` run (see `tree_args`): `-p <crate>
//! --all-features`, so the crate's optional dependencies count but a feature that only another
//! workspace member turns on does not (one workspace-wide resolve would merge them, and blame the
//! crate for an edge only the other member's build has); `--target all`, so every target's
//! dependencies count; `-e normal,build`, since dev-dependencies never reach the built crate.
//! With `--prefix depth --format {p}` each line is `<depth><name> v<version>`, then
//! `(proc-macro)` for a proc macro, then the source in parentheses for a path or git package,
//! then ` (*)` for a package printed before. Cargo prints a package's subtree in full the first
//! time and marks each later line of it `(*)`, so the graph maps each line's text, without the
//! `(*)`, to the packages under it, and a `(*)` line is a reference to that entry. The output
//! stays the size of the graph, not of the number of paths through it. A package that prints in
//! full twice (built for the host and for a target with different features) gets the packages
//! under both, so the graph never misses an edge, and may show one the other build alone has.
//! A line in any other shape fails the check.

use crate::checks::violation;
use crate::process::{self, Cmd, Program};
use crate::violation::Violation;
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

/// The root manifest, where a failed run points.
pub(crate) const ROOT_MANIFEST: &str = "Cargo.toml";

/// The stdout of `cargo <args>` run in `root`; `Err` is the violation to report when cargo
/// cannot be started or fails.
pub(crate) fn cargo_stdout(root: &Path, args: &[&str]) -> Result<String, Violation> {
    let cmd = Cmd::new(Program::Cargo, args.iter().copied());
    let quiet = process::run_quiet(&cmd, root).map_err(|error| {
        violation(
            ROOT_MANIFEST,
            1,
            format!("could not run `{cmd}`: {error}"),
            "put `cargo` on PATH: this check reads the dependency graph from cargo",
        )
    })?;
    if quiet.success {
        Ok(quiet.stdout)
    } else {
        let stderr = quiet.stderr.trim();
        Err(violation(
            ROOT_MANIFEST,
            1,
            format!("`{cmd}` failed: {stderr}"),
            failure_fix(&cmd, stderr),
        ))
    }
}

/// The fix for a failed run of `cmd` whose stderr is `stderr`, true for the cause cargo names.
fn failure_fix(cmd: &Cmd, stderr: &str) -> String {
    if stderr.contains("--locked was passed") {
        "Cargo.lock does not match the manifests and the check never rewrites it: run `cargo update --workspace` to bring it in line, then commit Cargo.lock".to_owned()
    } else if stderr.contains("--offline") || stderr.contains("offline mode") {
        "a crate is not downloaded and the check runs offline: run `cargo fetch --locked`, then run the check again".to_owned()
    } else {
        format!(
            "fix what cargo reports above until `{cmd}` succeeds: this check reads the dependency graph from it"
        )
    }
}

/// The arguments of the `cargo tree` run for `package`, then `flags`.
pub(crate) fn tree_args<'a>(package: &'a str, flags: &[&'a str]) -> Vec<&'a str> {
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
        "--format",
        "{p}",
    ];
    args.extend_from_slice(flags);
    args
}

/// The dependency graph of the workspace crate `package`, from `cargo tree` run in `root` with
/// `flags` after `tree_args`. `Err` is the violation to report, at `Cargo.toml:1`.
pub(crate) fn load_tree(root: &Path, package: &str, flags: &[&str]) -> Result<Tree, Violation> {
    let args = tree_args(package, flags);
    let stdout = cargo_stdout(root, &args)?;
    Tree::parse(&stdout, package).map_err(|error| {
        violation(
            ROOT_MANIFEST,
            1,
            format!("could not read the output of `cargo {}`: {error}", args.join(" ")),
            "update `Tree::parse` in xtask/src/checks/graph.rs to the lines this cargo prints: the check reads each crate's dependency graph from them",
        )
    })
}

/// One package of a graph: its name, its version and the packages it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Package {
    name: String,
    version: String,
    deps: Vec<usize>,
}

/// A crate's dependency graph: one entry per package, the crate itself first, then the others
/// in the order `cargo tree` first prints them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tree {
    packages: Vec<Package>,
}

/// The packages one search reached from its start, nearest first, each with the package it was
/// reached from.
pub(crate) struct Reach {
    /// The packages reached, the start first, in breadth-first order.
    order: Vec<usize>,
    /// For each package of the tree, the one it was first reached from; `None` for the start and
    /// for a package not reached.
    from: Vec<Option<usize>>,
}

impl Reach {
    /// The packages reached, the start first, in breadth-first order.
    pub(crate) fn order(&self) -> &[usize] {
        &self.order
    }

    /// The packages from the start to `package`, both included, through the path the search
    /// found first: a shortest one, and among those the first in `cargo tree`'s order.
    pub(crate) fn path(&self, package: usize) -> Vec<usize> {
        let mut path = vec![package];
        let mut at = package;
        while let Some(&Some(previous)) = self.from.get(at) {
            path.push(previous);
            at = previous;
        }
        path.reverse();
        path
    }
}

impl Tree {
    /// The crate itself.
    pub(crate) const ROOT: usize = 0;

    /// The name of `package`, or `""` for an index the tree does not hold.
    pub(crate) fn name(&self, package: usize) -> &str {
        self.packages.get(package).map_or("", |p| p.name.as_str())
    }

    /// The version of `package` without its `v`, or `""` for an index the tree does not hold.
    pub(crate) fn version(&self, package: usize) -> &str {
        self.packages
            .get(package)
            .map_or("", |p| p.version.as_str())
    }

    /// The packages `package` depends on, in `cargo tree`'s order.
    pub(crate) fn deps(&self, package: usize) -> &[usize] {
        self.packages
            .get(package)
            .map(|p| p.deps.as_slice())
            .unwrap_or_default()
    }

    /// `start` and every package it reaches, breadth first, each once.
    pub(crate) fn reach(&self, start: usize) -> Reach {
        let mut from = vec![None; self.packages.len()];
        let mut seen = vec![false; self.packages.len()];
        let mut order = Vec::new();
        let mut queue = VecDeque::from([start]);
        if let Some(first) = seen.get_mut(start) {
            *first = true;
        }
        while let Some(package) = queue.pop_front() {
            order.push(package);
            for &dep in self.deps(package) {
                if let Some(seen @ false) = seen.get_mut(dep) {
                    *seen = true;
                    if let Some(slot) = from.get_mut(dep) {
                        *slot = Some(package);
                    }
                    queue.push_back(dep);
                }
            }
        }
        Reach { order, from }
    }

    /// Reads `cargo tree --prefix depth --format {p}` output for `package`: its first line is
    /// `package` at depth 0, each later line is at least one deep and at most one deeper than
    /// the line before it, no line sits under a `(*)` line, and a `(*)` line names a package
    /// printed before.
    pub(crate) fn parse(stdout: &str, package: &str) -> Result<Tree, String> {
        let mut packages: Vec<Package> = Vec::new();
        let mut index: BTreeMap<&str, usize> = BTreeMap::new();
        // The package of each depth on the path to the line read last, and whether that line
        // was a `(*)` reference, which has no lines under it.
        let mut path: Vec<usize> = Vec::new();
        let mut last_starred = false;
        for (number, text) in stdout.lines().enumerate().map(|(n, t)| (n + 1, t)) {
            let line = parse_line(text).ok_or_else(|| format!("line {number} `{text}` is not `<depth><name> v<version>` followed by `(proc-macro)`, `(<source>)` or `(*)`"))?;
            let deepest = if path.is_empty() || last_starred {
                path.len()
            } else {
                path.len() + 1
            };
            let wrong_root =
                path.is_empty() && (line.depth != 0 || line.name != package || line.starred);
            if wrong_root || (!path.is_empty() && (line.depth == 0 || line.depth >= deepest)) {
                let deepest = deepest.saturating_sub(1);
                return Err(format!(
                    "line {number} `{text}` breaks the tree: the first line is `{package}` at depth 0, and each later line is one to {deepest} deep"
                ));
            }
            let id = match index.get(line.key) {
                Some(&id) => id,
                None if line.starred => {
                    return Err(format!(
                        "line {number} `{text}` marks a package not printed before with `(*)`"
                    ));
                }
                None => {
                    index.insert(line.key, packages.len());
                    packages.push(Package {
                        name: line.name.to_owned(),
                        version: line.version.to_owned(),
                        deps: Vec::new(),
                    });
                    packages.len() - 1
                }
            };
            path.truncate(line.depth);
            if let Some(parent) = path.last().and_then(|&p| packages.get_mut(p))
                && !parent.deps.contains(&id)
            {
                parent.deps.push(id);
            }
            path.push(id);
            last_starred = line.starred;
        }
        if packages.is_empty() {
            return Err(format!("it printed no line for `{package}`"));
        }
        Ok(Tree { packages })
    }
}

/// One line of `cargo tree` output.
struct Line<'a> {
    depth: usize,
    /// The line after its depth, without ` (*)`: the same text for every line of one package.
    key: &'a str,
    name: &'a str,
    version: &'a str,
    /// True for a ` (*)` line, a package printed in full before.
    starred: bool,
}

/// One line, or `None` when it has another shape: `<depth><name> v<version>`, then
/// ` (proc-macro)` for a proc macro, then ` (<source>)` for a package not from crates.io (a
/// path may hold parentheses, so the source runs to the line's last `)`), then ` (*)` for a
/// package printed before, as cargo 1.99's `{p}` writes them.
fn parse_line(text: &str) -> Option<Line<'_>> {
    let name_start = text.find(|c: char| !c.is_ascii_digit())?;
    let depth = text.get(..name_start)?.parse().ok()?;
    let rest = text.get(name_start..)?;
    let (key, starred) = match rest.strip_suffix(" (*)") {
        Some(key) => (key, true),
        None => (rest, false),
    };
    let (name, rest) = key.split_once(' ')?;
    let is_name_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if name.is_empty() || !name.chars().all(is_name_char) {
        return None;
    }
    let (version, rest) = rest.split_once(' ').unwrap_or((rest, ""));
    let version = version.strip_prefix('v')?;
    if !version.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let rest = rest
        .strip_prefix("(proc-macro)")
        .unwrap_or(rest)
        .trim_start();
    if !rest.is_empty() {
        let source = rest.strip_prefix('(')?.strip_suffix(')')?;
        if source.is_empty() || source == "*" {
            return None;
        }
    }
    Some(Line {
        depth,
        key,
        name,
        version,
        starred,
    })
}

#[cfg(test)]
mod tests;
