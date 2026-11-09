//! The one walk of the matter per launch: links that lead outside it, and every `.git`, other
//! [`PROTECTED_AT_ANY_DEPTH`] name and repository-shaped folder in it.

use std::collections::BTreeMap;
use std::fs::{self, FileType};
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::policy::{opens_as, same, under};
use crate::{Error, PROTECTED_AT_ANY_DEPTH};

pub(crate) mod network;

use network::{Mounts, Off, classify, link_to_network, read_if_link};

/// The most git dirs (folders holding a `HEAD`) one walk reads a `commondir` in; past it the
/// launch is refused, so a matter can't make the launch read without end.
pub(crate) const GIT_DIRS_MAX: usize = 10_000;

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
/// [`LinkFinding::path`] (or a link folder holding it), and on [`LinkFinding::target`] when an
/// allow rule would otherwise reach it ([`Policy::link_targets`](crate::Policy::link_targets));
/// a denial on either is [`PathClass::LinkTarget`](crate::PathClass::LinkTarget).
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

/// What one walk of the matter found: the links of [`scan_links`], every `.git`, and every other
/// [`PROTECTED_AT_ANY_DEPTH`] name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatterScan {
    links: Vec<LinkFinding>,
    git: Vec<PathBuf>,
    git_dirs: Vec<PathBuf>,
    /// The folders those name outside the matter, canonical, sorted: the policy refuses one the
    /// agent can write, where it could plant the repository's `config`.
    git_dirs_outside: Vec<PathBuf>,
    /// The `commondir` names beside a `HEAD` that are links, sorted: git follows one, the walk
    /// does not, so the policy refuses the launch.
    commondir_links: Vec<PathBuf>,
    /// The paths those name on a device the launch doesn't know (neither a local disk nor a
    /// network one), sorted: never touched, and the policy refuses the launch.
    git_dirs_unknown: Vec<PathBuf>,
    bare: Vec<PathBuf>,
    nested: Vec<PathBuf>,
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
    pub(crate) git_dirs_outside: Vec<PathBuf>,
    pub(crate) commondir_links: Vec<PathBuf>,
    pub(crate) git_dirs_unknown: Vec<PathBuf>,
    pub(crate) bare: Vec<PathBuf>,
    pub(crate) nested: Vec<PathBuf>,
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

    /// The folders that `.git` files' `gitdir:` lines and `commondir` files beside a `HEAD`
    /// (inside a `.git` or not) name inside the matter, and that `.git` links lead to inside it,
    /// canonical, sorted. One outside the matter is left out here (the policy refuses one the
    /// agent can write; the jail denies the rest anyway), and one on a network path is skipped
    /// unread ([`network`]).
    pub fn git_dirs(&self) -> &[PathBuf] {
        &self.git_dirs
    }

    /// The folders outside any `.git` that hold a `HEAD` file or link, sorted: git's discovery
    /// takes one with `objects` and `refs` folders, or a `commondir` file, for a git dir (no
    /// `.git` name involved), and the agent could add those.
    pub fn bare_repos(&self) -> &[PathBuf] {
        &self.bare
    }

    /// Every `.claude`, `.mcp.json`, `CLAUDE.md` and `AGENTS.md` in the matter at any depth, root
    /// included, in any letter case (the other [`PROTECTED_AT_ANY_DEPTH`] names), sorted: a
    /// folder, a file, or a link to something in the matter. Found by name in the listings the
    /// walk reads anyway, so no probe is added. None is listed inside a `.git` folder, whose
    /// names the walk doesn't read; one inside another is, and the policy drops it.
    pub fn nested(&self) -> &[PathBuf] {
        &self.nested
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
            git_dirs_outside: self.git_dirs_outside,
            commondir_links: self.commondir_links,
            git_dirs_unknown: self.git_dirs_unknown,
            bare: self.bare,
            nested: self.nested,
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
            tracing::warn!(%error, "link scan failed");
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
    /// The mount table, the matter's own mount counted local ([`Mounts::trusting`]).
    mounts: Mounts,
    /// The network mount points inside the matter: folders the walk never enters.
    network_inside: Vec<PathBuf>,
    /// How many more git dirs' `commondir` the walk may read.
    git_dirs_left: usize,
    /// Whether a git dir past [`GIT_DIRS_MAX`] was met.
    over_cap: bool,
}

