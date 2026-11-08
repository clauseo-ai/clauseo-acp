//! Where a path the agent chose really leads, without ever touching a network location: walked
//! name by name, each step checked by [`NetEnv::is_network`] before it is looked at, each link
//! read (never followed by the system) and its target checked the same way before the walk goes
//! on into it. `std::fs::canonicalize` and `dunce::canonicalize` would follow a local link to
//! `/net/host/…` or a Windows link to a UNC target and sign in there, so neither is used on an
//! agent's path.

use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::NetEnv;

#[cfg(test)]
pub(crate) mod tests;

/// The most links one path may pass through before it counts as a loop (Linux's own limit).
const MAX_HOPS: usize = 40;

/// What one name on the disk is, the link itself rather than its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entry {
    Dir,
    File,
    Link,
}

/// The two looks the walk takes at the disk, so a test can see each one.
pub(crate) trait Fs {
    /// What `path` is, never following a link at it (`symlink_metadata`).
    fn entry(&self, path: &Path) -> io::Result<Entry>;
    /// The target a link at `path` names (`read_link`), which the system does not follow.
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
}

/// The real disk.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RealFs;

impl Fs for RealFs {
    fn entry(&self, path: &Path) -> io::Result<Entry> {
        let kind = std::fs::symlink_metadata(path)?.file_type();
        Ok(if kind.is_symlink() {
            Entry::Link
        } else if kind.is_dir() {
            Entry::Dir
        } else {
            Entry::File
        })
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::read_link(path)
    }
}

/// Where a path leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// A local path, resolved, and whether it is a file (or does not exist).
    Local { path: PathBuf, is_file: bool },
    /// It, or a link on the way, names a network location: decided before touching it.
    Network,
    /// A relative path, a link that can't be read or that leads nowhere a path can be read
    /// from (a drive-relative target), an error other than a missing name, or a loop.
    Unresolved,
}

/// One part of a path still to walk.
enum Part {
    Root(OsString),
    Up,
    Name(OsString),
}

/// `path`'s parts, last first, to pop in order.
fn parts_of(path: &Path) -> Vec<Part> {
    let mut parts: Vec<Part> = path
        .components()
        .filter_map(|part| match part {
            Component::Prefix(_) | Component::RootDir => Some(Part::Root(part.as_os_str().into())),
            Component::CurDir => None,
            Component::ParentDir => Some(Part::Up),
            Component::Normal(name) => Some(Part::Name(name.into())),
        })
        .collect();
    parts.reverse();
    parts
}

/// Where `path` leads on `fs`: an existing name resolved (links read and walked, `..` taken on
/// what is resolved so far), a missing one and the names after it added as written (a `..`
/// there drops the name before it) and read as a file. A network location anywhere on the way
/// is [`Resolved::Network`], found before the walk touches it.
pub(crate) fn resolve(path: &Path, fs: &dyn Fs, net: &NetEnv) -> Resolved {
    if net.is_network(path) {
        return Resolved::Network;
    }
    let path = net.plain(path);
    if !path.is_absolute() {
        return Resolved::Unresolved;
    }
    let mut at = PathBuf::new();
    let mut rest = parts_of(&path);
    let mut hops = 0_usize;
    let mut missing = false;
    let mut last = Entry::Dir;
    while let Some(part) = rest.pop() {
        match part {
            Part::Root(root) => at.push(root),
            Part::Up => {
                at.pop();
                last = Entry::Dir;
            }
            Part::Name(name) => {
                let next = at.join(&name);
                if missing {
                    at = next;
                    continue;
                }
                if net.is_network(&next) {
                    return Resolved::Network;
                }
                match fs.entry(&next) {
                    Ok(Entry::Link) => {
                        hops = hops.saturating_add(1);
                        if hops > MAX_HOPS {
                            return Resolved::Unresolved;
                        }
                        let Ok(target) = fs.read_link(&next) else {
                            return Resolved::Unresolved;
                        };
                        if net.is_network(&target) {
                            return Resolved::Network;
                        }
                        let target = net.plain(&target);
                        if target.is_absolute() {
                            at = PathBuf::new();
                        } else if target.has_root()
                            || target
                                .components()
                                .any(|c| matches!(c, Component::Prefix(_)))
                        {
                            return Resolved::Unresolved;
                        } else if net.is_network(&at.join(&target)) {
                            return Resolved::Network;
                        }
                        rest.extend(parts_of(&target));
                    }
                    Ok(entry) => {
                        last = entry;
                        at = next;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        missing = true;
                        at = next;
                    }
                    Err(_) => return Resolved::Unresolved,
                }
            }
        }
    }
    Resolved::Local {
        path: at,
        is_file: missing || last == Entry::File,
    }
}
