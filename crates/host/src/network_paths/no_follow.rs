//! The last step of touching a path an agent named: open it so that no name on it is followed
//! when it is a link (a symlink, a junction, any reparse point that is not a known local kind).
//! The network-safe walk ([`super::resolve::walk`]) gives back a path with every link already
//! resolved ([`WalkedPath`]); between the walk and the touch an agent that can write somewhere on
//! that path could swap a folder for a link, and a plain open would follow it, to a network
//! location too. [`open_no_follow`] refuses instead ([`LinkOnTheWay`], logged by its kind, never
//! the path), and the caller acts on the handle it gets: its kind, its identity, the entries of a
//! folder by name ([`NoFollow`] is the held-open check's [`DocumentFolder`]).
//!
//! - Linux: `openat2` with `RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`; on a kernel without it
//!   (`ENOSYS`) an `openat` per name, each `O_NOFOLLOW` and checked not to be a link. Never a
//!   plain open.
//! - macOS: the same `openat` per name with `O_NOFOLLOW`, from `/`.
//! - Windows: `NtCreateFile` per name relative to the folder's handle, from the volume's own
//!   device (the drive letter read with `QueryDosDeviceW`, a `subst` followed, so a letter
//!   mapped to a share since the walk is never opened). Each name is first read from its
//!   folder's listing (`NtQueryDirectoryFile`, which opens nothing): a link refuses, as does a
//!   reparse point not known to be local; the open itself takes `FILE_OPEN_REPARSE_POINT` and
//!   its attributes are checked again.
//!
//! The walk's own looks ([`super::resolve::RealFs`]) go through the same opens, so no look at an
//! agent's path, before or after the walk vouches for it, follows a link the walk did not read.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_matters::{DocumentFolder, HeldOpen};
use clauseo_protocol::MatterPath;

use super::resolve::Entry;

#[cfg(unix)]
#[path = "no_follow/unix.rs"]
mod sys;
#[cfg(windows)]
#[path = "no_follow/windows.rs"]
mod sys;

#[cfg(any(windows, test))]
mod listing;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests;

/// A path the network-safe walk vouched for and resolved ([`super::touchable`]): absolute,
/// every link on it already followed, so it should hold none. Only [`open_no_follow`] touches
/// it, and that refuses a link that is there now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalkedPath(PathBuf);

impl WalkedPath {
    /// The walk's result for a path it vouched for.
    pub(super) fn new(path: PathBuf) -> Self {
        Self(path)
    }

    /// The path, for comparing its text (the matter folder's compare); never to open.
    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }

    /// This path with `name` added, still never to open but by [`open_no_follow`].
    pub(crate) fn join(&self, name: impl AsRef<Path>) -> Self {
        Self(self.0.join(name))
    }
}

/// What an open is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// Its kind and identity only (`O_PATH` on Linux, `FILE_READ_ATTRIBUTES` on Windows).
    Look,
    /// A folder whose entries are looked at by name only (`O_PATH` on Linux: no read
    /// permission needed, as for a `stat` through it).
    Folder,
    /// A folder whose entries are listed, or looked at by name.
    List,
    /// A file read (the stale rule's lock query on macOS).
    Read,
    /// A file read with no sharing on Windows (a sharing violation shows it held); a read
    /// elsewhere.
    Exclusive,
}

/// What a handle is: the walk's [`Entry`] without links, which a no-follow open never gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Dir,
    File,
}

/// The error a no-follow open gives when a name on the path is a link now (or a reparse point
/// of a kind not known to be local): the touch is refused.
#[derive(Debug)]
pub(crate) struct LinkOnTheWay;

impl fmt::Display for LinkOnTheWay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a name on a walked path is a link now")
    }
}

impl std::error::Error for LinkOnTheWay {}

/// The [`LinkOnTheWay`] error.
pub(crate) fn link_on_the_way() -> io::Error {
    io::Error::other(LinkOnTheWay)
}

/// Whether `error` is [`LinkOnTheWay`].
pub(crate) fn is_link_on_the_way(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(<dyn std::error::Error + Send + Sync>::is::<LinkOnTheWay>)
}

/// An open handle to a file or folder, reached with no link followed on the way.
#[derive(Debug)]
pub(crate) struct NoFollow(sys::Handle);

/// Opens the walked path `path` for `access` with no name on it followed, the one way callers
/// touch a path an agent named once the walk vouched for it. A name that is a link now (the walk
/// saw none: a swap since) refuses with [`LinkOnTheWay`], logged at warn by its kind; any other
/// failure is logged at debug by its kind. Never logs the path.
pub(crate) fn open_no_follow(path: &WalkedPath, access: Access) -> io::Result<NoFollow> {
    let opened = open_path(path.as_path(), access);
    if let Err(error) = &opened {
        log_refusal(error);
    }
    opened
}

