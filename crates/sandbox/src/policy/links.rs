//! The link findings in the policy: canonical, the hard-link groups that join a read-only name
//! to a writable one, and the rule budget.
//!
//! The budget: Seatbelt compiles one rule per finding, and a few thousand stop the profile
//! (4,000 failed to compile in review; 20,000 overflowed the argument list). So a policy holds
//! at most [`LINK_RULES_MAX`] link rules: one per finding path, one per distinct target. Past
//! it, findings are replaced by deny rules on folders (read and write, subpath), nearest first.
//! At the deepest level that has a rule that can move, the rules are grouped by their folder;
//! the largest groups merge into a rule on their folder first (a group of k saves k - 1) until
//! the count fits; if merging every group of two or more is not enough, the single rules move up
//! a level too, where they may merge next round. A rule under a folder rule is dropped. The
//! rounds repeat until the count fits. A folder is never the matter
//! root (it holds the protected names) or a folder that holds a `.git` (a repository's
//! working tree, which would hide the whole repository); a rule whose folder is one of those
//! stays where it is. If the count still does not fit, `build` refuses.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::build::{beneath_existing, refuse};
use super::{PathRule, instruction_file};
use crate::{Error, LinkFinding, LinkKind};

/// The most link rules (finding paths, folders and targets) one profile holds.
pub const LINK_RULES_MAX: usize = 256;

/// Each finding with its path canonical up to its own name and its target canonical as far as
/// it exists; a finding whose path is not inside the matter is refused. A target that cannot be
/// resolved (a broken link with `..` past its missing part) stays as written: it cannot be
/// reached while that part is missing.
pub(super) fn canonical_findings(
    findings: Vec<LinkFinding>,
    matter: &Path,
) -> Result<Vec<LinkFinding>, Error> {
    // The walk's findings share folders; each folder is resolved once.
    let mut folders: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    let mut out: Vec<LinkFinding> = Vec::with_capacity(findings.len());
    let mut seen: BTreeSet<(PathBuf, Option<PathBuf>)> = BTreeSet::new();
    for finding in findings {
        let LinkFinding { path, kind, target } = finding;
        if !path.is_absolute() {
            return refuse(format!(
                "a link finding must be an absolute path: {}",
                path.display()
            ));
        }
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return refuse(format!(
                "a link finding has no file name: {}",
                path.display()
            ));
        };
        let folder = if let Some(canonical) = folders.get(folder) {
            canonical.clone()
        } else {
            let canonical = dunce::canonicalize(folder).map_err(|error| {
                Error::Policy(format!(
                    "a link finding's folder must exist: {}: {error}",
                    path.display()
                ))
            })?;
            folders.insert(folder.to_owned(), canonical.clone());
            canonical
        };
        let canonical = folder.join(name);
        if !canonical.starts_with(matter) || canonical == matter {
            return refuse(format!(
                "a link finding must be inside the matter {}: {}",
                matter.display(),
                canonical.display()
            ));
        }
        let target = target.map(|t| beneath_existing(&t, "link target").unwrap_or(t));
        let finding = LinkFinding {
            path: canonical,
            kind,
            target,
        };
        if seen.insert((finding.path.clone(), finding.target.clone())) {
            out.push(finding);
        }
    }
    Ok(out)
}

/// Hard-link findings for the groups of [`MatterScan`](crate::MatterScan) whose every name is
/// in the matter but that join a write-denied name (a `.git`, a protected name, an over-cap
/// file, a nested `CLAUDE.md`) to a writable one: the writable names, which would carry a write
/// through to the read-only one. A group whose names are all writable, or all read-only, gives
/// nothing.
pub(super) fn split_linked(
    linked: Vec<Vec<PathBuf>>,
    matter: &Path,
    read_only: &[PathRule],
) -> Vec<LinkFinding> {
    let denied =
        |p: &Path| read_only.iter().any(|rule| rule.covers(p)) || instruction_file(matter, p);
    let mut out = Vec::new();
    for names in linked {
        let (locked, open): (Vec<PathBuf>, Vec<PathBuf>) =
            names.into_iter().partition(|p| denied(p));
        if locked.is_empty() {
            continue;
        }
        out.extend(open.into_iter().map(|path| LinkFinding {
            path,
            kind: LinkKind::HardLink,
            target: None,
        }));
    }
    out
}

/// The folders that replace findings so the policy fits [`LINK_RULES_MAX`] (none when it fits
/// already), by the algorithm in this module's notes; refuses when it cannot fit. `keep` are
/// folders never used (the matter root is never used either).
pub(super) fn fit_budget(
    findings: &[LinkFinding],
    matter: &Path,
    keep: &[PathBuf],
) -> Result<Vec<PathBuf>, Error> {
    let targets: BTreeSet<&PathBuf> = findings.iter().filter_map(|f| f.target.as_ref()).collect();
    let mut paths: BTreeSet<PathBuf> = findings.iter().map(|f| f.path.clone()).collect();
    let mut folders: BTreeSet<PathBuf> = BTreeSet::new();
    let usable = |folder: &Path| {
        folder != matter && folder.starts_with(matter) && !keep.iter().any(|k| k == folder)
    };
    loop {
        let mut count = paths.len() + folders.len() + targets.len();
        if count <= LINK_RULES_MAX {
            return Ok(folders.into_iter().collect());
        }
        // The deepest rules that can move up a level, by the folder they would move to.
        let movable = |p: &PathBuf| p.parent().is_some_and(usable);
        let depth = paths
            .iter()
            .chain(&folders)
            .filter(|p| movable(p))
            .map(|p| p.components().count())
            .max();
        let Some(depth) = depth else {
            return refuse(format!(
                "the matter has {} links that lead outside it ({count} link rules after \
                 grouping them by folder); a jail holds at most {LINK_RULES_MAX}",
                findings.len()
            ));
        };
        let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        for p in paths.iter().chain(&folders) {
            if p.components().count() == depth
                && let Some(parent) = p.parent().filter(|parent| usable(parent))
            {
                groups.entry(parent.to_owned()).or_default().push(p.clone());
            }
        }
        // Largest groups first: each merge of k rules saves k - 1. Single rules move up only
        // when merging every group at this level is not enough; they may merge one level up.
        let mut groups: Vec<(PathBuf, Vec<PathBuf>)> = groups.into_iter().collect();
        groups.sort_by_key(|(_, members)| std::cmp::Reverse(members.len()));
        let mut moves: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
        for (parent, members) in groups {
            if count > LINK_RULES_MAX {
                count -= members.len() - 1;
                moves.push((parent, members));
            }
        }
        for (parent, members) in moves {
            for member in &members {
                paths.remove(member);
                folders.remove(member);
            }
            folders.insert(parent);
        }
        // A rule under a folder rule is covered by it.
        let covered = |p: &PathBuf, folders: &BTreeSet<PathBuf>| {
            p.ancestors().skip(1).any(|a| folders.contains(a))
        };
        paths.retain(|p| !covered(p, &folders));
        let snapshot = folders.clone();
        folders.retain(|p| !covered(p, &snapshot));
    }
}
