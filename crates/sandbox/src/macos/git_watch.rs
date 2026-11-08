//! The git watch: the lawyer-side backstop against what an agent leaves in the matter for the
//! lawyer's own git to trust (decision record 0038). The agent runs as the lawyer, so whatever
//! it leaves is the lawyer's, and the lawyer's git, or an editor polling git, would run a
//! repository's hooks and `core.fsmonitor` as the lawyer. Seatbelt refuses a new `.git` or
//! `HEAD` by name, but checks only the last name of a rename's destination: a folder holding a
//! repository, made in the state folder, moves into the matter under any other name. A link
//! can lead git out of the matter. So outside the jail, while each jailed session runs, the
//! watch looks for:
//!
//! - every new `.git` (a folder, or a file naming a `gitdir:`);
//! - every `HEAD` (a file or a link) in a folder that was not a repository at launch, whatever
//!   surrounds it; a new repository-shaped folder (`HEAD` with `objects` and `refs`, folders
//!   or links, or a `commondir` file) is taken whole;
//! - every new link whose target resolves outside the matter, or doesn't resolve;
//! - anything new at a launch repository's `commondir` target that git would read there.
//!
//! Names compare as the volume does: without the code points HFS+ ignores
//! (`strip_hfs_ignorables`), then by the policy's fold (APFS). How:
//!
//! - `FSEvents` watches the matter (`fsevents.rs`: file events, no defer, 0.1 s latency). Any
//!   event re-checks its parent folder; a new or renamed-in folder is scanned whole. A dropped
//!   or must-scan event, a changed root, or a path outside the matter rescans the matter. A
//!   stream that cannot start refuses the launch. The session's notes folder gets a watch of
//!   its own, the same in every way (its stream, its state file, its scans before and after),
//!   ended with the same session.
//! - A canary checks the stream (`run.rs`): every 10 s a file in the set-aside folder, on the
//!   matter's volume and out of the agent's reach, is rewritten; when its event doesn't come
//!   within 2 s, or it can't be written, the stream is lost: the session ends as for a find,
//!   and the lawyer gets [`Denial::GitWatchLost`](crate::Denial).
//! - Every 30 s a pass lists the folders whose modification or status-change time changed
//!   since the last pass (`check/sweep.rs`), at most 250 ms of work at a time, going on where
//!   it stopped. What it finds ends the session as an event's find does.
//! - What was there at launch is left alone while it keeps its file ID: the `.git`s the
//!   policy's walk found (the profile denies every write to them), the links it found leading
//!   out, and the repositories, `HEAD` folders and folders of the watch's own walk.
//! - On the first find while the agent may run, the watch ends every process of the session:
//!   the process group, the tree, and every process carrying the launch's Seatbelt probe
//!   (`jail_procs.rs`), which finds a `setsid` child re-parented to launchd. The kill goes
//!   before any rescan or 30 s pass. Only once none is left does it scan the matter and set
//!   aside, so nothing races the moves; when one outlives the kill's 10 s, nothing moves.
//! - A watchdog thread (`run/watchdog.rs`) ends the session when one check runs past 2 s,
//!   such as a link's target that makes autofs mount a slow server.
//! - Setting aside is one `rename(2)`, never replacing, into
//!   `<set-aside>/<UTC time>-<session>-<random>/item` beside a `record.json` (`aside.rs`),
//!   with the item's own `HEAD` and `config` renamed. The folder is the data dir's, or a
//!   `.clauseo` folder at the root of the matter's volume when that is another; the profile
//!   denies both. An item that still can't move is neutralised in place: its `HEAD` and
//!   `config` and its own name get a random `-set-aside-by-clauseo-` suffix, and a link is
//!   removed once its target is recorded. If that fails too, the matter's next launch is
//!   refused until the item is gone.
//! - A small state file per matter (`state.rs`) records each running session's launch
//!   baseline. A session whose host quit or crashed before its final scan, or whose processes
//!   outlived the kill, gets that scan at the host's next start ([`start_up`]) or the
//!   matter's next launch, once its surviving processes are killed by its probe (only those
//!   started since its launch); while one is left nothing moves and the matter's launches are
//!   refused. At the
//!   host's quit, [`quit`] kills every running session by its probe and waits a little for
//!   the final scans.
//! - Each set-aside reaches the launch's sink as [`Denial::GitSetAside`](crate::Denial),
//!   uncapped.
//! - Scans never follow a link and never enter a network volume (`os::local`); a matter on
//!   one is never scanned, which is logged. That is a known gap: an agent can leave a
//!   repository in a matter on a network volume and nothing sets it aside.
//!
//! The pieces: [`Watch`] decides (`check.rs`), `aside.rs` moves and records, `run.rs` drives
//! the watch from the stream and the session, `state.rs` keeps the per-matter record, `host.rs`
//! has the host's start and quit, `os.rs` is the real file system and `fsevents.rs` the stream.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::policy::{opens_as, under};
use crate::{LinkKind, PathRule, Policy};

