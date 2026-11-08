//! Where a path the agent chose really leads, on `crate::network_paths`' fail-closed walk
//! (#766): never touching a network location, each local name in its real form. This adds what
//! a card needs: whether the path is a file, and why a refused one was refused (for the log).
//! [`resolve`] is 3a's one call to the walk (a change to #766's API changes it alone), and
//! only the local path it gives is used afterwards. R18 3b calls it again at grant time.

use std::cell::Cell;
use std::io;
use std::path::{Component, Path, PathBuf};

pub(crate) use crate::network_paths::resolve::{Disk, Entry, Fs};
use crate::network_paths::resolve::{Walked, walk};
use crate::network_paths::{NetEnv, refused_windows_name};

#[cfg(test)]
mod tests;

/// Where a path leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// A local path, resolved, and whether it is a file (or does not exist).
    Local { path: PathBuf, is_file: bool },
    /// It, or a link on the way, names a network location: decided before touching it.
    Network,
    /// Anything else the walk can't vouch for (a relative path, a link that can't be read, an
    /// error other than a missing name, a loop, a name Windows reads as another place). Refused
    /// as a network path is: never offered.
    Unresolved,
}

/// Where `path` leads on `disk` ([`walk`]), and whether it is a file. A name Windows reads as
/// another place (`refused_windows_name`: a trailing dot or space, a `:` stream, a device name)
/// is [`Resolved::Unresolved`] before anything is touched. An 8.3 name is walked in its long
/// form; the identity layer pins it to its place.
pub(crate) fn resolve(path: &Path, disk: &Disk<'_>) -> Resolved {
    let net = disk.net;
    if net.is_network(path) {
        return Resolved::Network;
    }
    if refused_windows_name(path, net) {
        return Resolved::Unresolved;
    }
    let seen = Seen {
        fs: disk.fs,
        net,
        network: Cell::new(false),
    };
    let watched = Disk {
        fs: &seen,
        real: disk.real,
        net,
    };
    match walk(path, &watched) {
        Walked::Local(path) => {
            let is_file = match disk.fs.entry(&path) {
                Ok(Entry::Dir | Entry::Link) => false,
                Ok(Entry::File) => true,
                Err(error) => error.kind() == io::ErrorKind::NotFound,
            };
            Resolved::Local { path, is_file }
        }
        Walked::Refused if seen.network.get() => Resolved::Network,
        Walked::Refused => Resolved::Unresolved,
    }
}

/// The disk as the walk sees it, noting whether a link it read leads to a network location:
/// only to tell the log why a path was refused.
struct Seen<'a> {
    fs: &'a dyn Fs,
    net: &'a NetEnv,
    network: Cell<bool>,
}

impl Fs for Seen<'_> {
    fn entry(&self, path: &Path) -> io::Result<Entry> {
        self.fs.entry(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        let target = self.fs.read_link(path)?;
        let joined = path.parent().map(|folder| lexical(&folder.join(&target)));
        if self.net.is_network(&target) || joined.is_some_and(|p| self.net.is_network(&p)) {
            self.network.set(true);
        }
        Ok(target)
    }

    fn identity(&self, path: &Path) -> Option<(u64, u128)> {
        self.fs.identity(path)
    }
}

/// `path` with `.` and `..` taken off by their text (for the log's word only).
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => out.push(part),
        }
    }
    out
}
