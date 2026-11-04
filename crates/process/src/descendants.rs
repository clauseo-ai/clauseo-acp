//! What an agent started, and ending it. [`Descendants`] lists the agent's descendants: on Unix by
//! parent pid from the agent down (a command in a session of its own has left the agent's
//! process group, but it is still the child of the process that started it); on Windows from the
//! agent's Job Object, which holds every process the agent started, with each one's parent from
//! a Toolhelp snapshot. Each listed process carries its start time, so a pid the OS has given to
//! another process since is never signalled.
//!
//! [`Descendants::end_new`] ends what is new since an earlier [`Snapshot`] and started before a
//! [`Cutoff`]: the host lists when a turn's prompt goes out, and takes the cutoff on its own
//! thread when the turn ends cancelled, before the next turn can start, so what the next turn
//! starts is never ended.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::Timer;
use crate::os;

/// How long a sweep waits between its two passes: on Unix between SIGTERM and SIGKILL, on
/// Windows before it ends what the first pass's processes started meanwhile.
const END_GRACE: Duration = Duration::from_secs(2);

/// One running descendant of an agent, as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Proc {
    pub(crate) pid: u32,
    /// Its parent's pid as the OS last recorded it; `None` when the OS did not say. On Windows
    /// it can name a parent that has exited (and a pid since reused): see [`is_child_of`].
    pub(crate) parent: Option<u32>,
    /// Its start in the OS's own unit (Linux: clock ticks since boot; macOS: microseconds since
    /// the epoch; Windows: 100 ns since 1601). Compared only with starts and cutoffs read from
    /// the same OS clock.
    pub(crate) start: u64,
    /// It leads a process group of its own: its pid is the group's id (Unix only).
    pub(crate) leads_group: bool,
    /// Its executable, when the OS says.
    pub(crate) exe: Option<PathBuf>,
}

impl Proc {
    /// Whether `other` is the same process: the same pid with the same start.
    fn is(&self, other: &Proc) -> bool {
        self.pid == other.pid && self.start == other.start
    }
}

/// An agent's descendants, for the caller to list and end while the agent runs. Cheap to clone,
/// `Send`, and it outlives the [`crate::Handle`] it came from; it keeps nothing alive (on
/// Windows it holds the job weakly, so the job still closes with the handle).
#[derive(Debug, Clone)]
pub struct Descendants {
    root: os::Root,
}

/// The agent's descendants at one moment, or that they could not be listed.
#[derive(Debug, Clone)]
pub struct Snapshot {
    listing: Listing,
}

/// A listing, or its failure: a failed listing is never read as "no descendants".
#[derive(Debug, Clone)]
enum Listing {
    /// The agent was gone, or the OS refused the listing.
    Failed,
    Taken(Vec<Proc>),
}

/// An instant on the clock the OS stamps process starts with, read rounded down to that clock's
/// unit: Linux clock ticks since boot (`CLOCK_BOOTTIME` in `_SC_CLK_TCK` units, usually 10 ms),
/// macOS microseconds of wall time, Windows the system time in 100 ns units as
/// `GetSystemTimeAsFileTime` gives it (it moves in timer ticks, about 15.6 ms). A process
/// started after it has a start at or past it, so [`Descendants::end_new`] ends only processes
/// whose start is strictly before it; a process started in the same unit as the cutoff is
/// spared. On macOS and Windows a wall clock set back between the cutoff and a start can make a
/// later process look earlier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cutoff(u64);

impl Cutoff {
    /// The cutoff now; `None` when the OS clock could not be read (another Unix, or a failed
    /// call). A cheap call: one or two system calls, no file read.
    pub fn now() -> Option<Cutoff> {
        os::now().map(Cutoff)
    }
}