/// The one walk of `matter`: [`scan_links`]'s findings and every `.git`, with the folders the
/// `.git` files name, and every other [`PROTECTED_AT_ANY_DEPTH`] name. A sub-folder that
/// cannot be read is skipped with a warning: the agent runs as the same user, so it cannot read
/// it either (and a file whose other name is in it counts as linked outside). Fails if `matter`
/// cannot be resolved or listed, or the mount table can't be read, and refuses past
/// [`GIT_DIRS_MAX`] git dirs. A network mount inside the matter is never entered.
pub fn scan_matter(matter: &Path) -> Result<MatterScan, Error> {
    let mounts = Mounts::current();
    if mounts.unread {
        return Err(Error::Probe(io::Error::other(
            "the mount list could not be read",
        )));
    }
    scan_matter_with(matter, mounts, GIT_DIRS_MAX)
}

/// [`scan_matter`] with this mount table and cap.
fn scan_matter_with(matter: &Path, mounts: Mounts, cap: usize) -> Result<MatterScan, Error> {
    let matter = dunce::canonicalize(matter).map_err(Error::Probe)?;
    let mounts = mounts.trusting(&matter);
    let mut walk = Walk {
        folders: vec![(matter.clone(), false)],
        network_inside: mounts.network_inside(&matter),
        mounts,
        git_dirs_left: cap,
        scan: MatterScan::default(),
        hard: BTreeMap::new(),
        over_cap: false,
    };
    let mut first = true;
    while let Some((folder, in_git)) = walk.folders.pop() {
        if walk.over_cap {
            return Err(Error::Policy(format!(
                "the matter holds more than {cap} git folders: {}",
                matter.display()
            )));
        }
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if first => return Err(Error::Probe(error)),
            Err(error) => {
                tracing::warn!(%error, "matter walk skipped a folder");
                continue;
            }
        };
        first = false;
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(%error, "matter walk skipped an entry");
                    continue;
                }
            };
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if in_git => walk.visit_in_git(&matter, path, kind, &entry),
                Ok(kind) => walk.visit(&matter, path, kind, &entry),
                Err(error) => {
                    tracing::warn!(%error, "matter walk skipped an entry");
                }
            }
        }
    }
    if walk.over_cap {
        return Err(Error::Policy(format!(
            "the matter holds more than {cap} git folders: {}",
            matter.display()
        )));
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
    scan.git_dirs_outside.sort();
    scan.git_dirs_outside.dedup();
    scan.commondir_links.sort();
    scan.git_dirs_unknown.sort();
    scan.bare.sort();
    scan.bare.dedup();
    scan.nested.sort();
    Ok(scan)
}

impl Walk {
    /// One entry of the walk outside any `.git` folder.
    fn visit(&mut self, matter: &Path, path: PathBuf, kind: FileType, entry: &fs::DirEntry) {
        let name = entry.file_name();
        let is_git = name.eq_ignore_ascii_case(".git");
        // The other names write-denied at any depth: one name check per entry listed.
        let nested = !is_git
            && PROTECTED_AT_ANY_DEPTH
                .iter()
                .any(|protected| name.eq_ignore_ascii_case(protected));
        if nested && !kind.is_symlink() {
            self.scan.nested.push(path.clone());
        }
        if (kind.is_file() || kind.is_symlink()) && opens_as(&name, "head") {
            self.bare_repo(matter, &path);
        }
        if kind.is_symlink() {
            let found = symlink_finding(matter, &path, link_kind(kind), &self.mounts);
            match found {
                Some(finding) => self.scan.links.push(finding),
                // A `.git` link to a folder in the matter: that folder is the repository.
                None if is_git => {
                    if let Ok(dir) = dunce::canonicalize(&path) {
                        self.scan.git_dirs.push(dir);
                    }
                    self.scan.git.push(path);
                }
                // One to something in the matter, as a `.git` link is: the deny is on its name.
                None if nested => self.scan.nested.push(path),
                None => {}
            }
        } else if kind.is_dir() {
            if is_git {
                self.scan.git.push(path.clone());
            }
            self.enter(path, is_git);
        } else if kind.is_file() {
            if is_git {
                self.scan.git.push(path.clone());
                let named = git_dir_named(matter, &path, &self.mounts);
                self.note(named);
            }
            self.hard_link(path, entry);
        }
    }

