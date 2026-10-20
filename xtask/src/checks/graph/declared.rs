//! Which lines of a crate's manifest declare the direct dependency that brings a package in, and
//! the text that says so, for both checks that read the graph, `dependencies` and `plugins`.
//!
//! Each declaration `cargo metadata --no-deps` lists is matched to the graph node it produced.
//! The candidates are the crate's direct dependencies with the declaration's package name whose
//! lines came under its section (`Tree::sections`). A path declaration matches the candidate
//! whose folder is its path, which cargo has already resolved against the manifest's folder.
//! Any other declaration matches the candidates that are not path packages and whose version
//! satisfies its requirement, read with the `semver` crate, as
//! cargo reads it. With one candidate, that one is the node.
//! A declaration that matches no candidate or several could have produced any of them, so the
//! text says it may bring the package in.

use super::{Section, Tree};
use crate::checks::and_list;
use crate::checks::toml_lines::Manifest;
use crate::workspace::{CargoDependency, PackageDependencies};
use std::collections::BTreeMap;

/// The lines of some declarations that are blamed together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Group {
    /// The lines, sorted, each once.
    pub lines: Vec<u32>,
    /// True when every declaration of the group was matched to one node, the same for all of
    /// them, so cargo builds them as one package. False for declarations matched to no node or
    /// to several.
    pub known: bool,
}

/// The declarations of a crate's manifest that may bring a package in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Blamed {
    /// The keys of those declarations as the manifest writes them, sorted, each once.
    pub keys: Vec<String>,
    /// Their lines, sorted, each once. A declaration whose line is not found has none.
    pub lines: Vec<u32>,
    /// True when one of them is optional.
    pub optional: bool,
    /// Their lines, one group per node they were matched to, then one group for the rest.
    pub groups: Vec<Group>,
}

impl Blamed {
    /// What is known about the blamed lines, of the manifest at `manifest`, that bring
    /// `package` in. It is meant for two lines or more.
    pub(crate) fn declared(&self, manifest: &str, package: &str) -> String {
        declared(&self.keys, manifest, package, &self.groups)
    }
}

/// The declarations among `deps`, the crate's dependencies as `cargo metadata` lists them, that
/// produced or may have produced one of `reaching`, direct dependencies of the crate in `tree`
/// that bring a package in. `text` is the crate's manifest, read for the lines.
pub(crate) fn blame(
    tree: &Tree,
    reaching: &[usize],
    deps: &PackageDependencies,
    text: &str,
) -> Blamed {
    let manifest = Manifest::parse(text);
    let mut known: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    let mut unknown = Vec::new();
    let mut keys = Vec::new();
    let mut optional = false;
    for dep in &deps.dependencies {
        let Some(section) = Section::of_kind(dep.kind.as_deref()) else {
            continue;
        };
        let (nodes, matched) = produced(tree, dep, section);
        if !nodes.iter().any(|node| reaching.contains(node)) {
            continue;
        }
        keys.push(dep.key().to_owned());
        optional |= dep.optional;
        let line = manifest.declaration_under(dep.key(), section.tables(), dep.target.as_deref());
        match (matched, nodes.as_slice()) {
            (true, [node]) => known.entry(*node).or_default().extend(line),
            _ => unknown.extend(line),
        }
    }
    keys.sort();
    keys.dedup();
    let mut groups: Vec<Group> = known
        .into_values()
        .map(|lines| group(lines, true))
        .chain([group(unknown, false)])
        .filter(|group| !group.lines.is_empty())
        .collect();
    groups.sort_by_key(|group| group.lines.first().copied());
    let mut lines: Vec<u32> = groups
        .iter()
        .flat_map(|g| g.lines.iter().copied())
        .collect();
    lines.sort_unstable();
    lines.dedup();
    Blamed {
        keys,
        lines,
        optional,
        groups,
    }
}

/// The group of `lines`, sorted and each once.
fn group(mut lines: Vec<u32>, known: bool) -> Group {
    lines.sort_unstable();
    lines.dedup();
    Group { lines, known }
}