/// What [`Descendants::end_new`] did. Not `#[non_exhaustive]`: a new outcome should make every
/// match on it fail to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Swept {
    /// The earlier snapshot or the current listing failed, so nothing was ended.
    NotListed,
    /// Nothing new was running.
    NothingNew,
    /// This many new processes were found and ended (with what they started meanwhile).
    Ended {
        /// The new processes the first listing found.
        processes: usize,
    },
}

impl Descendants {
    pub(crate) fn new(root: os::Root) -> Descendants {
        Descendants { root }
    }

    /// The agent's running descendants now. Blocking, and cheap on macOS and Windows (a few
    /// calls per descendant); on Linux it reads `/proc/<pid>/stat` of every process on the
    /// machine, so call it off any thread that must stay responsive.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            listing: self.listing(),
        }
    }

    /// Ends every descendant running now that `before` did not hold and that started before
    /// `cutoff` (the targets), with what the targets start meanwhile. On Unix: SIGTERM to each
    /// target (to its group too when it leads one, as a command started in a session of its own
    /// does), 2 s on `timer`, then SIGKILL to the targets still running and to every process
    /// found below one of them. On Windows: `TerminateProcess` on each target, then after the
    /// same 2 s on the targets still running and every process found below one of them, with
    /// each target's handle held across the wait so its pid names no other process. A
    /// process that started at or after `cutoff` and is no target's descendant is never ended,
    /// so what the agent starts for a later turn survives this sweep.
    ///
    /// A new process whose executable is in `keep` (an MCP server the agent started during the
    /// turn) is left running, with every new process under it. Paths compare exactly (ASCII
    /// case aside on Windows), so pass them resolved as the OS names a running executable.
    /// Nothing is ended when `before` failed: a failed listing never makes every descendant look
    /// new.
    ///
    /// The listings block (see [`Descendants::snapshot`]); run it off any thread that must stay
    /// responsive. A process the agent starts and that re-parents itself away (a double fork on
    /// Unix) is not found, and a process the agent starts during `before`'s listing may count as
    /// old.
    pub async fn end_new<T: Timer>(
        &self,
        before: &Snapshot,
        cutoff: Cutoff,
        keep: &[PathBuf],
        timer: &T,
    ) -> Swept {
        let Listing::Taken(old) = &before.listing else {
            return Swept::NotListed;
        };
        let Listing::Taken(now) = self.listing() else {
            return Swept::NotListed;
        };
        let targets = new_since(old, &now, cutoff, keep);
        if targets.is_empty() {
            return Swept::NothingNew;
        }
        let processes = targets.len();
        #[cfg(unix)]
        for process in &targets {
            os::signal(process, os::Ask::Terminate);
        }
        // Only a target whose handle `terminate` holds stays one: that handle stays open until
        // the second pass is done, so its pid names no other process meanwhile, and a process
        // listed with it as parent was started by it (an exited target whose parent still holds
        // it opens too, and stays pinned). A target that could not be opened, or whose pid named
        // another process, holds none, so its pid may go to a later process, and it is no parent
        // in the second pass.
        #[cfg(windows)]
        let (targets, pinned) = held(targets, os::terminate);
        timer.after(END_GRACE).await;
        // A target still running with its listed start is signalled again; one already ended
        // fails that check and is skipped. A process the next turn started is none of these.
        let again = match self.listing() {
            Listing::Taken(again) => again,
            Listing::Failed => Vec::new(),
        };
        for process in below(targets, &again, keep) {
            #[cfg(unix)]
            os::signal(&process, os::Ask::Kill);
            #[cfg(windows)]
            drop(os::terminate(&process));
        }
        #[cfg(windows)]
        drop(pinned);
        Swept::Ended { processes }
    }

    fn listing(&self) -> Listing {
        self.root.list().map_or(Listing::Failed, Listing::Taken)
    }
}

impl Snapshot {
    /// The pids it holds, or `None` when the listing failed.
    pub fn pids(&self) -> Option<Vec<u32>> {
        match &self.listing {
            Listing::Failed => None,
            Listing::Taken(processes) => Some(processes.iter().map(|p| p.pid).collect()),
        }
    }

