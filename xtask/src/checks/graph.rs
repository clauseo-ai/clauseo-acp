//! A workspace crate's dependency graph, for the checks that read one: `dependencies` walks the
//! graph of each crate whose row bans a crate, `plugins` walks the desktop crate's. A failed
//! cargo run becomes one failure with no `file:line`, with a fix for the cause cargo names.
//!
//! The graph comes from the crate's own `cargo tree` run (see `tree_args`): `-p <crate>
//! --all-features`, so the crate's optional dependencies count but a feature that only another
//! workspace member turns on does not (one workspace-wide resolve would merge them, and blame the
//! crate for an edge only the other member's build has); `--target all`, so every target's
//! dependencies count; `-e normal,build`, since dev-dependencies never reach the built crate.
//! With `--prefix indent --charset ascii --format "{p} [{f}]"` the output is a tree drawing. The
//! first line is the crate, with no prefix. Each later line has one four-column chunk for each
//! level between it and the crate: `|   ` where the parent has a later sibling, four spaces where
//! it has none. Then `|-- ` or `` `-- `` comes before the node, whose text is `<name> v<version>`,
//! then `(proc-macro)` for a proc macro, then the source in parentheses for a path or git package,
//! then the features cargo turned on in brackets, then ` (*)` for a node printed before. Cargo
//! prints a node's subtree in full the first time and marks each later line of it `(*)`, so the
//! graph maps each line's text, without the `(*)`, to the nodes under it, and a `(*)` line is a
//! reference to that entry. The output stays the size of the graph, not of the number of paths
//! through it. A package built twice with different features (for the host by a build dependency,
//! and for the target) is two cargo nodes with different children; the features keep their lines
//! apart, so each is its own entry and no path runs through the other's children. Two lines of one
//! text are two builds with the same features, so the same children.
//!
//! Under a package, cargo prints its normal edges first, then a `[build-dependencies]` header
//! line at the level of its build edges, then those edges. The header covers the edges at its
//! level below it, up to the next line at that level or shallower, so a direct dependency knows
//! which section its lines came under (`Tree::sections`). A line in any other shape fails the
//! check.

mod fix;
pub(crate) use fix::Fix;

use crate::checks::Failure;
use crate::checks::toml_lines::{BUILD, Kind, NORMAL};
use cargo::cargo_stdout;
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

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
        "indent",
        "--charset",
        "ascii",
        "--format",
        "{p} [{f}]",
    ];
    args.extend_from_slice(flags);
    args
}

/// The dependency graph of the workspace crate `package`, from `cargo tree` run in `root` with
/// `flags` after `tree_args`. `Err` is the failure to report, with no `file:line`.
pub(crate) fn load_tree(root: &Path, package: &str, flags: &[&str]) -> Result<Tree, Failure> {
    let args = tree_args(package, flags);
    let stdout = cargo_stdout(root, &args)?;
    Tree::parse(&stdout, package).map_err(|error| Failure {
        message: format!(
            "could not read the output of `cargo {}`: {error}",
            args.join(" ")
        ),
        fix: Fix::TreeParse.into(),
    })
}

/// The section of a crate's manifest that a dependency is declared in, as far as the graph
/// tells: a direct dependency's edge came under one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Section {
    /// `[dependencies]`, under no header.
    Normal,
    /// `[build-dependencies]`, under its header.
    Build,
}

impl Section {
    /// The section of a `cargo metadata` dependency of `kind` (`None` for normal, `"build"` for
    /// build), or `None` for a kind that is not in the graph, such as dev.
    pub(crate) fn of_kind(kind: Option<&str>) -> Option<Section> {
        match kind {
            None | Some("normal") => Some(Section::Normal),
            Some("build") => Some(Section::Build),
            Some(_) => None,
        }
    }

    /// The manifest tables of this section, with or without a target.
    pub(crate) fn tables(self) -> &'static [Kind] {
        match self {
            Section::Normal => NORMAL,
            Section::Build => BUILD,
        }
    }
}