    /// Lists the folder holding the `HEAD` file or link at `head`: with one, the agent could
    /// complete it into a git dir (`objects/` and `refs/`, or `commondir`). Also reads the
    /// `commondir` beside it ([`Walk::commondir`]).
    fn bare_repo(&mut self, matter: &Path, head: &Path) {
        let Some(folder) = head.parent() else {
            return;
        };
        self.scan.bare.push(folder.to_owned());
        self.commondir(matter, folder);
    }

    /// Notes the folder the `commondir` file in `folder`, which holds a `HEAD`, names, by the
    /// `gitdir:` rule: git reads `commondir` in any git dir (`.git`, `.git/worktrees/<name>`,
    /// `.git/modules/<name>` and its worktrees, a bare repository) and takes the repository's
    /// `config` from the folder it names. A `commondir` that is a link is noted for the policy
    /// to refuse, unread. Names compare by [`opens_as`]: one listing of the folder per `HEAD`.
    fn commondir(&mut self, matter: &Path, folder: &Path) {
        let Some(left) = self.git_dirs_left.checked_sub(1) else {
            self.over_cap = true;
            return;
        };
        self.git_dirs_left = left;
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            if !opens_as(&entry.file_name(), "commondir") {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_symlink() => self.scan.commondir_links.push(entry.path()),
                Ok(kind) if kind.is_file() => {
                    let Some(line) = read_small(&entry.path())
                        .ok()
                        .and_then(|text| text.lines().next().map(str::to_owned))
                    else {
                        continue;
                    };
                    let named = dir_named(matter, folder, &line, &self.mounts);
                    self.note(named);
                }
                _ => {}
            }
        }
    }

    /// One entry inside a `.git` folder: folders are walked and files counted for hard links;
    /// symbolic links are neither followed nor found (git makes none that lead out). A `HEAD`
    /// file or link makes its folder a git dir, whose `commondir` is read.
    fn visit_in_git(&mut self, matter: &Path, path: PathBuf, kind: FileType, entry: &fs::DirEntry) {
        if (kind.is_file() || kind.is_symlink())
            && opens_as(&entry.file_name(), "head")
            && let Some(folder) = path.parent()
        {
            self.commondir(matter, folder);
        }
        if kind.is_dir() {
            self.enter(path, true);
        } else if kind.is_file() {
            self.hard_link(path, entry);
        }
    }

    /// Lists the folder `path` later, unless it is a network mount point.
    fn enter(&mut self, path: PathBuf, in_git: bool) {
        if self.network_inside.iter().any(|point| same(point, &path)) {
            tracing::warn!("the matter walk did not enter a network mount in the matter");
            return;
        }
        self.folders.push((path, in_git));
    }

    /// Adds where a `gitdir:` or `commondir` line leads to the scan.
    fn note(&mut self, named: Option<Named>) {
        match named {
            Some(Named::Inside(dir)) => self.scan.git_dirs.push(dir),
            Some(Named::Outside(dir)) => self.scan.git_dirs_outside.push(dir),
            Some(Named::Unknown(path)) => self.scan.git_dirs_unknown.push(path),
            None => {}
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

/// The finding for the symbolic link at `path`, if it leads outside `matter` or nowhere, by
/// [`symlink_finding_with`] with the file system's calls.
fn symlink_finding(
    matter: &Path,
    path: &Path,
    kind: LinkKind,
    mounts: &Mounts,
) -> Option<LinkFinding> {
    symlink_finding_with(matter, path, kind, mounts, read_if_link, |link| {
        dunce::canonicalize(link)
    })
}

/// [`symlink_finding`] with `read` and `resolve` in place of reading a link and canonicalize.
/// A link whose chain reaches a network path ([`link_to_network`]) leads outside, to that path,
/// and is neither resolved nor followed further.
fn symlink_finding_with(
    matter: &Path,
    path: &Path,
    kind: LinkKind,
    mounts: &Mounts,
    read: impl Fn(&Path) -> io::Result<Option<PathBuf>>,
    resolve: impl FnOnce(&Path) -> io::Result<PathBuf>,
) -> Option<LinkFinding> {
    let finding = |target| {
        Some(LinkFinding {
            path: path.to_owned(),
            kind,
            target: Some(target),
        })
    };
    if let Some((network, _)) = link_to_network(path, mounts, &read) {
        tracing::warn!("the matter walk did not follow a link to a network path");
        return finding(network);
    }
    match resolve(path) {
        Ok(target) if under(&target, matter) => None,
        Ok(target) => finding(target),
        Err(_) => {
            // Broken (or a loop): outside by rule. Keep where it says it leads.
            let written = read(path).ok().flatten().unwrap_or_default();
            finding(
                path.parent()
                    .map_or(written.clone(), |folder| folder.join(&written)),
            )
        }
    }
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

/// Where a `gitdir:` line or a `commondir` file's line leads, canonical.
#[derive(Debug, PartialEq, Eq)]
enum Named {
    /// A folder inside the matter, not the matter itself.
    Inside(PathBuf),
    /// A folder outside the matter.
    Outside(PathBuf),
    /// A path on a device the launch doesn't know, as far as it was read: never touched.
    Unknown(PathBuf),
}

/// Where a `.git` file's `gitdir:` line leads, by [`dir_named`].
fn git_dir_named(matter: &Path, git_file: &Path, mounts: &Mounts) -> Option<Named> {
    let text = read_small(git_file).ok()?;
    let line = text.lines().find_map(|line| line.strip_prefix("gitdir:"))?;
    dir_named(matter, git_file.parent()?, line, mounts)
}

/// Where the folder `line` names (absolute, or relative to `base`) leads, canonical, if it
/// exists: a `gitdir:` line or a `commondir` file's. By [`named_with`] and the file system.
fn dir_named(matter: &Path, base: &Path, line: &str, mounts: &Mounts) -> Option<Named> {
    named_with(matter, base, line, mounts, read_if_link, |path| {
        dunce::canonicalize(path)
    })
}

/// [`dir_named`] with `read` and `resolve` in place of reading a link and canonicalize. The
/// path is classified as written, then name by name and link by link ([`link_to_network`]),
/// and `resolve` runs only once every name and link on the way is local. A network path is
/// skipped with a warning that leaves the path out; an unknown one is [`Named::Unknown`]. The
/// matter itself is left out too: naming it would make the whole matter read-only.
fn named_with(
    matter: &Path,
    base: &Path,
    line: &str,
    mounts: &Mounts,
    read: impl Fn(&Path) -> io::Result<Option<PathBuf>>,
    resolve: impl FnOnce(&Path) -> io::Result<PathBuf>,
) -> Option<Named> {
    let written = Path::new(line.trim());
    let off = match classify(written, mounts) {
        network::Place::Local(named) => {
            // A `subst` drive's mapping is a whole path, whatever this platform calls absolute.
            let joined = if named.is_absolute() || named != written {
                named
            } else {
                base.join(named)
            };
            match link_to_network(&joined, mounts, read) {
                None => return resolved(matter, resolve(&joined).ok()?),
                Some(stop) => stop,
            }
        }
        network::Place::Off(off) => (written.to_owned(), off),
    };
    match off {
        (_, Off::Network) => {
            tracing::warn!("the matter walk skipped a git folder named on a network path");
            None
        }
        (path, Off::Unknown) => Some(Named::Unknown(path)),
    }
}

/// [`Named`] for the canonical `dir`; the matter itself is `None`.
fn resolved(matter: &Path, dir: PathBuf) -> Option<Named> {
    if same(&dir, matter) {
        None
    } else if under(&dir, matter) {
        Some(Named::Inside(dir))
    } else {
        Some(Named::Outside(dir))
    }
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
