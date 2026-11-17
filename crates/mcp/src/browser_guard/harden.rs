//! The guard's own process, hidden from other processes of the user before any secret exists
//! (route 2, decision record 0060). Linux only.
//!
//! Linux: [`not_dumpable`] sets `PR_SET_DUMPABLE` to 0, which makes `/proc/<guard>/` root's, so
//! a same-user process can read neither the guard's `environ` and `mem` nor its fds, and cannot
//! attach to it. A child's exec makes it dumpable again, so agent-browser's environment (the
//! relay's URL) stays readable: the relay's peer check (`devtools/peer.rs`) is what keeps other
//! processes out. A `/proc/<guard>/mem` opened before the call keeps working, so it is the
//! guard's first act; Linux is for development and CI only. Any failure refuses the browser: the
//! guard exits before it starts anything.
//!
//! macOS: nothing to set. `task_for_pid` on another process needs an entitlement or root, so a
//! same-user process cannot read the guard's memory; it can read a process's arguments and
//! environment (`KERN_PROCARGS2`, `ps eww`) unless its own Seatbelt profile denies
//! `process-info`, so there too the peer check, not the secret alone, is the boundary.
//!
//! Windows: nothing either. The guard and its children run with the user's default process
//! DACL, as the agents' own processes do; Clauseo does not try to be safer than the agent's own
//! mode. A protected DACL on the guard was tried and removed: it denied the process its own
//! `PROCESS_DUP_HANDLE`, which `WSADuplicateSocketW` (std's `TcpStream::try_clone`) needs
//! (decision record 0060, revision of 2026-11-17).

/// Linux: no core dumps, and `/proc/<guard>/` and `ptrace` closed to the user's other processes.
#[cfg(target_os = "linux")]
pub(crate) fn not_dumpable() -> std::io::Result<()> {
    nix::sys::prctl::set_dumpable(false).map_err(std::io::Error::from)
}
