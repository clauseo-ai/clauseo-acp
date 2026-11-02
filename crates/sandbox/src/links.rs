//! The one walk of the matter per launch: links that lead outside it, and every `.git` in it.

use std::fs::{self, FileType};
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::Error;

/// A `.git` file larger than this is not read for its `gitdir:` line; git writes one line.
const GIT_FILE_MAX: u64 = 64 * 1024;

/// What kind of link a [`LinkFinding`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// A symbolic link (on Windows, a file symbolic link).
    Symlink,
    /// On Windows, a folder link: a junction or a folder symbolic link, which std does not tell
    /// apart. Never found elsewhere.
    Junction,
    /// A regular file with more than one name (link count above 1). Unix only: stable std has
    /// no link count on Windows.
    HardLink,
}

/// A link in the matter that can lead the agent outside it. The jail denies read and write on
/// both [`LinkFinding::path`] and [`LinkFinding::target`]; a denial on either is
/// [`PathClass::LinkTarget`](crate::PathClass::LinkTarget).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkFinding {
    /// The link in the matter, canonical up to its own name (the link itself is not followed).
    pub path: PathBuf,
    /// What kind of link.
    pub kind: LinkKind,
    /// Where it leads, canonical; for a broken link, its target as written, joined to the
    /// link's folder. `None` for a hard link, whose other names cannot be found from here.
    pub target: Option<PathBuf>,
}

/// What one walk of the matter found: the links of [`scan_links`] and every `.git`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatterScan {
    links: Vec<LinkFinding>,
    git: Vec<PathBuf>,
    git_dirs: Vec<PathBuf>,
}

impl MatterScan {
    /// The links that lead outside the matter, in walk order.
    pub fn links(&self) -> &[LinkFinding] {
        &self.links
    }

    /// Every `.git` in the matter at any depth, folder or file, sorted.
    pub fn git(&self) -> &[PathBuf] {
        &self.git
    }

    /// The folders that `.git` files' `gitdir:` lines name inside the matter, canonical, sorted.
    /// One outside the matter is left out: the jail denies it anyway.
    pub fn git_dirs(&self) -> &[PathBuf] {
        &self.git_dirs
    }

    /// Adds `more` findings (from an earlier scan, say) to this one's.
    pub(crate) fn add_links(&mut self, more: Vec<LinkFinding>) {
        for finding in more {
            if !self.links.contains(&finding) {
                self.links.push(finding);
            }
        }
    }

    /// The parts, for the policy.
    pub(crate) fn into_parts(self) -> (Vec<LinkFinding>, Vec<PathBuf>, Vec<PathBuf>) {
        (self.links, self.git, self.git_dirs)
    }
}

/// The links in `matter` that lead outside it: symbolic links (and on Windows folder links)
/// that resolve outside it, a broken one counting as outside, and regular files with a link
/// count above 1. The walk does not follow links and does not go into a `.git` folder. A
/// matter that cannot be read gives no findings (and a warning in the log); the policy uses
/// [`scan_matter`], which fails instead.
pub fn scan_links(matter: &Path) -> Vec<LinkFinding> {
    match scan_matter(matter) {
        Ok(scan) => scan.links,
        Err(error) => {
            tracing::warn!(matter = %matter.display(), %error, "link scan failed");
            Vec::new()
        }
    }
}

/// The one walk of `matter`: [`scan_links`]'s findings and every `.git`, with the folders the
/// `.git` files name. A sub-folder that cannot be read is skipped with a warning: the agent
/// runs as the same user, so it cannot read it either. Fails if `matter` cannot be resolved
/// or listed.
pub fn scan_matter(matter: &Path) -> Result<MatterScan, Error> {
    let matter = dunce::canonicalize(matter).map_err(Error::Probe)?;
    let mut scan = MatterScan::default();
    let mut folders = vec![matter.clone()];
    let mut first = true;
    while let Some(folder) = folders.pop() {
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if first => return Err(Error::Probe(error)),
            Err(error) => {
                tracing::warn!(folder = %folder.display(), %error, "matter walk skipped a folder");
                continue;
            }
        };
        first = false;
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(folder = %folder.display(), %error, "matter walk skipped an entry");
                    continue;
                }
            };
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) => visit(&matter, &path, kind, &entry, &mut scan, &mut folders),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "matter walk skipped an entry");
                }
            }
        }
    }
    scan.git.sort();
    scan.git_dirs.sort();
    scan.git_dirs.dedup();
    Ok(scan)
}

