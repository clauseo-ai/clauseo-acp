//! [`crate::SpawnOptions::own_session`] and [`crate::SpawnOptions::max_processes`]: between fork
//! and exec the child calls `setsid` (so it leads a new session and a new process group whose id
//! is its pid, as `process_group(0)` would give) and sets `RLIMIT_NPROC`, soft and hard.
#![expect(
    unsafe_code,
    reason = "Command::pre_exec, to call setsid and setrlimit in the child before exec"
)]

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Has `command`'s child call `setsid` when `session`, then set its `RLIMIT_NPROC` to
/// `max_processes`, soft and hard, when set; a failure of either fails the start. Nothing when
/// neither is asked.
pub(crate) fn prepare(command: &mut Command, session: bool, max_processes: Option<u64>) {
    if !session && max_processes.is_none() {
        return;
    }
    let limit = max_processes.map(|n| libc::rlimit {
        rlim_cur: n,
        rlim_max: n,
    });
    let set = move || {
        // SAFETY: setsid(2) is async-signal-safe and takes no argument.
        if session && unsafe { libc::setsid() } < 0 {
            return Err(io::Error::last_os_error());
        }
        if let Some(limit) = &limit {
            // SAFETY: setrlimit(2) is async-signal-safe; `limit` is a plain struct the closure
            // owns, read and not kept by the call.
            if unsafe { libc::setrlimit(libc::RLIMIT_NPROC, limit) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    };
    // SAFETY: the closure runs in the forked child before exec, where only async-signal-safe
    // calls are allowed. It calls setsid(2) and setrlimit(2) only, reads a struct it owns, and
    // builds an `io::Error` from errno, which allocates nothing; it takes no lock and touches no
    // state of the parent's other threads.
    unsafe {
        command.pre_exec(set);
    }
}
