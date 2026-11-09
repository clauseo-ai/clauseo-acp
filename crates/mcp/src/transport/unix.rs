//! Unix only: the Unix-socket transport ([`Listener`] and [`Connection`]) and the socket the
//! guard's `--listen` names, which it serves instead of stdio when the host runs it outside the
//! agent's jail (the agent reaches it through `clauseo-mcp browser-bridge`).
//!
//! [`Socket::make`] first opens and checks the socket's folder one component at a time from `/`,
//! by descriptor (`folder.rs`): no link on the way, every folder owned by root or the guard's
//! user and open to no other writer unless sticky, the last one mode 0700 and the guard's. It
//! creates the socket fresh in that folder, owner-only (mode 0600), and refuses a name that exists
//! already, whatever it is: it never unlinks, replaces or follows anything there. The bind, the
//! look after it and the unlink at the drop all name the socket relative to the folder's
//! descriptor, so a path swapped after the walk moves none of them; within the folder only a
//! process of the guard's own user can swap the file, and the drop removes it only if its device
//! and inode are still the ones the bind made. A guard killed outright leaves the file: the host
//! (J4) cleans a killed guard's socket before it starts another.
//!
//! [`peer_is`] is the peer check: the peer's effective uid must be this process's own
//! (`SO_PEERCRED` on Linux, `getpeereid` on macOS) and, for the bridge, the peer must be the very
//! guard process the host started, by its [`Identity`] (`identity.rs`), so neither a squatter
//! nor a process that later reuses the guard's pid passes.
//!
//! The listener accepts one connection at a time under [`hold_accepts`]'s lock, which every
//! spawn of the guard's takes too: macOS has no `accept4`, so std marks an accepted descriptor
//! close-on-exec only just after `accept`, and a child spawned in between would inherit the
//! connection. The listener waits in `poll` outside the lock, then accepts without blocking.
//! No message here names a path.

use std::ffi::OsString;
use std::io;
use std::net::Shutdown;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use nix::fcntl::AtFlags;
use nix::sys::stat::{FileStat, Mode, SFlag, fstatat};

use super::{Connection, Identity, Listener, Peer};
use crate::browser_guard::note;
use crate::write;

mod folder;
pub(crate) mod identity;

/// The refusal when the path exists already.
pub(crate) const EXISTS: &str =
    "the --listen socket path exists already; the guard makes it fresh and never replaces it";

/// Held by each accept and each spawn of the guard's ([`hold_accepts`]).
static ACCEPTS: Mutex<()> = Mutex::new(());

/// Keeps the listener from accepting while the caller spawns a child, so no accepted connection
/// is inherited before it is marked close-on-exec (macOS). Every spawn of the guard's holds it.
pub(crate) fn hold_accepts() -> MutexGuard<'static, ()> {
    ACCEPTS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The listen socket, removed on drop if its name in the folder still names it.
pub(crate) struct Socket {
    listener: Option<UnixListener>,
    /// The checked folder, open for the guard's life.
    folder: OwnedFd,
    name: OsString,
    /// The socket's device and inode, as the bind made it.
    made: (nix::libc::dev_t, nix::libc::ino_t),
}

impl Socket {
    /// Opens and checks the folder, then creates and binds the socket in it, mode 0600. Call it
    /// while the guard has one thread: it sets the process's umask and working folder around the
    /// bind. `Err` with the reason, which names no path.
    pub(crate) fn make(path: &Path) -> Result<Socket, String> {
        use nix::sys::stat::umask;
        let (folder, name) = folder::open_checked(path)?;
        if look(&folder, &name).is_ok() {
            return Err(EXISTS.to_owned());
        }
        // 0o177, which leaves 0600; named bits, as `mode_t` is `u16` on macOS, `u32` on Linux.
        let old = umask(Mode::S_IXUSR | Mode::S_IRWXG | Mode::S_IRWXO);
        let bound = write::bind_unix_socket_in(&folder, &name);
        umask(old);
        let listener = bound.map_err(|error| match error.kind() {
            io::ErrorKind::AddrInUse | io::ErrorKind::AlreadyExists => EXISTS.to_owned(),
            _ => format!("cannot listen on the --listen socket: {error}"),
        })?;
        let stat = look(&folder, &name)
            .map_err(|errno| format!("cannot read the --listen socket: {errno}"))?;
        let socket = Socket {
            listener: Some(listener),
            folder,
            name,
            made: (stat.st_dev, stat.st_ino),
        };
        let kind = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
        if kind != SFlag::S_IFSOCK {
            return Err("the --listen socket path is not the socket the guard made".to_owned());
        }
        // Dropping `socket` removes the file the guard made.
        if Mode::from_bits_truncate(stat.st_mode) != Mode::S_IRUSR | Mode::S_IWUSR {
            return Err("the --listen socket's mode is not 600".to_owned());
        }
        Ok(socket)
    }