/// One node of a graph: a package with one set of features, and the nodes it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Package {
    name: String,
    version: String,
    /// The source cargo prints in parentheses for a path or git package, else `None`.
    source: Option<String>,
    deps: Vec<usize>,
    /// For a direct dependency of the crate, the sections its lines came under, each once.
    sections: Vec<Section>,
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

    /// The folder of `package`, which is the source cargo prints for it when that is an absolute
    /// path, as it is for a path package. `None` for a registry or git package.
    pub(crate) fn source_path(&self, package: usize) -> Option<&Path> {
        let source = self.packages.get(package)?.source.as_deref()?;
        Some(Path::new(source)).filter(|path| path.is_absolute())
    }

    /// The repository and reference of the git package `package`, such as
    /// `https://example.com/s?branch=main`, which is the source cargo prints for it without the
    /// `#` and commit at its end. `None` for a registry or path package.
    pub(crate) fn git_source(&self, package: usize) -> Option<&str> {
        let source = self.packages.get(package)?.source.as_deref()?;
        if Path::new(source).is_absolute() {
            return None;
        }
        source.rsplit_once('#').map(|(repository, _)| repository)
    }

    /// The sections the lines of the direct dependency `package` came under: `Build` as well as
    /// `Normal` when cargo printed it under both. Empty for a package the crate does not depend on
    /// directly.
    pub(crate) fn sections(&self, package: usize) -> &[Section] {
        self.packages
            .get(package)
            .map(|p| p.sections.as_slice())
            .unwrap_or_default()
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

    /// Reads `cargo tree --prefix indent --charset ascii --format "{p} [{f}]"` output for
    /// `package`: its first line is `package` at depth 0, each later node line is at least one
    /// deep and at most one deeper than the line before it, no line sits under a `(*)` line, and a
    /// `(*)` line names a node printed before. A `[build-dependencies]` header needs a package
    /// with edges below it at its level.
    pub(crate) fn parse(stdout: &str, package: &str) -> Result<Tree, String> {
        let mut packages: Vec<Package> = Vec::new();
        let mut index: BTreeMap<&str, usize> = BTreeMap::new();
        // The package of each depth on the path to the line read last, and whether that line
        // was a `(*)` reference, which has no lines under it.
        let mut path: Vec<usize> = Vec::new();
        let mut last_starred = false;
        // The section each level's edges came under: `headers[level]` is set by a header above
        // them, and dropped by the first line at a shallower depth.
        let mut headers: Vec<Section> = Vec::new();
        for (number, text) in stdout.lines().enumerate().map(|(n, t)| (n + 1, t)) {
            let entry = parse_line(text).ok_or_else(|| format!("line {number} `{text}` is not a node line (`<prefix><name> v<version>`, then `(proc-macro)`, `(<source>)`, `[<features>]` and `(*)`) or a `[build-dependencies]` header"))?;
            let line = match entry {
                Entry::Header { level, section } => {
                    if path.len() <= level || (path.len() == level + 1 && last_starred) {
                        return Err(format!(
                            "line {number} `{text}` is a section header with no package above it that can have edges"
                        ));
                    }
                    headers.resize(level + 1, Section::Normal);
                    headers[level] = section;
                    continue;
                }
                Entry::Node(line) => line,
            };
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
                        source: line.source.map(str::to_owned),
                        deps: Vec::new(),
                        sections: Vec::new(),
                    });
                    packages.len() - 1
                }
            };
            // A line at this depth ends the headers of the levels below it.
            headers.truncate(line.depth);
            let section = line
                .depth
                .checked_sub(1)
                .and_then(|level| headers.get(level))
                .copied()
                .unwrap_or(Section::Normal);
            path.truncate(line.depth);
            if let Some(parent) = path.last().and_then(|&p| packages.get_mut(p))
                && !parent.deps.contains(&id)
            {
                parent.deps.push(id);
            }
            if line.depth == 1 && !packages[id].sections.contains(&section) {
                packages[id].sections.push(section);
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

/// One line of `cargo tree` output: a node line, or a section header.
enum Entry<'a> {
    Node(Line<'a>),
    /// A `[build-dependencies]` header under the package at depth `level`, labelling the edges
    /// `level` chunks deep below it.
    Header {
        level: usize,
        section: Section,
    },
}

/// One node line of `cargo tree` output.
struct Line<'a> {
    /// The depth in the tree: 0 for the crate, 1 for a direct dependency.
    depth: usize,
    /// The line after its prefix, without ` (*)`: the same text for every line of one node.
    key: &'a str,
    name: &'a str,
    version: &'a str,
    /// The source in parentheses, for a path or git package.
    source: Option<&'a str>,
    /// True for a ` (*)` line, a node printed in full before.
    starred: bool,
}

/// One line, or `None` when it has another shape. A node line is its prefix (a `|   ` or four
/// spaces for each level between it and the crate, then `|-- ` or `` `-- ``, and none for the
/// crate itself) and then `node_line`'s text. A header is its prefix and `[build-dependencies]`.
fn parse_line(text: &str) -> Option<Entry<'_>> {
    let mut rest = text;
    let mut level = 0;
    while let Some(next) = rest
        .strip_prefix("|   ")
        .or_else(|| rest.strip_prefix("    "))
    {
        rest = next;
        level += 1;
    }
    if let Some(node) = rest
        .strip_prefix("|-- ")
        .or_else(|| rest.strip_prefix("`-- "))
    {
        return node_line(node, level + 1).map(Entry::Node);
    }
    if rest.starts_with('[') {
        let section = match rest {
            "[build-dependencies]" => Section::Build,
            _ => return None,
        };
        return Some(Entry::Header { level, section });
    }
    if level == 0 {
        return node_line(rest, 0).map(Entry::Node);
    }
    None
}

/// The node line `text` at `depth`, or `None` when it has another shape: `<name> v<version>`,
/// then ` (proc-macro)` for a proc macro, then ` (<source>)` for a package not from crates.io (a
/// path may hold parentheses, so the source runs to the line's last `)`), then ` [<features>]`
/// (comma-separated, maybe empty), then ` (*)` for a node printed before, as cargo 1.99's
/// `{p} [{f}]` writes them. The key is the text without ` (*)`, features included.
fn node_line(text: &str, depth: usize) -> Option<Line<'_>> {
    let (key, starred) = match text.strip_suffix(" (*)") {
        Some(key) => (key, true),
        None => (text, false),
    };
    let (package, features) = key.strip_suffix(']')?.rsplit_once(" [")?;
    let is_feature_char = |c: char| c.is_ascii_alphanumeric() || "-_+./,".contains(c);
    if !features.chars().all(is_feature_char) {
        return None;
    }
    let (name, rest) = package.split_once(' ')?;
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
    let source = if rest.is_empty() {
        None
    } else {
        let source = rest.strip_prefix('(')?.strip_suffix(')')?;
        if source.is_empty() || source == "*" {
            return None;
        }
        Some(source)
    };
    Some(Line {
        depth,
        key,
        name,
        version,
        source,
        starred,
    })
}

mod cargo;
mod declared;

pub(crate) use declared::blame;

#[cfg(test)]
mod tests;