/// One entry of the walk.
fn visit(
    matter: &Path,
    path: &Path,
    kind: FileType,
    entry: &fs::DirEntry,
    scan: &mut MatterScan,
    folders: &mut Vec<PathBuf>,
) {
    let is_git = entry.file_name().eq_ignore_ascii_case(".git");
    if kind.is_symlink() {
        if let Some(finding) = symlink_finding(matter, path, kind) {
            scan.links.push(finding);
        }
    } else if kind.is_dir() {
        if is_git {
            scan.git.push(path.to_owned());
        } else {
            folders.push(path.to_owned());
        }
    } else if kind.is_file() {
        if is_git {
            scan.git.push(path.to_owned());
            if let Some(dir) = git_dir_named(matter, path) {
                scan.git_dirs.push(dir);
            }
        }
        if has_other_names(entry) {
            scan.links.push(LinkFinding {
                path: path.to_owned(),
                kind: LinkKind::HardLink,
                target: None,
            });
        }
    }
}

/// The finding for the symbolic link at `path`, if it leads outside `matter` or nowhere.
fn symlink_finding(matter: &Path, path: &Path, kind: FileType) -> Option<LinkFinding> {
    let target = match dunce::canonicalize(path) {
        Ok(target) if target.starts_with(matter) => return None,
        Ok(target) => target,
        Err(_) => {
            // Broken (or a loop): outside by rule. Keep where it says it leads.
            let written = fs::read_link(path).unwrap_or_default();
            path.parent()
                .map_or(written.clone(), |folder| folder.join(&written))
        }
    };
    Some(LinkFinding {
        path: path.to_owned(),
        kind: link_kind(kind),
        target: Some(target),
    })
}

#[cfg(windows)]
fn link_kind(kind: FileType) -> LinkKind {
    use std::os::windows::fs::FileTypeExt;
    if kind.is_symlink_dir() {
        LinkKind::Junction
    } else {
        LinkKind::Symlink
    }
}

#[cfg(not(windows))]
fn link_kind(_kind: FileType) -> LinkKind {
    LinkKind::Symlink
}

/// Whether the regular file of `entry` has a link count above 1.
#[cfg(unix)]
fn has_other_names(entry: &fs::DirEntry) -> bool {
    use std::os::unix::fs::MetadataExt;
    entry.metadata().is_ok_and(|meta| meta.nlink() > 1)
}

#[cfg(not(unix))]
fn has_other_names(_entry: &fs::DirEntry) -> bool {
    false
}

/// The folder a `.git` file's `gitdir:` line names, canonical, if it exists inside `matter`.
fn git_dir_named(matter: &Path, git_file: &Path) -> Option<PathBuf> {
    let text = read_small(git_file).ok()?;
    let line = text.lines().find_map(|line| line.strip_prefix("gitdir:"))?;
    let named = Path::new(line.trim());
    let joined = if named.is_absolute() {
        named.to_owned()
    } else {
        git_file.parent()?.join(named)
    };
    let dir = dunce::canonicalize(joined).ok()?;
    // The matter itself is left out: naming it would make the whole matter read-only.
    (dir.starts_with(matter) && dir != matter).then_some(dir)
}

/// The text of `path` if it is at most [`GIT_FILE_MAX`] bytes.
fn read_small(path: &Path) -> io::Result<String> {
    if fs::metadata(path)?.len() > GIT_FILE_MAX {
        return Err(io::Error::other("too large for a .git file"));
    }
    fs::read_to_string(path)
}

#[cfg(test)]
mod tests;
