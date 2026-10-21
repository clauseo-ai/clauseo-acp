//! The Unix half of a command's tree: a process group of its own (`process_group(0)`, so the
//! group's id is the command's pid), signalled through nix's `killpg`, and registered with the
//! leader (`leader.rs`) so it is stopped when xtask dies.

use super::leader;
use super::running::Tree;
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

/// The process group a command leads.
pub(super) struct Group {
    group: Pid,
}

impl Tree for Group {
    const NAME: &'static str = "process group";

    /// Makes `command` start in a new process group.
    fn prepare(command: &mut Command) {
        command.process_group(0);
    }

    /// The tree of `child`, just started from a `prepare`d command, registered with the leader.
    /// When that fails, the group gets SIGKILL and the error is returned; the caller reaps.
    fn adopt(child: &mut Child) -> io::Result<Group> {
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
        Ok(Group { group })
    }

    /// SIGTERM to the group.
    fn terminate(&self) -> io::Result<()> {
        signal(self.group, Signal::SIGTERM)
    }

    /// SIGKILL to the group.
    fn kill(&self) -> io::Result<()> {
        signal(self.group, Signal::SIGKILL)
    }

    fn empty(&self) -> bool {
        empty(self.group)
    }

    /// SIGKILL to what is left of the group, then tells the leader the group is no longer
    /// xtask's to stop, so a later group with the same id is left alone. The command was reaped
    /// already, so once the group is empty its id is free again, and a signal sent then would
    /// reach a new process that took it. That is assumed not to happen: macOS and Linux hand out
    /// pids in rising order and come back to an id only after wrapping at their limit (`PID_MAX`,
    /// 99999, on macOS; `pid_max` on Linux), so pids would have to go all the way round in
    /// between. The SIGTERM at the exit, the checks that the group is empty, and the SIGKILL
    /// after the grace rest on the same assumption once the command is reaped.
    fn finish(&self) {
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

/// Whether no process is left in `group`. `EPERM` means one is, owned by another user.
pub(super) fn empty(group: Pid) -> bool {
    killpg(group, None) == Err(Errno::ESRCH)
}