mod aside;
mod check;
mod fsevents;
mod host;
mod os;
mod run;
mod state;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_live;

pub(crate) use check::Watch;
pub use host::{quit, start_up};
pub(crate) use run::{Mode, Session, start};

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

/// A folder's modification and status-change times, seconds and nanoseconds each: a change
/// of its entries moves both, and no one can set the second back.
pub(crate) type Stamp = [i64; 4];

/// The file system the watch reads and changes: `os::Real`, or the tests' fake.
pub(crate) trait Fs: Send {
    /// The entry at `path`, never through a link; `None` when it is missing or unreadable.
    fn meta(&self, path: &Path) -> Option<Meta>;
    /// [`Fs::meta`] with the entry's [`Stamp`], from the same read.
    fn dated(&self, path: &Path) -> Option<(Meta, Stamp)>;
    /// Writes the file at `path`, made if missing, never through a link: the canary.
    fn touch(&self, path: &Path) -> io::Result<()>;
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
    /// Where the link at `path` leads, as written.
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
    /// Removes the link at `path`, never anything else.
    fn remove_link(&self, path: &Path) -> io::Result<()>;
    /// A random number, for names no one can guess.
    fn random(&self) -> u64;
    /// The wall time, for records.
    fn now(&self) -> SystemTime;
}

/// Whether the session's agent may still be running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// It may: a find ends the session, and nothing moves yet.
    Running,
    /// Every process of it is gone: finds are set aside.
    Ended,
}

/// What a check asks of the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[must_use]
pub(crate) struct Asks {
    /// The events were incomplete: scan the whole matter.
    pub(crate) rescan: bool,
    /// Something was found while the agent may run: end the session.
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

/// What the launch knew of the matter, from its policy: its `.git`s, its links that lead out
/// of the matter or nowhere, the paths at its repositories' `commondir` targets inside the
/// matter, and the volume set-aside folder when it is inside the matter.
#[derive(Debug, Clone, Default)]
pub(crate) struct Launch {
    pub(crate) git: Vec<PathBuf>,
    pub(crate) links: Vec<PathBuf>,
    pub(crate) common: Vec<PathBuf>,
    pub(crate) skip: Vec<PathBuf>,
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
            common: policy
                .git
                .commondir
                .iter()
                .flat_map(crate::policy::git_aside::CommonDir::denied)
                .filter(|path| under(path, policy.matter()))
                .collect(),
            skip: policy
                .volume_set_aside()
                .filter(|dir| under(dir, policy.matter()))
                .map(Path::to_owned)
                .into_iter()
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

/// The `HEAD` file or link in a folder's listing, with its kind.
fn head_of(entries: &[(OsString, Kind)]) -> Option<(&OsStr, Kind)> {
    entries
        .iter()
        .find(|(name, kind)| matches!(kind, Kind::File | Kind::Link) && is_head(name))
        .map(|(name, kind)| (name.as_os_str(), *kind))
}

/// The name of the `HEAD` in a folder's listing that makes it a repository as git's discovery
/// sees one (bare, or a `.git`'s insides): `HEAD` (a file or a link) with `objects` and
/// `refs` (folders, or links, which git follows), or with a `commondir` file (a linked
/// worktree's git folder).
fn repo_head(entries: &[(OsString, Kind)]) -> Option<&OsStr> {
    let has = |want: &str, dir: bool| {
        entries.iter().any(|(name, kind)| {
            let fits = if dir {
                matches!(kind, Kind::Dir | Kind::Link)
            } else {
                matches!(kind, Kind::File | Kind::Link)
            };
            fits && is_name(name, want)
        })
    };
    let (head, _) = head_of(entries)?;
    let shaped = (has("objects", true) && has("refs", true)) || has("commondir", false);
    shaped.then_some(head)
}
