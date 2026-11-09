//! Unix only: a process's [`Identity`], its own and its socket peer's. On Linux the pid with the
//! process's start time, `/proc/<pid>/stat` field 22 (for development and CI); on macOS the pid
//! and pidversion from the process's audit token (`identity/mac.rs`): its own through
//! `task_info(TASK_AUDIT_TOKEN)`, the peer's through `LOCAL_PEERTOKEN`.
//!
//! The peer's identity is read live from the peer process at each check, never kept from the
//! connect: `LOCAL_PEERTOKEN` asks the kernel for the peer's audit token at the call, and on Linux
//! the start time is read from `/proc` at the call. A peer whose pid was reused since has another
//! version; a peer that has gone, or a lookup that fails, reads as `None`, so the check fails
//! closed. On Linux the pid comes from the peer's pidfd (`SO_PEERPIDFD`, Linux 6.5 on) when the
//! kernel has it, and the start time is taken only while that pidfd shows the peer still running,
//! so a pid reused between the two reads cannot pass; an older kernel (`ENOPROTOOPT`, `EINVAL`)
//! falls back to `SO_PEERCRED`'s pid and its start time, which a pid reused within that window
//! could match.

use std::os::unix::net::UnixStream;

use super::super::Identity;

/// This process's identity, `None` when the kernel cannot say.
pub(crate) fn own() -> Option<Identity> {
    #[cfg(target_os = "macos")]
    {
        mac::own_token().and_then(mac::identity)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let pid = std::process::id();
        start_time(pid).map(|version| Identity { pid, version })
    }
}

/// How many threads this process has, `None` when the kernel cannot say: `Socket::make` checks it
/// is one (`unix.rs`). It sits here with the other reads of this process's own task.
pub(crate) fn threads() -> Option<usize> {
    #[cfg(target_os = "macos")]
    {
        mac::threads()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        status
            .lines()
            .find_map(|line| line.strip_prefix("Threads:"))?
            .trim()
            .parse()
            .ok()
    }
}

/// The identity of the process at `stream`'s other end, `None` when the kernel cannot say.
#[cfg(target_os = "macos")]
pub(crate) fn of_peer(stream: &UnixStream) -> Option<Identity> {
    use nix::sys::socket::{getsockopt, sockopt::LocalPeerToken};
    getsockopt(stream, LocalPeerToken)
        .ok()
        .and_then(mac::identity)
}

/// The identity of the process at `stream`'s other end: its pid from `SO_PEERCRED`, then that
/// pid's start time now, checked against the peer's pidfd where the kernel gives one. A peer gone
/// since and its pid reused reads with another start time.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn of_peer(stream: &UnixStream) -> Option<Identity> {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    let pidfd = peer_pidfd(stream)?;
    let pid = getsockopt(stream, PeerCredentials).ok()?.pid();
    let pid = u32::try_from(pid).ok()?;
    let version = start_time(pid)?;
    // The pidfd and `SO_PEERCRED` name the same process, the peer; while it runs, the pid is
    // still its own, so the start time just read is the peer's.
    if let Pidfd::Got(pidfd) = pidfd
        && !running(&pidfd)
    {
        return None;
    }
    Some(Identity { pid, version })
}

/// What `SO_PEERPIDFD` gave.
#[cfg(any(target_os = "linux", target_os = "android"))]
enum Pidfd {
    /// The peer's pidfd.
    Got(std::os::fd::OwnedFd),
    /// A kernel without `SO_PEERPIDFD` (before Linux 6.5).
    Unsupported,
}

/// The peer's pidfd; `None` when the kernel has `SO_PEERPIDFD` and refuses.
#[cfg(target_os = "linux")]
fn peer_pidfd(stream: &UnixStream) -> Option<Pidfd> {
    use nix::errno::Errno;
    use nix::sys::socket::{getsockopt, sockopt::PeerPidfd};
    match getsockopt(stream, PeerPidfd) {
        Ok(pidfd) => Some(Pidfd::Got(pidfd)),
        Err(Errno::ENOPROTOOPT | Errno::EINVAL) => Some(Pidfd::Unsupported),
        Err(_) => None,
    }
}

/// Android: no pidfd is asked for.
#[cfg(target_os = "android")]
fn peer_pidfd(_stream: &UnixStream) -> Option<Pidfd> {
    Some(Pidfd::Unsupported)
}

/// Whether the process `pidfd` names still runs: a pidfd polls readable once it has exited.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn running(pidfd: &std::os::fd::OwnedFd) -> bool {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use std::os::fd::AsFd;
    let mut fds = [PollFd::new(pidfd.as_fd(), PollFlags::POLLIN)];
    poll(&mut fds, PollTimeout::ZERO).is_ok_and(|ready| ready == 0)
}

/// Elsewhere no peer's identity is known, so no process passes.
#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
pub(crate) fn of_peer(_stream: &UnixStream) -> Option<Identity> {
    None
}

/// The start time of process `pid`: field 22 of `/proc/<pid>/stat`, read after the last `)`, as
/// the command name before it may hold spaces and parentheses.
#[cfg(not(target_os = "macos"))]
fn start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    start_time_in(&stat)
}

/// Field 22 of a `/proc/<pid>/stat` line.
#[cfg(not(target_os = "macos"))]
fn start_time_in(stat: &str) -> Option<u64> {
    let (_, rest) = stat.rsplit_once(") ")?;
    // `rest` starts at field 3 (the state), so field 22 is its 20th.
    rest.split_ascii_whitespace().nth(19)?.parse().ok()
}

#[cfg(target_os = "macos")]
mod mac;

#[cfg(test)]
mod thread_tests {
    use super::threads;

    /// The count is this process's: the harness's thread and this test's at least, and one more
    /// while another thread lives.
    #[test]
    fn the_thread_count_counts_this_processes_threads() {
        let before = threads().expect("this process's thread count");
        assert!(
            before >= 2,
            "{before}: the harness runs each test on a thread of its own"
        );
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let (started, running) = std::sync::mpsc::channel();
        let other = std::thread::spawn(move || {
            started.send(threads()).unwrap();
            // An error only says the test has dropped its end, which is the signal too.
            if stopped.recv().is_err() {}
        });
        let during = running.recv().unwrap().unwrap();
        drop(stop);
        other.join().unwrap();
        // Other tests' threads may come and go in the same process (not under nextest).
        assert!(during > before || during >= 3, "{before} then {during}");
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::start_time_in;

    #[test]
    fn the_start_time_is_field_22_whatever_the_name_holds() {
        let fields: Vec<String> = (3..=30).map(|n| n.to_string()).collect();
        let line = format!("42 (a ) (b) c) {}", fields.join(" "));
        assert_eq!(start_time_in(&line), Some(22));
        assert_eq!(start_time_in("42 (short) S 1 2"), None);
    }
}
