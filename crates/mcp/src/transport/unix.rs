//! Unix only: the Unix-socket transport ([`Listener`] and [`Connection`]) and the socket the
//! guard's `--listen` names, which it serves instead of stdio when the host runs it outside the
//! agent's jail (the agent reaches it through `clauseo-mcp browser-bridge`).
//!
//! [`Socket::make`] creates the socket fresh, owner-only (mode 0600), and refuses a path that
//! exists already, whatever it is: it never unlinks, replaces or follows anything there. Binding
//! does not follow a link, and a path made between the look and the bind fails the bind. The
//! socket's device and inode are noted at once, and when the [`Socket`] drops (the guard's exit,
//! or a later step of its start failing) the file is removed only if the path still names that
//! same socket. A guard killed outright leaves the file; the folder is the host's.
//!
//! [`same_user`] is the peer check: a client whose effective uid is not this process's own is
//! closed (`SO_PEERCRED` on Linux, `getpeereid` on macOS). No message here names the path.

use std::io;
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Connection, Listener};
use crate::browser_guard::note;
use crate::write;

/// The socket file's mode.
const MODE: u32 = 0o600;
/// The refusal when the path exists already.
pub(crate) const EXISTS: &str =
    "the --listen socket path exists already; the guard makes it fresh and never replaces it";

/// The listen socket, removed on drop if the path still names it.
pub(crate) struct Socket {
    listener: Option<UnixListener>,
    path: PathBuf,
    dev: u64,
    ino: u64,
}

impl Socket {
    /// Creates and binds the socket at `path`, mode 0600. Call it while the guard has one thread:
    /// it sets the process's umask around the bind, so the file is never more open than 0600.
    /// `Err` with the reason, which names no path.
    pub(crate) fn make(path: &Path) -> Result<Socket, String> {
        use nix::sys::stat::{Mode, umask};
        if std::fs::symlink_metadata(path).is_ok() {
            return Err(EXISTS.to_owned());
        }
        let old = umask(Mode::from_bits_truncate(0o777 & !MODE));
        let bound = write::bind_unix_socket(path);
        umask(old);
        let listener = bound.map_err(|error| match error.kind() {
            io::ErrorKind::AddrInUse | io::ErrorKind::AlreadyExists => EXISTS.to_owned(),
            _ => format!("cannot listen on the --listen socket: {error}"),
        })?;
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| format!("cannot read the --listen socket: {error}"))?;
        let socket = Socket {
            listener: Some(listener),
            path: path.to_owned(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        };
        if !metadata.file_type().is_socket() {
            return Err("the --listen socket path is not the socket the guard made".to_owned());
        }
        let mode = metadata.mode() & 0o7777;
        if mode != MODE {
            // Dropping `socket` removes the file the guard made.
            return Err(format!(
                "the --listen socket's mode is {mode:o}, not {MODE:o}"
            ));
        }
        Ok(socket)
    }

    /// The listener, once, for the relay (`browser_guard/relay/clients.rs`).
    pub(crate) fn take_listener(&mut self) -> Option<UnixListener> {
        self.listener.take()
    }

    /// Whether `path` still names the socket this guard made.
    fn still_ours(&self) -> bool {
        std::fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.file_type().is_socket()
                && metadata.dev() == self.dev
                && metadata.ino() == self.ino
        })
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        if !self.still_ours() {
            note("the --listen socket was replaced or removed; left the path alone");
            return;
        }
        if let Err(error) = write::remove_file(&self.path) {
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

    fn same_user(&self) -> bool {
        same_user(self)
    }
}

impl Listener for UnixListener {
    type Connection = UnixStream;

    fn accept(&self) -> io::Result<UnixStream> {
        UnixListener::accept(self).map(|(stream, _)| stream)
    }
}

/// The peer check: whether the process on `stream`'s other end runs as this process's own
/// effective user. `false` when the kernel cannot say.
pub(crate) fn same_user(stream: &UnixStream) -> bool {
    peer_uid(stream).is_some_and(|uid| uid == nix::unistd::geteuid().as_raw())
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
mod tests {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::os::unix::net::UnixStream;

    use super::{EXISTS, Socket, same_user};

    fn dir(test: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("clauseo-listen-{test}-{}", std::process::id()));
        drop(crate::write::remove_dir_all(&dir));
        crate::write::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_socket_is_fresh_owner_only_and_removed_on_drop() {
        let dir = dir("fresh");
        let path = dir.join("g.sock");
        let mut socket = Socket::make(&path).unwrap();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(metadata.file_type().is_socket());
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        let listener = socket.take_listener().unwrap();
        let client = UnixStream::connect(&path).unwrap();
        let (served, _) = listener.accept().unwrap();
        assert!(same_user(&served), "the test's own uid");
        drop((client, served));
        drop(socket);
        assert!(
            std::fs::symlink_metadata(&path).is_err(),
            "the socket stayed"
        );
        crate::write::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_path_that_exists_is_refused_and_left_alone() {
        let dir = dir("exists");
        let file = dir.join("file.sock");
        crate::write::write(&file, "kept").unwrap();
        assert_eq!(Socket::make(&file).err().as_deref(), Some(EXISTS));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "kept");
        let link = dir.join("link.sock");
        let target = dir.join("target");
        crate::write::symlink(&target, &link).unwrap();
        assert_eq!(Socket::make(&link).err().as_deref(), Some(EXISTS));
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            std::fs::symlink_metadata(&target).is_err(),
            "the link was followed"
        );
        crate::write::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_socket_replaced_before_the_drop_is_left_alone() {
        let dir = dir("replaced");
        let path = dir.join("g.sock");
        let socket = Socket::make(&path).unwrap();
        crate::write::remove_file(&path).unwrap();
        crate::write::write(&path, "someone else's").unwrap();
        drop(socket);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "someone else's");
        crate::write::remove_dir_all(&dir).unwrap();
    }
}