    /// The processes it holds, for the stop's kill; `None` when the listing failed.
    #[cfg(unix)]
    pub(crate) fn into_processes(self) -> Option<Vec<Proc>> {
        match self.listing {
            Listing::Failed => None,
            Listing::Taken(processes) => Some(processes),
        }
    }
}

/// SIGKILL to each of `processes` still running with its listed start, its group too when it
/// leads one: what the stop path adds to the group kill (Unix).
#[cfg(unix)]
pub(crate) fn kill_all(processes: &[Proc]) {
    for process in processes {
        os::signal(process, os::Ask::Kill);
    }
}

/// The processes of `now` that `old` did not hold and that started before `cutoff`, less those
/// `keep` names and every new process below one of those.
fn new_since(old: &[Proc], now: &[Proc], cutoff: Cutoff, keep: &[PathBuf]) -> Vec<Proc> {
    let new: Vec<&Proc> = now
        .iter()
        .filter(|process| !old.iter().any(|earlier| earlier.is(process)))
        .collect();
    let mut kept: Vec<&Proc> = new
        .iter()
        .copied()
        .filter(|process| is_kept(process, keep))
        .collect();
    // Every new process under a kept one is kept too; the walk ends when a pass adds nothing.
    loop {
        let before = kept.len();
        for &process in &new {
            if !kept.iter().any(|k| k.is(process))
                && kept.iter().any(|parent| is_child_of(process, parent, now))
            {
                kept.push(process);
            }
        }
        if kept.len() == before {
            break;
        }
    }
    new.into_iter()
        .filter(|process| process.start < cutoff.0 && !kept.iter().any(|k| k.is(process)))
        .cloned()
        .collect()
}

/// The targets `hold` returns a hold for, with those holds; the rest are dropped (Windows).
#[cfg(any(windows, test))]
fn held<H>(targets: Vec<Proc>, hold: impl Fn(&Proc) -> Option<H>) -> (Vec<Proc>, Vec<H>) {
    targets
        .into_iter()
        .filter_map(|process| hold(&process).map(|h| (process, h)))
        .unzip()
}

/// `targets`, and every process of `again` below one of them (whatever its start: only a target
/// starts it), less those `keep` names.
fn below(mut targets: Vec<Proc>, again: &[Proc], keep: &[PathBuf]) -> Vec<Proc> {
    loop {
        let before = targets.len();
        for process in again {
            if !targets.iter().any(|target| target.is(process))
                && !is_kept(process, keep)
                && targets
                    .iter()
                    .any(|parent| is_child_of(process, parent, again))
            {
                targets.push(process.clone());
            }
        }
        if targets.len() == before {
            break;
        }
    }
    targets
}

/// Whether `child` is a child of `parent`: its recorded parent pid is `parent`'s, it started no
/// earlier than `parent`, and `listing` holds no other process under that pid (a parent that
/// has exited may be absent, as on Windows, where a child keeps its dead parent's pid; a pid
/// reused since is present with another start and refused).
fn is_child_of(child: &Proc, parent: &Proc, listing: &[Proc]) -> bool {
    child.parent == Some(parent.pid)
        && child.start >= parent.start
        && listing
            .iter()
            .filter(|process| process.pid == parent.pid)
            .all(|process| process.is(parent))
}

/// Whether `process` runs one of `keep`.
fn is_kept(process: &Proc, keep: &[PathBuf]) -> bool {
    process
        .exe
        .as_deref()
        .is_some_and(|exe| keep.iter().any(|path| same_path(exe, path)))
}

/// Whether two paths name the same executable as the OS reports it: equal, ignoring ASCII case
/// on Windows.
fn same_path(exe: &Path, keep: &Path) -> bool {
    if cfg!(windows) {
        exe.as_os_str().eq_ignore_ascii_case(keep.as_os_str())
    } else {
        exe == keep
    }
}

#[cfg(test)]
mod tests;
