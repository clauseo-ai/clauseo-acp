//! Unix only: a process's [`Identity`], its own and its socket peer's. On Linux the pid with the
//! process's start time, `/proc/<pid>/stat` field 22 (for development and CI); on macOS the pid
//! and pidversion from the process's audit token (`identity/mac.rs`): its own through
//! `task_info(TASK_AUDIT_TOKEN)`, the peer's through `LOCAL_PEERTOKEN`, which the kernel took when
//! the peer listened or connected. A peer whose pid was reused since has another version.

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

/// The identity of the process at `stream`'s other end, `None` when the kernel cannot say.
#[cfg(target_os = "macos")]
pub(crate) fn of_peer(stream: &UnixStream) -> Option<Identity> {
    use nix::sys::socket::{getsockopt, sockopt::LocalPeerToken};
    getsockopt(stream, LocalPeerToken)
        .ok()
        .and_then(mac::identity)
}

/// The identity of the process at `stream`'s other end: its pid from `SO_PEERCRED`, then that
/// pid's start time now. A peer gone since and its pid reused reads with another start time.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn of_peer(stream: &UnixStream) -> Option<Identity> {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    let pid = getsockopt(stream, PeerCredentials).ok()?.pid();
    let pid = u32::try_from(pid).ok()?;
    start_time(pid).map(|version| Identity { pid, version })
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
