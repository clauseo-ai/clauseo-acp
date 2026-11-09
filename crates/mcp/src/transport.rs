//! The seam between listen mode and the OS's local transport: the guard's accept loop
//! (`browser_guard/relay/clients.rs`) and the bridge (`browser_bridge.rs`) see only a
//! [`Listener`] and its [`Connection`]s, so another transport (a named pipe, say) is one more
//! implementation here and nothing in the relay, the lifecycle or the bridge changes.
//!
//! Unix sockets are the one transport today (`unix.rs`): the socket made fresh with mode 0600 and
//! removed at exit, and the peer check.

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
    /// The peer check: whether the process at the other end runs as this process's own user.
    /// `false` when the OS cannot say.
    fn same_user(&self) -> bool;
}

/// Where the guard accepts its clients.
pub(crate) trait Listener: Send + 'static {
    type Connection: Connection;
    /// Waits for the next connection.
    fn accept(&self) -> io::Result<Self::Connection>;
}

#[cfg(unix)]
pub(crate) mod unix;
