//! The Unix half: the agent leads a process group of its own, and the ladder signals the group.

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

use async_process::Child;
use nix::errno::Errno;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

use crate::{Error, SpawnOptions};

/// Puts the agent in a new process group whose id is its own pid, so a signal to the group
/// reaches every process it starts that stays in the group. Safe std, no `pre_exec`.
pub(crate) fn prepare(command: &mut Command, _opts: &SpawnOptions) {
    command.process_group(0);
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
