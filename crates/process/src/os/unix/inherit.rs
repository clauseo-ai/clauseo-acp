//! [`crate::SpawnOptions::pass_fd`] and [`crate::SpawnOptions::close_other_fds`]: between fork
//! and exec the child gets each passed descriptor at its fixed number, with `fcntl` and `dup2`
//! (at most 16; more fail the start), then closes every other descriptor of 3 or above that
//! would stay open across exec.
#![expect(
    unsafe_code,
    reason = "Command::pre_exec, to set the child's descriptors before exec"
)]

use std::io;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;

/// The most descriptors one start may pass.
const MAX_PASSED: usize = 16;

/// Has `command`'s child get each `(target, fd)` as its descriptor `target`, and, with
/// `close_others`, close each other descriptor of 3 or above that is not close-on-exec. Each
/// source is first copied above every target (close-on-exec), so a source whose number is
/// another's target is never overwritten; then each copy is `dup2`ed to its target, which clears
/// close-on-exec there. Close-on-exec descriptors are left to the exec, so the pipe std reports
/// a failed exec on stays open until then. The closure holds the sources, so they stay open
/// until the command drops.
pub(crate) fn pass(command: &mut Command, fds: &[(RawFd, Arc<OwnedFd>)], close_others: bool) {
    if fds.is_empty() && !close_others {
        return;
    }
    let held: Vec<(RawFd, Arc<OwnedFd>)> = fds.to_vec();
    let floor = held.iter().map(|(target, _)| *target).max().unwrap_or(2) + 1;
    let set = move || {
        if held.len() > MAX_PASSED {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        let mut copies = [(0, 0); MAX_PASSED];
        let mut kept = [-1; MAX_PASSED];
        for ((slot, keep), (target, fd)) in copies.iter_mut().zip(kept.iter_mut()).zip(&held) {
            // SAFETY: fcntl(2) is async-signal-safe; the source stays open (`held`).
            let copy = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, floor) };
            if copy < 0 {
                return Err(io::Error::last_os_error());
            }
            *slot = (*target, copy);
            *keep = *target;
        }
        for (target, copy) in copies.iter().take(held.len()) {
            // SAFETY: dup2(2) is async-signal-safe; both descriptors are open here.
            if unsafe { libc::dup2(*copy, *target) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        if close_others {
            close_inherited(&kept);
        }
        Ok(())
    };
    // SAFETY: the closure runs in the forked child before exec, where only async-signal-safe
    // calls are allowed. It calls fcntl(2), dup2(2), close(2), getdtablesize(2) and, on macOS,
    // proc_pidinfo (one system call, into a buffer on its own stack); it writes fixed arrays on
    // its own stack and builds an `io::Error` from a raw OS code, which allocates nothing; it
    // takes no lock and touches no state of the parent's other threads.
    unsafe {
        command.pre_exec(set);
    }
}

/// Closes each descriptor of 3 or above that is open across exec and not in `kept`.
fn close_inherited(kept: &[RawFd]) {
    let close = |fd: RawFd| {
        if fd < 3 || kept.contains(&fd) {
            return;
        }
        // SAFETY: fcntl(2) and close(2) are async-signal-safe; a closed number answers EBADF.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                libc::close(fd);
            }
        }
    };
    #[cfg(target_os = "macos")]
    if listed(&close).is_some() {
        return;
    }
    // SAFETY: getdtablesize(2) is async-signal-safe.
    let size = unsafe { libc::getdtablesize() };
    for fd in 3..size {
        close(fd);
    }
}

/// Runs `each` on every descriptor the kernel lists for this process, when the list fits in a
/// buffer of 1024; `None` when it does not, or the call failed.
#[cfg(target_os = "macos")]
fn listed(each: &dyn Fn(RawFd)) -> Option<()> {
    const SLOTS: usize = 1024;
    let mut list = [libc::proc_fdinfo {
        proc_fd: 0,
        proc_fdtype: 0,
    }; SLOTS];
    let size = i32::try_from(size_of_val(&list)).ok()?;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `list`, which outlives the call;
    // getpid(2) always succeeds.
    let got = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            list.as_mut_ptr().cast(),
            size,
        )
    };
    // A full buffer may have been cut short.
    if got <= 0 || got >= size {
        return None;
    }
    let count = usize::try_from(got).ok()? / size_of::<libc::proc_fdinfo>();
    for info in list.iter().take(count) {
        each(info.proc_fd);
    }
    Some(())
}
