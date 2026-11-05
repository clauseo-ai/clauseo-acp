//! The Unix half: the agent leads a process group of its own, and the ladder signals the group.
//! Its descendants are listed by parent pid (`linux.rs`, `macos.rs`), because a command an agent
//! starts in a session of its own (Codex does) has left the group.

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

use async_process::Child;
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;

use crate::descendants::Proc;
use crate::{Error, SpawnOptions};

#[cfg(target_os = "linux")]
use super::linux as listing;
#[cfg(target_os = "macos")]
use super::macos as listing;

/// Puts the agent in a new process group whose id is its own pid, so a signal to the group
/// reaches every process it starts that stays in the group. Safe std, no `pre_exec`, except on
/// Linux for [`SpawnOptions::end_with_parent`] (`linux.rs`).
pub(crate) fn prepare(command: &mut Command, opts: &SpawnOptions) {
    command.process_group(0);
    #[cfg(target_os = "linux")]
    if opts.end_with_parent {
        listing::end_with_parent(command);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = opts;
}

/// The agent's process group.
#[derive(Debug)]
pub(crate) struct Tree {
    group: Pid,
}

impl Tree {
    /// The group `child` leads, started from a `prepare`d command. On failure the child is ended.
    pub(crate) fn adopt(child: &mut Child) -> Result<Tree, Error> {
        match i32::try_from(child.id()) {
            Ok(pid) => Ok(Tree {
                group: Pid::from_raw(pid),
            }),
            Err(source) => {
                drop(child.kill());
                Err(Error::Os {
                    what: "the agent's pid is out of range",
                    source: io::Error::other(source),
                })
            }
        }
    }

    /// Sends SIGTERM to the group. A failure is dropped: the ladder's next step still comes.
    pub(crate) fn terminate(&self) {
        self.signal(Signal::SIGTERM);
    }

    /// Sends SIGKILL to the group. Returns whether it went, an empty group (`ESRCH`) counting as
    /// gone; the ladder kills the agent's own pid as well either way.
    pub(crate) fn kill(&self) -> bool {
        self.signal(Signal::SIGKILL)
    }

    fn signal(&self, signal: Signal) -> bool {
        matches!(killpg(self.group, signal), Ok(()) | Err(Errno::ESRCH))
    }
}

/// Where a listing of the agent's descendants starts: the agent's pid and its start, read while
/// the agent was this process's unreaped child, so a later reuse of the pid is told apart.
#[derive(Debug, Clone)]
pub(crate) struct Root {
    pid: u32,
    /// `None` when the start could not be read: then nothing is ever listed.
    start: Option<u64>,
}

impl Root {
    /// The root at agent `pid`, which must be this process's child and not yet reaped.
    pub(crate) fn of(_tree: &Tree, pid: u32) -> Root {
        Root {
            pid,
            start: start_of(pid),
        }
    }

    /// Every running descendant of the agent, by parent pid; `None` when the agent is gone or
    /// the OS listing failed. Blocking: on Linux it reads `/proc/<pid>/stat` of every process.
    pub(crate) fn list(&self) -> Option<Vec<Proc>> {
        list(self.pid, self.start?)
    }
}

/// How hard [`signal`] asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ask {
    Terminate,
    Kill,
}

/// Signals `process` if it still runs with the start it was listed with (so a reused pid is
/// never signalled): its group too when it leads one (a command in a session of its own takes
/// its whole group with it), then the process itself. Returns whether a signal went.
pub(crate) fn signal(process: &Proc, ask: Ask) -> bool {
    let Ok(raw) = i32::try_from(process.pid) else {
        return false;
    };
    if start_of(process.pid) != Some(process.start) {
        return false;
    }
    let signal = match ask {
        Ask::Terminate => Signal::SIGTERM,
        Ask::Kill => Signal::SIGKILL,
    };
    let pid = Pid::from_raw(raw);
    // Its pid is its group's id, verified just above, so the group is the one listed.
    let group = process.leads_group && killpg(pid, signal).is_ok();
    let own = kill(pid, signal).is_ok();
    group || own
}

/// Whether process `pid` is still running (a zombie is not), for tests.
#[cfg(feature = "test-support")]
pub(crate) fn running(pid: u32) -> bool {
    start_of(pid).is_some()
}

/// SIGKILL to `pid`, for a test that cleans up after a failed assert.
#[cfg(feature = "test-support")]
pub(crate) fn kill_pid(pid: u32) {
    if let Ok(pid) = i32::try_from(pid) {
        kill(Pid::from_raw(pid), Signal::SIGKILL).unwrap_or_default();
    }
}

/// Now on the clock process starts are stamped with (see [`crate::Cutoff`]); another Unix
/// reads none.
pub(crate) fn now() -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return listing::now();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    None
}

/// Whether process `pid` started before a cutoff taken now, for tests: a sweep ends only what
/// did.
#[cfg(feature = "test-support")]
pub(crate) fn started_before_now(pid: u32) -> bool {
    start_of(pid)
        .zip(now())
        .is_some_and(|(start, now)| start < now)
}

/// The start of running process `pid`, `None` when it is gone or has exited.
fn start_of(pid: u32) -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return listing::start_of(pid);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        drop(pid);
        None
    }
}

/// The running descendants of `root`; another Unix lists nothing (`None`).
fn list(root: u32, root_start: u64) -> Option<Vec<Proc>> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return listing::list(root, root_start);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        drop((root, root_start));
        None
    }
}
