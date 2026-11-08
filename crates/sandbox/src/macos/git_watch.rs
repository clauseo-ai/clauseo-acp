//! The git watch: the lawyer-side backstop against what an agent leaves in the matter for the
//! lawyer's own git to trust (decision record 0038). The agent runs as the lawyer, so whatever
//! it leaves is the lawyer's, and the lawyer's git, or an editor polling git, would run a
//! repository's hooks and `core.fsmonitor` as the lawyer. Seatbelt refuses a new `.git` by
//! name, but checks only the last name of a rename's destination: a folder holding a `.git`,
//! made in `TMPDIR`, the state folder or the notes folder, moves into the matter under any other
//! name. A bare repository needs no `.git` name at all, and a link can lead git out of the
//! matter. So outside the jail, while each jailed session runs, the watch sets aside:
//!
//! - every new `.git` (a folder, or a file naming a `gitdir:`);
//! - every new repository-shaped folder (`HEAD`, a file or a link, with `objects` and `refs`
//!   folders or a `commondir` file), whole; in a folder that existed at launch, only its `HEAD`;
//! - every new `HEAD` file or link, or its folder when that is new and repository-shaped;
//! - every new link whose target resolves outside the matter, or doesn't resolve.
//!
//! Names compare as the volume does: without the code points HFS+ ignores
//! (`strip_hfs_ignorables`), then by the policy's fold (APFS). How:
//!
//! - `FSEvents` watches the matter (`fsevents.rs`: file events, no defer, 0.1 s latency). A new
//!   or renamed-in item is checked, and a new, renamed-in or cloned folder is scanned whole. A
//!   dropped or must-scan event, or a changed root, rescans the matter. A stream that cannot
//!   start falls back to a full scan every 2 s, logged.
//! - What was there at launch is left alone while it keeps its file ID: the `.git`s the
//!   policy's walk found (the profile denies every write to them), the links it found leading
//!   out, and the bare repositories and folders of the watch's own walk at its start.
//! - A full scan runs once the jailed tree has exited.
//! - Setting aside is one `rename(2)`, never replacing, into
//!   `<git_set_aside>/<UTC time>-<session>-<n>/item` beside a `record.json` (`aside.rs`). The
//!   folder is in the data dir, hidden from the agent: the policy checks it is inside a hidden
//!   path and apart from every path allowed again, and the profile denies it last. Nothing is
//!   ever deleted.
//! - A move that fails while the agent runs (`EXDEV`, a matter on another volume, or anything
//!   else) ends the session through the process tree's kill; once the tree is gone the final
//!   scan renames each such item in place to `<name>-set-aside-by-clauseo-<n>`, which git never
//!   reads, and records it (fail-closed).
//! - Each set-aside reaches the launch's sink as [`Denial::GitSetAside`](crate::Denial),
//!   uncapped.
//! - Scans never follow a link and never enter a network volume (`os::local`).
//!
//! The pieces: [`Watch`] decides (`check.rs`), `aside.rs` moves and records, `run.rs` drives
//! the watch from the stream and the session, `os.rs` is the real file system and
//! `fsevents.rs` the stream.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::policy::opens_as;
use crate::{LinkKind, PathRule, Policy};

mod aside;
mod check;
mod fsevents;
mod os;
mod run;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_live;

pub(crate) use check::Watch;
pub(crate) use run::{Session, start};

/// The `FSEvents` flags the watch reads.
pub(crate) mod flags {
    /// Coalesced events below the path: rescan.
    pub(crate) const MUST_SCAN_SUB_DIRS: u32 = 0x1;
    /// The stream's own queue dropped events.
    pub(crate) const USER_DROPPED: u32 = 0x2;
    /// The kernel dropped events.
    pub(crate) const KERNEL_DROPPED: u32 = 0x4;
    /// The watched root itself, or a folder above it, changed.
    pub(crate) const ROOT_CHANGED: u32 = 0x20;
    /// A volume was mounted below the root.
    pub(crate) const MOUNT: u32 = 0x40;
    /// The item was made.
    pub(crate) const ITEM_CREATED: u32 = 0x100;
    /// The item was renamed, to this path or away from it.
    pub(crate) const ITEM_RENAMED: u32 = 0x800;
    /// The item was cloned (`clonefile`, `cp -c`).
    pub(crate) const ITEM_CLONED: u32 = 0x40_0000;
}

/// The flags that mean the events can't be trusted to be complete.
const RESCAN: u32 =
    flags::MUST_SCAN_SUB_DIRS | flags::USER_DROPPED | flags::KERNEL_DROPPED | flags::ROOT_CHANGED;

/// The flags that mean something arrived at the path.
const ARRIVED: u32 = flags::ITEM_CREATED | flags::ITEM_RENAMED | flags::ITEM_CLONED | flags::MOUNT;

