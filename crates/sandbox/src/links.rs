//! The one walk of the matter per launch: links that lead outside it, and every `.git` in it.

use std::collections::BTreeMap;
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
    /// A regular file with a name outside the matter (its link count is above the names the
    /// walk found in the matter). Unix only: stable std has no link count on Windows.
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
    /// Files with more than one name, every name of which is in the matter: each group's
    /// names, sorted. Not findings; the policy checks that no group joins a read-only name
    /// to a writable one.
    linked: Vec<Vec<PathBuf>>,
}

/// The parts of a [`MatterScan`], for the policy.
pub(crate) struct ScanParts {
    pub(crate) links: Vec<LinkFinding>,
    pub(crate) git: Vec<PathBuf>,
    pub(crate) git_dirs: Vec<PathBuf>,
    pub(crate) linked: Vec<Vec<PathBuf>>,
}

impl MatterScan {
    /// The links that lead outside the matter: symbolic links in walk order, then hard links
    /// sorted by path.
    pub fn links(&self) -> &[LinkFinding] {
        &self.links
    }

    /// Every `.git` in the matter at any depth (folder, file, or link to a folder in the
    /// matter), sorted. A `.git` inside another `.git` is not listed: the outer rule covers it.
    pub fn git(&self) -> &[PathBuf] {
        &self.git
    }

    /// The folders that `.git` files' `gitdir:` lines name inside the matter, and that `.git`
    /// links lead to inside it, canonical, sorted. One outside the matter is left out: the jail
    /// denies it anyway.
    pub fn git_dirs(&self) -> &[PathBuf] {
        &self.git_dirs
    }

    /// Adds `more` findings (from an earlier scan, say) to this one's.
    pub(crate) fn add_links(&mut self, more: Vec<LinkFinding>) {
        // Duplicates are dropped when the policy makes the findings canonical.
        self.links.extend(more);
    }

    /// The parts, for the policy.
    pub(crate) fn into_parts(self) -> ScanParts {
        ScanParts {
            links: self.links,
            git: self.git,
            git_dirs: self.git_dirs,
            linked: self.linked,
        }
    }
}

/// The links in `matter` that lead outside it: symbolic links (and on Windows folder links)
/// that resolve outside it, a broken one counting as outside, and every name in the matter of
/// a regular file that also has a name outside it (its link count is above the names the walk
/// found). The walk does not follow links. It goes into a `.git` folder for hard links only
/// (`git clone --local` hard-links objects from the repository it copies), never for symbolic
/// links or `.git` files. A matter that cannot be read gives no findings (and a warning in the
/// log); the policy uses [`scan_matter`], which fails instead.
pub fn scan_links(matter: &Path) -> Vec<LinkFinding> {
    match scan_matter(matter) {
        Ok(scan) => scan.links,
        Err(error) => {
            tracing::warn!(matter = %matter.display(), %error, "link scan failed");
            Vec::new()
        }
    }
}

/// A file with more than one name: its link count and the names the walk found.
struct Names {
    nlink: u64,
    names: Vec<PathBuf>,
}

/// The walk's state.
#[derive(Default)]
struct Walk {
    scan: MatterScan,
    /// Folders still to list, each with whether it is inside a `.git` folder.
    folders: Vec<(PathBuf, bool)>,
    /// Files with a link count above 1, by (device, inode).
    hard: BTreeMap<(u64, u64), Names>,
}

/// The one walk of `matter`: [`scan_links`]'s findings and every `.git`, with the folders the
/// `.git` files name. A sub-folder that cannot be read is skipped with a warning: the agent
/// runs as the same user, so it cannot read it either (and a file whose other name is in it
/// counts as linked outside). Fails if `matter` cannot be resolved or listed.
pub fn scan_matter(matter: &Path) -> Result<MatterScan, Error> {
    let matter = dunce::canonicalize(matter).map_err(Error::Probe)?;
    let mut walk = Walk {
        folders: vec![(matter.clone(), false)],
        ..Walk::default()
    };
    let mut first = true;
    while let Some((folder, in_git)) = walk.folders.pop() {
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
                Ok(kind) if in_git => walk.visit_in_git(path, kind, &entry),
                Ok(kind) => walk.visit(&matter, path, kind, &entry),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "matter walk skipped an entry");
                }
            }
        }
    }
    let Walk { mut scan, hard, .. } = walk;
    let mut outside = Vec::new();
    for (_, mut file) in hard {
        file.names.sort();
        if u64::try_from(file.names.len()).unwrap_or(u64::MAX) < file.nlink {
            outside.append(&mut file.names);
        } else {
            scan.linked.push(file.names);
        }
    }
    outside.sort();
    scan.links
        .extend(outside.into_iter().map(|path| LinkFinding {
            path,
            kind: LinkKind::HardLink,
            target: None,
        }));
    scan.linked.sort();
    scan.git.sort();
    scan.git_dirs.sort();
    scan.git_dirs.dedup();
    Ok(scan)
}

impl Walk {
    /// One entry of the walk outside any `.git` folder.
    fn visit(&mut self, matter: &Path, path: PathBuf, kind: FileType, entry: &fs::DirEntry) {
        let is_git = entry.file_name().eq_ignore_ascii_case(".git");
        if kind.is_symlink() {
            match symlink_finding(matter, &path, kind) {
                Some(finding) => self.scan.links.push(finding),
                // A `.git` link to a folder in the matter: that folder is the repository.
                None if is_git => {
                    if let Ok(dir) = dunce::canonicalize(&path) {
                        self.scan.git_dirs.push(dir);
                    }
                    self.scan.git.push(path);
                }
                None => {}
            }
        } else if kind.is_dir() {
            if is_git {
                self.scan.git.push(path.clone());
            }
            self.folders.push((path, is_git));
        } else if kind.is_file() {
            if is_git {
                self.scan.git.push(path.clone());
                if let Some(dir) = git_dir_named(matter, &path) {
                    self.scan.git_dirs.push(dir);
                }
            }
            self.hard_link(path, entry);
        }
    }

    /// One entry inside a `.git` folder: folders are walked and files counted for hard links;
    /// symbolic links are neither followed nor found (git makes none that lead out).
    fn visit_in_git(&mut self, path: PathBuf, kind: FileType, entry: &fs::DirEntry) {
        if kind.is_dir() {
            self.folders.push((path, true));
        } else if kind.is_file() {
            self.hard_link(path, entry);
        }
    }

    /// Counts the regular file at `path` if it has more than one name.
    fn hard_link(&mut self, path: PathBuf, entry: &fs::DirEntry) {
        if let Some((key, nlink)) = file_id(entry) {
            self.hard
                .entry(key)
                .or_insert_with(|| Names {
                    nlink,
                    names: Vec::new(),
                })
                .names
                .push(path);
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

/// The (device, inode) and link count of the regular file of `entry`, if it has more than
/// one name.
#[cfg(unix)]
fn file_id(entry: &fs::DirEntry) -> Option<((u64, u64), u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = entry.metadata().ok()?;
    (meta.nlink() > 1).then(|| ((meta.dev(), meta.ino()), meta.nlink()))
}

/// Stable std has no link count on Windows.
#[cfg(not(unix))]
fn file_id(_entry: &fs::DirEntry) -> Option<((u64, u64), u64)> {
    None
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
