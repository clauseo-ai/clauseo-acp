//! The Unix half: the agent leads a process group of its own, and the ladder signals the group.

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

use async_process::Child;
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

    /// Sends SIGTERM to the group.
    pub(crate) fn terminate(&self) {
        self.signal(Signal::SIGTERM);
    }

    /// Sends SIGKILL to the group.
    pub(crate) fn kill(&self) {
        self.signal(Signal::SIGKILL);
    }

    /// An empty group (`ESRCH`) is fine. Any other failure is dropped too: the ladder's next step,
    /// or the kill of the agent's own pid at its end, still ends the agent.
    fn signal(&self, signal: Signal) {
        killpg(self.group, signal).unwrap_or_default();
    }
}