/// The nodes `dep`, declared in `section`, may have produced among the crate's direct
/// dependencies in `tree`, and true when that is the one node it produced.
fn produced(tree: &Tree, dep: &CargoDependency, section: Section) -> (Vec<usize>, bool) {
    let candidates: Vec<usize> = tree
        .deps(Tree::ROOT)
        .iter()
        .copied()
        .filter(|&node| tree.name(node) == dep.name && tree.sections(node).contains(&section))
        .collect();
    if candidates.len() == 1 {
        return (candidates, true);
    }
    let matched: Option<Vec<usize>> = match &dep.path {
        Some(path) => Some(
            candidates
                .iter()
                .copied()
                .filter(|&node| tree.source_path(node) == Some(path.as_path()))
                .collect(),
        ),
        // A version this cannot compare leaves the declaration unmatched.
        None => candidates
            .iter()
            .copied()
            .filter(|&node| tree.source_path(node).is_none())
            .map(|node| satisfies(&dep.req, tree.version(node)).map(|ok| (node, ok)))
            .collect::<Option<Vec<(usize, bool)>>>()
            .map(|all| {
                all.into_iter()
                    .filter(|(_, ok)| *ok)
                    .map(|(n, _)| n)
                    .collect()
            }),
    };
    match matched {
        Some(one) if one.len() == 1 => (one, true),
        _ => (candidates, false),
    }
}

/// `Some(true)` when `version` satisfies the requirement `req`, `Some(false)` when it does not,
/// and `None` when the `semver` crate cannot read either.
fn satisfies(req: &str, version: &str) -> Option<bool> {
    let req = semver::VersionReq::parse(req).ok()?;
    let version = semver::Version::parse(version).ok()?;
    Some(req.matches(&version))
}

/// What is known about the manifest lines that declare the direct dependency keys `keys`, which
/// bring `package` in, given in `groups`. Cargo builds the lines of a known group as one package,
/// so a known group of several lines says only that one or more of them brings `package` in.
/// With every group known and on one line, each line brings it in. The lines of a group that is
/// not known may each bring it in.
pub(crate) fn declared(keys: &[String], manifest: &str, package: &str, groups: &[Group]) -> String {
    let mut all: Vec<u32> = groups
        .iter()
        .flat_map(|g| g.lines.iter().copied())
        .collect();
    all.sort_unstable();
    all.dedup();
    let at = |lines: &[u32]| {
        let lines: Vec<String> = lines.iter().map(u32::to_string).collect();
        and_list(&lines)
    };
    let quoted: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
    let verb = if keys.len() == 1 { "is" } else { "are" };
    let head = format!(
        "{} {verb} declared at lines {} of {manifest}",
        and_list(&quoted),
        at(&all)
    );
    let one_or_more = format!("one or more of them brings {package} in");
    let not_told = "xtask could not tell which package cargo builds";
    match groups {
        _ if groups.iter().all(|g| g.known && g.lines.len() == 1) => {
            format!("{head}, and each declaration brings {package} in")
        }
        [only] if only.known => {
            format!("{head}. Cargo builds these declarations as one package, and {one_or_more}")
        }
        [_] => format!(
            "{head}. {not_told} for each of these declarations, so each of them may bring {package} in"
        ),
        _ => {
            let sentences: Vec<String> = groups
                .iter()
                .map(|group| match (group.known, group.lines.as_slice()) {
                    (true, [line]) => format!("The declaration at line {line} brings {package} in"),
                    (true, lines) => format!(
                        "Cargo builds the declarations at lines {} as one package, and {one_or_more}",
                        at(lines)
                    ),
                    (false, [line]) => format!(
                        "{not_told} for the declaration at line {line}, so it may bring {package} in"
                    ),
                    (false, lines) => format!(
                        "{not_told} for the declarations at lines {}, so each of them may bring {package} in",
                        at(lines)
                    ),
                })
                .collect();
            format!("{head}. {}", sentences.join(". "))
        }
    }
}

#[cfg(test)]
mod tests;
