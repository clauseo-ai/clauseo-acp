//! [`crate::SpawnOptions::end_with_parent`] on Linux: the child sets SIGKILL as its
//! parent-death signal (`PR_SET_PDEATHSIG`) between fork and exec, the one `pre_exec` of the
//! crate. The kernel sends it when the thread that forked the child ends, which a crash, a
//! kill or an exit that skips every drop all do. The child then checks that its parent is still
//! the process that started it: had that process died before the signal was set, the child was
//! already reparented and the signal would never come, so the start fails instead.
#![expect(
    unsafe_code,
    reason = "Command::pre_exec, to set the child's parent-death signal before it runs"
)]

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::errno::Errno;
use nix::sys::prctl;
use nix::sys::signal::Signal;
use nix::unistd::{getpid, getppid};

/// Has `command`'s child set SIGKILL as its parent-death signal before it execs.
pub(crate) fn end_with_parent(command: &mut Command) {
    let parent = getpid();
    let set = move || {
        prctl::set_pdeathsig(Signal::SIGKILL).map_err(io::Error::from)?;
        if getppid() == parent {
            Ok(())
        } else {
            // The parent is gone already: the signal would never come.
            Err(io::Error::from(Errno::ESRCH))
        }
    };
    // SAFETY: the closure runs in the forked child before exec, where only async-signal-safe
    // calls are allowed. It makes two system calls, prctl(2) and getppid(2), both
    // async-signal-safe, and builds an `io::Error` from a raw OS code, which allocates nothing;
    // it takes no lock and touches no state of the parent's other threads.
    unsafe {
        command.pre_exec(set);
    }
}
