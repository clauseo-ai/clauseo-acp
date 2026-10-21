//! The Unix half of a command's tree: a process group of its own (`process_group(0)`, so the
//! group's id is the command's pid), signalled through nix's `killpg`, and registered with the
//! leader (`leader.rs`) so it is stopped when xtask dies.
//!
//! A group of its own is a background group on xtask's terminal, so an `Output::Inherit` command
//! that reads the terminal gets SIGTTIN and stops, silently, until its limit or Ctrl-C. Before
//! the door, git and ssh prompts (a credential, a passphrase) during `cargo install` worked. So an
//! `Output::Inherit` command gets `GIT_TERMINAL_PROMPT=0` and, unless xtask's own environment sets
//! `GIT_SSH_COMMAND`, `GIT_SSH_COMMAND="ssh -o BatchMode=yes"`: git and ssh then fail at once
//! instead of prompting. Costs left: any other program that reads `/dev/tty` from an
//! `Output::Inherit` command still stops until its limit or Ctrl-C, and the `GIT_SSH_COMMAND` set
//! here takes the place of a `GIT_SSH` or `core.sshCommand` the developer set.

use super::leader;
use super::running::{Streams, Tree};
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

/// The variables an `Output::Inherit` command gets so that git and ssh fail instead of prompting
/// on a terminal it cannot read. `GIT_SSH_COMMAND` is set only when xtask's own environment does
/// not set it (`prompts_fail`).
const NO_PROMPT_ENV: [(&str, &str); 2] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_SSH_COMMAND", "ssh -o BatchMode=yes"),
];

/// The process group a command leads.
pub(super) struct Group {
    group: Pid,
}

impl Tree for Group {
    const NAME: &'static str = "process group";

    /// Makes `command` start in a new process group; with `Streams::Inherit`, also with git and
    /// ssh prompts turned off (`prompts_fail`), since the group cannot read the terminal.
    fn prepare(command: &mut Command, streams: Streams) {
        command.process_group(0);
        if let Streams::Inherit = streams {
            prompts_fail(command, std::env::var_os("GIT_SSH_COMMAND").is_some());
        }
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

/// Sets `NO_PROMPT_ENV` on `command`, all but `GIT_SSH_COMMAND` when `ssh_set` says xtask's own
/// environment sets it, which the command then inherits.
fn prompts_fail(command: &mut Command, ssh_set: bool) {
    for (name, value) in NO_PROMPT_ENV {
        if !(ssh_set && name == "GIT_SSH_COMMAND") {
            command.env(name, value);
        }
    }
}

/// Sends `signal` to process group `group`. A group with no process left (`ESRCH`) is stopped
/// already, so that is not an error. `EPERM` (no process in it could be signalled, which macOS
/// also answers for a group of zombies) is returned; `Running::stop` judges it.
pub(super) fn signal(group: Pid, signal: Signal) -> io::Result<()> {
    match killpg(group, signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(errno) => Err(io::Error::from(errno)),
    }
}

/// Whether no process is left in `group`. `EPERM` means one is: owned by another user, or, on
/// macOS, exited and not yet reaped (a zombie), which its new parent reaps soon.
pub(super) fn empty(group: Pid) -> bool {
    killpg(group, None) == Err(Errno::ESRCH)
}