/// One `FSEvents` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Event {
    /// The path, as the volume stores it.
    pub(crate) path: PathBuf,
    /// Its `kFSEventStreamEventFlag*` bits.
    pub(crate) flags: u32,
}

/// What an entry is, read without following a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Dir,
    File,
    Link,
    Other,
}

/// An entry's kind and file ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Meta {
    pub(crate) kind: Kind,
    /// The volume and the inode.
    pub(crate) id: (u64, u64),
}

/// The file system the watch reads and changes: `os::Real`, or the tests' fake.
pub(crate) trait Fs: Send {
    /// The entry at `path`, never through a link; `None` when it is missing or unreadable.
    fn meta(&self, path: &Path) -> Option<Meta>;
    /// The names in folder `dir`, each with its kind.
    fn list(&self, dir: &Path) -> io::Result<Vec<(OsString, Kind)>>;
    /// Where `path` leads with every link followed, canonical; `None` when it leads nowhere.
    fn resolve(&self, path: &Path) -> Option<PathBuf>;
    /// Whether `dir` is on a local volume; a scan never enters one that is not.
    fn local(&self, dir: &Path) -> bool;
    /// Makes `dir` and the folders above it, as the lawyer only.
    fn make_dirs(&self, dir: &Path) -> io::Result<()>;
    /// Makes the one folder `dir`; an existing one is `AlreadyExists`.
    fn make_dir(&self, dir: &Path) -> io::Result<()>;
    /// Renames `from` to `to`, never replacing an entry at `to` (`AlreadyExists`).
    fn rename_new(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Writes a new file; an existing one is `AlreadyExists`.
    fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// The wall time, for records.
    fn now(&self) -> SystemTime;
}

/// Whether the session's agent may still be running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// It may: a failed move must end the session.
    Running,
    /// Its tree is gone: a failed move renames in place.
    Ended,
}

/// What a check asks of the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[must_use]
pub(crate) struct Asks {
    /// The events were incomplete: scan the whole matter.
    pub(crate) rescan: bool,
    /// A move failed while the agent ran: end the session.
    pub(crate) end: bool,
}

impl Asks {
    fn and(self, other: Asks) -> Asks {
        Asks {
            rescan: self.rescan || other.rescan,
            end: self.end || other.end,
        }
    }
}

/// What the launch knew of the matter, from its policy: its `.git`s, and its links that lead
/// out of the matter or nowhere.
#[derive(Debug, Clone, Default)]
pub(crate) struct Launch {
    pub(crate) git: Vec<PathBuf>,
    pub(crate) links: Vec<PathBuf>,
}

impl Launch {
    /// `policy`'s: the read-only rules in the matter named `.git`, and the symbolic links among
    /// its link findings.
    pub(crate) fn of(policy: &Policy) -> Launch {
        Launch {
            git: policy
                .read_only_in_matter()
                .iter()
                .map(PathRule::path)
                .filter(|path| path.file_name().is_some_and(is_git))
                .map(Path::to_owned)
                .collect(),
            links: policy
                .links()
                .iter()
                .filter(|finding| finding.kind == LinkKind::Symlink)
                .map(|finding| finding.path.clone())
                .collect(),
        }
    }
}

/// Whether `name` is `want` as the matter's volume opens names: without the code points HFS+
/// ignores, then by the policy's fold ([`opens_as`]).
fn is_name(name: &OsStr, want: &str) -> bool {
    opens_as(name, want)
}

/// Whether `name` is `.git`.
pub(crate) fn is_git(name: &OsStr) -> bool {
    is_name(name, ".git")
}

/// Whether `name` is `HEAD`.
fn is_head(name: &OsStr) -> bool {
    is_name(name, "HEAD")
}

/// The name of the `HEAD` in a folder's listing that makes it a repository as git's discovery
/// sees one (bare, or a `.git`'s insides): `HEAD` (a file or a link) with an `objects` and a
/// `refs` folder, or with a `commondir` file (a linked worktree's git folder).
fn repo_head(entries: &[(OsString, Kind)]) -> Option<&OsStr> {
    let has = |want: &str, dir: bool| {
        entries.iter().any(|(name, kind)| {
            let fits = if dir {
                *kind == Kind::Dir
            } else {
                matches!(kind, Kind::File | Kind::Link)
            };
            fits && is_name(name, want)
        })
    };
    let head = entries
        .iter()
        .find(|(name, kind)| matches!(kind, Kind::File | Kind::Link) && is_head(name))?;
    let shaped = (has("objects", true) && has("refs", true)) || has("commondir", false);
    shaped.then_some(head.0.as_os_str())
}