/// Logs a failed no-follow open by its kind, never the path.
pub(crate) fn log_refusal(error: &io::Error) {
    if is_link_on_the_way(error) {
        tracing::warn!(
            kind = "link_on_the_way",
            "a walked path holds a link now; not touched"
        );
    } else if error.kind() != io::ErrorKind::NotFound {
        tracing::debug!(kind = ?error.kind(), "a walked path could not be opened");
    }
}

/// The held-open check ([`clauseo_matters::held_open_resolved`]) of the walked path `walked` in
/// the matter `folder`: the document's folder opened with no link followed, every look at its
/// entries through that handle. A link on the way refuses the check (nothing shows the file
/// held, nothing is touched past it), logged by its kind.
pub(crate) fn held_open_walked(
    folder: &Path,
    walked: &WalkedPath,
) -> Option<(MatterPath, HeldOpen)> {
    clauseo_matters::held_open_resolved(folder, walked.as_path(), |parent| {
        let opened = open_path(parent, Access::List);
        if let Err(error) = &opened {
            log_refusal(error);
        }
        opened
    })
}

/// [`open_no_follow`] for any absolute path, unlogged: the walk's own looks at each name.
pub(crate) fn open_path(path: &Path, access: Access) -> io::Result<NoFollow> {
    sys::open_path(path, access).map(NoFollow)
}

/// `name` when it is one plain name (no separator, not `.` or `..`, not empty).
fn single(name: &OsStr) -> io::Result<&OsStr> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(only)), None) if only == name => Ok(name),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a single name",
        )),
    }
}

impl NoFollow {
    /// Whether it is a folder or a file.
    pub(crate) fn kind(&self) -> io::Result<Kind> {
        self.0.kind()
    }

    /// Its identity: device or volume, and file number.
    pub(crate) fn identity(&self) -> io::Result<(u64, u128)> {
        self.0.identity()
    }

    /// What the entry `name` of this folder is, a link reported as one and never followed.
    pub(crate) fn entry_at(&self, name: &OsStr) -> io::Result<Entry> {
        self.0.entry_at(single(name)?)
    }

    /// The target the link `name` in this folder names, never followed.
    pub(crate) fn read_link_at(&self, name: &OsStr) -> io::Result<PathBuf> {
        self.0.read_link_at(single(name)?)
    }

    /// The identity of the entry `name` in this folder, a link's own when it is one.
    pub(crate) fn identity_at(&self, name: &OsStr) -> io::Result<(u64, u128)> {
        self.0.identity_at(single(name)?)
    }

    /// The entry `name` of this folder opened for `access`; a link refuses ([`LinkOnTheWay`]).
    pub(crate) fn open_at(&self, name: &OsStr, access: Access) -> io::Result<Self> {
        self.0.open_at(single(name)?, access).map(Self)
    }

    /// Up to `max` of this folder's entry names (`.` and `..` left out).
    pub(crate) fn names(&self, max: usize) -> io::Result<Vec<OsString>> {
        Ok(self
            .entries(max)?
            .into_iter()
            .map(|(name, _)| name)
            .collect())
    }

    /// Up to `max` of this folder's entries (`.` and `..` left out), each with what it is as
    /// the listing tells it, a link as one; `None` when that can't be told, or on Windows for a
    /// reparse point of a kind not known to be local (never to be opened).
    pub(crate) fn entries(&self, max: usize) -> io::Result<Vec<(OsString, Option<Entry>)>> {
        self.0.entries(max)
    }

    /// The path the system gives this handle, each name in its canonical form (Windows' long
    /// names), `\\?\` taken off.
    #[cfg(windows)]
    pub(crate) fn final_path(&self) -> io::Result<PathBuf> {
        self.0.final_path()
    }

    /// The handle as a file.
    fn into_file(self) -> File {
        self.0.into_file()
    }
}

impl DocumentFolder for NoFollow {
    fn has(&self, name: &str) -> bool {
        self.entry_at(OsStr::new(name)).is_ok()
    }

    fn names(&self, max: usize) -> Option<Vec<String>> {
        Some(
            Self::names(self, max)
                .ok()?
                .into_iter()
                .filter_map(|name| name.into_string().ok())
                .collect(),
        )
    }

    fn open(&self, name: &str, exclusive: bool) -> io::Result<File> {
        let access = if exclusive {
            Access::Exclusive
        } else {
            Access::Read
        };
        let opened = self.open_at(OsStr::new(name), access);
        if let Err(error) = &opened {
            log_refusal(error);
        }
        opened.map(Self::into_file)
    }

    fn local_apfs_or_hfs(&self) -> Option<bool> {
        self.0.local_apfs_or_hfs()
    }
}
