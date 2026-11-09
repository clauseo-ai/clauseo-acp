//! The seam between listen mode and the OS's local transport: the guard's accept loop
//! (`browser_guard/relay/clients.rs`) and the bridge (`browser_bridge.rs`) see only a
//! [`Listener`] and its [`Connection`]s, so another transport (a named pipe, say) is one more
//! implementation here and nothing in the relay, the lifecycle or the bridge changes.
//!
//! Two transports. Unix sockets on macOS and Linux (`unix.rs`): the socket made fresh with mode
//! 0600 and removed at exit, the peer check ([`Connection::peer_is`]) and the process identity it
//! compares ([`Identity`]). On Windows a named pipe (`windows.rs`) with a DACL for the agent's
//! account, a Low label, and a peer check on each end. `checks.rs` holds the pipe options'
//! checks, which run on every OS.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::time::Duration;

/// One connection between the bridge and the guard.
pub(crate) trait Connection: Read + Write + Send + Sized + 'static {
    /// A second handle on the same connection, for another thread.
    fn try_clone(&self) -> io::Result<Self>;
    /// Ends one direction or both. Ending both wakes a read or a write blocked on another handle.
    fn shutdown(&self, how: Shutdown) -> io::Result<()>;
    /// Bounds each later write; `None` for no bound.
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    /// Bounds each later read; `None` for no bound.
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    /// The peer check, run by both ends: whether the process at the other end is `peer`. The
    /// guard takes any process of its own user ([`Peer::SameUser`]); the bridge takes only the
    /// guard the host started ([`Peer::Process`]), so a process of the same user that squats the
    /// path, or reuses the guard's pid after it, is no guard. On a pipe each end carries what it
    /// expects and `peer` is not read: the guard's end asks whether the client runs as the
    /// `--client-sid` user, the bridge's whether the pipe is owned by the `--server-sid` user (a
    /// squatted pipe is its squatter's). `false` when the OS cannot say.
    fn peer_is(&self, peer: Peer) -> bool;
}

/// What the peer check requires of the process at the other end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Peer {
    /// Any process of this process's own user.
    SameUser,
    /// This one process, running as this process's own user.
    Process(Identity),
}

/// One process, told apart from any later one that reuses its pid: its pid and a number the
/// kernel gives that process alone. On macOS the pidversion from its audit token, which also
/// changes at each exec; on Linux its start time (`/proc/<pid>/stat`, field 22). Written and read
/// as `<pid>/<version>`: the guard prints its own (`browser_guard.rs`), the host passes it to the
/// bridge as `--guard-pid` and `--guard-pidversion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) pid: u32,
    pub(crate) version: u64,
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.pid, self.version)
    }
}

/// Where the guard accepts its clients.
pub(crate) trait Listener: Send + 'static {
    type Connection: Connection;
    /// Waits for the next connection.
    fn accept(&self) -> io::Result<Self::Connection>;
}

pub(crate) mod checks;
#[cfg(all(test, unix))]
pub(crate) mod fake;
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;
