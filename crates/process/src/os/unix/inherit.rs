//! [`crate::SpawnOptions::pass_fd`]: the child gets each passed descriptor at its fixed number
//! between fork and exec, with `fcntl` and `dup2`. At most 16 descriptors; more fail the start.
#![expect(
    unsafe_code,
    reason = "Command::pre_exec, to put the passed descriptors at their numbers before exec"
)]

use std::io;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;

/// Has `command`'s child get each `(target, fd)` as its descriptor `target`. Each source is
/// first copied above every target (close-on-exec), so a source whose number is another's
/// target is never overwritten; then each copy is `dup2`ed to its target, which clears
/// close-on-exec there. The closure holds the sources, so they stay open until the command
/// drops.
pub(crate) fn pass(command: &mut Command, fds: &[(RawFd, Arc<OwnedFd>)]) {
    if fds.is_empty() {
        return;
    }
    let held: Vec<(RawFd, Arc<OwnedFd>)> = fds.to_vec();
    let floor = held.iter().map(|(target, _)| *target).max().unwrap_or(2) + 1;
    let set = move || {
        let mut copies = [(0, 0); 16];
        if held.len() > copies.len() {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        let count = held.len();
        for (slot, (target, fd)) in copies.iter_mut().zip(&held) {
            // SAFETY: fcntl(2) is async-signal-safe; the source stays open (`held`).
            let copy = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, floor) };
            if copy < 0 {
                return Err(io::Error::last_os_error());
            }
            *slot = (*target, copy);
        }
        for (target, copy) in copies.iter().take(count) {
            // SAFETY: dup2(2) is async-signal-safe; both descriptors are open here.
            if unsafe { libc::dup2(*copy, *target) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    };
    // SAFETY: the closure runs in the forked child before exec, where only async-signal-safe
    // calls are allowed. It calls fcntl(2) and dup2(2) only, writes a fixed array on its own
    // stack, and builds an `io::Error` from a raw OS code, which allocates nothing; it takes
    // no lock and touches no state of the parent's other threads.
    unsafe {
        command.pre_exec(set);
    }
}
