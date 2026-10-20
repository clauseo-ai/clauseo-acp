//! The Unix half of a command's tree: a process group of its own (`process_group(0)`, so the
//! group's id is the command's pid), signalled through nix's `killpg`, and registered with the
//! leader (`leader.rs`) so it is stopped when xtask dies.

use super::leader;
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

/// The process group a command leads.
pub(super) struct Tree {
    group: Pid,
}

impl Tree {
    /// Makes `command` start in a new process group.
    pub(super) fn prepare(command: &mut Command) {
        command.process_group(0);
    }

    /// The tree of `child`, just started from a `prepare`d command, registered with the leader.
    /// When that fails, the group gets SIGKILL and the error is returned; the caller reaps.
    pub(super) fn adopt(child: &mut Child) -> io::Result<Tree> {
        let Ok(raw) = i32::try_from(child.id()) else {
            drop(child.kill());
            return Err(io::Error::other(format!(
                "pid {} does not fit a process group id",
                child.id()
            )));
        };
        let group = Pid::from_raw(raw);
        if let Err(error) = leader::register(group) {
            drop(signal(group, Signal::SIGKILL));
            return Err(error);
        }
        Ok(Tree { group })
    }

    /// SIGTERM to the group.
    pub(super) fn terminate(&self) -> io::Result<()> {
        signal(self.group, Signal::SIGTERM)
    }

    /// SIGKILL to the group.
    pub(super) fn kill(&self) -> io::Result<()> {
        signal(self.group, Signal::SIGKILL)
    }

    /// SIGKILL to what is left of the group, then tells the leader the group is no longer
    /// xtask's to stop, so a later group with the same id is left alone. The command was reaped
    /// already, so an empty group's id is free again; a new group would have to take that exact id
    /// in between, which macOS and Linux, handing out pids in rising order, do not do.
    pub(super) fn finish(&self) {
        drop(self.kill());
        leader::release(self.group);
    }
}

/// Sends `signal` to process group `group`. A group with no process left (`ESRCH`) is stopped
/// already, so that is not an error.
pub(super) fn signal(group: Pid, signal: Signal) -> io::Result<()> {
    match killpg(group, signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(errno) => Err(io::Error::from(errno)),
    }
}