    /// The listener, once, for the relay (`browser_guard/relay/clients.rs`).
    pub(crate) fn take_listener(&mut self) -> Option<UnixListener> {
        self.listener.take()
    }

    /// Whether the socket's name still names the socket this guard made.
    fn still_ours(&self) -> bool {
        look(&self.folder, &self.name).is_ok_and(|stat| (stat.st_dev, stat.st_ino) == self.made)
    }
}

/// `fstatat` of `name` in `folder`, not following a link.
fn look(folder: &OwnedFd, name: &OsString) -> nix::Result<FileStat> {
    fstatat(folder, name.as_os_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
}

impl Drop for Socket {
    fn drop(&mut self) {
        if !self.still_ours() {
            note("the --listen socket was replaced or removed; left it alone");
            return;
        }
        if let Err(error) = write::remove_in(&self.folder, &self.name) {
            note(&format!("cannot remove the --listen socket: {error}"));
        }
    }
}

impl Connection for UnixStream {
    fn try_clone(&self) -> io::Result<Self> {
        UnixStream::try_clone(self)
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        UnixStream::shutdown(self, how)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        UnixStream::set_write_timeout(self, timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        UnixStream::set_read_timeout(self, timeout)
    }

    fn peer_is(&self, peer: Peer) -> bool {
        peer_is(self, peer)
    }
}

impl Listener for UnixListener {
    type Connection = UnixStream;

    /// Waits in `poll` until a connection is there, then accepts it under [`hold_accepts`]
    /// without blocking; another process may have taken it meanwhile, and the wait goes on.
    fn accept(&self) -> io::Result<UnixStream> {
        use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
        self.set_nonblocking(true)?;
        loop {
            let mut ready = [PollFd::new(self.as_fd(), PollFlags::POLLIN)];
            match poll(&mut ready, PollTimeout::NONE) {
                Ok(_) | Err(nix::errno::Errno::EINTR) => {}
                Err(errno) => return Err(errno.into()),
            }
            let accepted = {
                let _spawns = hold_accepts();
                UnixListener::accept(self)
            };
            match accepted {
                // A listener's non-blocking flag is inherited on macOS; the client blocks.
                Ok((stream, _)) => {
                    stream.set_nonblocking(false)?;
                    return Ok(stream);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
    }
}

/// The peer check: whether the process on `stream`'s other end runs as this process's own
/// effective user and, for [`Peer::Process`], is that very process ([`Identity`]). `false`
/// when the kernel cannot say.
pub(crate) fn peer_is(stream: &UnixStream, peer: Peer) -> bool {
    let same_user = peer_uid(stream).is_some_and(|uid| uid == nix::unistd::geteuid().as_raw());
    match peer {
        Peer::SameUser => same_user,
        Peer::Process(guard) => same_user && identity::of_peer(stream) == Some(guard),
    }
}

/// This process's identity, which the guard prints for the host (`browser_guard.rs`).
pub(crate) fn own_identity() -> Option<Identity> {
    identity::own()
}

/// The effective uid of the process that connected, from `SO_PEERCRED`.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    getsockopt(stream, PeerCredentials)
        .ok()
        .map(|credentials| credentials.uid())
}

/// The effective uid of the process that connected, from `getpeereid`.
#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    nix::unistd::getpeereid(stream)
        .ok()
        .map(|(uid, _)| uid.as_raw())
}

/// Elsewhere the guard cannot check its peer, so it serves no one.
#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn peer_uid(_stream: &UnixStream) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests;
