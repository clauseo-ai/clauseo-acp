//! Unix only: the Unix-socket transport ([`Listener`] and [`Connection`]) and the socket the
//! guard's `--listen` names, which it serves instead of stdio when the host runs it outside the
//! agent's jail (the agent reaches it through `clauseo-mcp browser-bridge`).
//!
//! [`Socket::make`] first checks the socket's folder as `browser_guard/socket.rs` checks
//! agent-browser's: a real folder with mode exactly 0700, owned by the guard's own user, and no
//! link anywhere in the path (on macOS, then, the host passes `/private/tmp/...`, not `/tmp/...`).
//! It creates the socket fresh, owner-only (mode 0600), and refuses a path that exists already,
//! whatever it is: it never unlinks, replaces or follows anything there. Binding does not follow
//! a link, and a path made between the look and the bind fails the bind. The socket's device and
//! inode are noted at once, and when the [`Socket`] drops (the guard's exit, or a later step of
//! its start failing) the file is removed only if the path still names that same socket. A
//! guard killed outright leaves the file; the folder is the host's.
//!
//! The look after the bind and the look before the unlink each race a swap of the file, but
//! only a process of the guard's own user can swap anything in a 0700 folder it owns, and that
//! user is the one the guard serves anyway.
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
        check_folder(path)?;
        if std::fs::symlink_metadata(path).is_ok() {
            return Err(EXISTS.to_owned());
        }
        // 0o177, which leaves [`MODE`]; named bits, as `mode_t` is `u16` on macOS, `u32` on Linux.
        let old = umask(Mode::S_IXUSR | Mode::S_IRWXG | Mode::S_IRWXO);
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

/// `Err` with the reason unless `socket`'s folder is a real folder with mode exactly 0700, owned
/// by the guard's effective user, and nothing on the way to it, the folder included, is a link.
/// The reasons name no path.
fn check_folder(socket: &Path) -> Result<(), String> {
    let folder = socket
        .parent()
        .filter(|folder| socket.is_absolute() && !folder.as_os_str().is_empty())
        .ok_or("the --listen socket must be an absolute path inside a folder")?;
    for step in folder.ancestors() {
        let metadata = std::fs::symlink_metadata(step)
            .map_err(|error| format!("cannot read the --listen socket's folder: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err("the --listen socket's path goes through a link".to_owned());
        }
    }
    let metadata = std::fs::symlink_metadata(folder)
        .map_err(|error| format!("cannot read the --listen socket's folder: {error}"))?;
    folder_refusal(
        metadata.is_dir(),
        metadata.mode() & 0o7777,
        metadata.uid(),
        nix::unistd::geteuid().as_raw(),
    )
    .map_or(Ok(()), |reason| {
        Err(format!("the --listen socket's folder: {reason}"))
    })
}

/// Why a folder that is no link, with these facts, may not hold the socket; `None` if it may.
fn folder_refusal(is_dir: bool, mode: u32, uid: u32, own_uid: u32) -> Option<String> {
    if !is_dir {
        return Some("it is not a folder".to_owned());
    }
    if mode != 0o700 {
        return Some(format!("its mode is {mode:o}, not 700"));
    }
    (uid != own_uid).then(|| format!("its owner is uid {uid}, not the guard's uid {own_uid}"))
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

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        UnixStream::set_read_timeout(self, timeout)
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
    use std::path::PathBuf;

    use super::{EXISTS, Socket, folder_refusal, same_user};

    /// A fresh folder with mode 0700 under `/tmp`, by a path without links (on macOS `/tmp` is a
    /// link to `/private/tmp`).
    fn dir(test: &str) -> PathBuf {
        let temp = PathBuf::from(if cfg!(target_os = "macos") {
            "/private/tmp"
        } else {
            "/tmp"
        });
        let dir = temp.join(format!("clauseo-listen-{test}-{}", std::process::id()));
        drop(crate::write::remove_dir_all(&dir));
        crate::write::create_dir_all(&dir).unwrap();
        crate::write::set_mode(&dir, 0o700).unwrap();
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

    /// Another socket at the path by the drop (another inode): the guard leaves it.
    #[test]
    fn a_socket_replaced_before_the_drop_is_left_alone() {
        let dir = dir("replaced");
        let path = dir.join("g.sock");
        let socket = Socket::make(&path).unwrap();
        let ours = std::fs::symlink_metadata(&path).unwrap().ino();
        crate::write::remove_file(&path).unwrap();
        let theirs = crate::write::bind_unix_socket(&path).unwrap();
        let replaced = std::fs::symlink_metadata(&path).unwrap();
        assert!(replaced.file_type().is_socket());
        assert_ne!(replaced.ino(), ours, "the same inode came back");
        drop(socket);
        let left = std::fs::symlink_metadata(&path).expect("the other socket was removed");
        assert_eq!(left.ino(), replaced.ino());
        drop(theirs);
        crate::write::remove_dir_all(&dir).unwrap();
    }

    /// A folder that is open to others, a link, or reached through a link: refused, and nothing
    /// is made in it.
    #[test]
    fn the_folder_must_be_private_and_reached_without_links() {
        let dir = dir("folder");
        let open = dir.join("open");
        crate::write::create_dir(&open).unwrap();
        crate::write::set_mode(&open, 0o755).unwrap();
        let error = Socket::make(&open.join("g.sock")).err().unwrap();
        assert!(error.contains("its mode is 755, not 700"), "{error}");
        let private = dir.join("private");
        crate::write::create_dir(&private).unwrap();
        crate::write::set_mode(&private, 0o700).unwrap();
        let link = dir.join("link");
        crate::write::symlink(&private, &link).unwrap();
        let error = Socket::make(&link.join("g.sock")).err().unwrap();
        assert!(error.contains("goes through a link"), "{error}");
        let inner = private.join("inner");
        crate::write::create_dir(&inner).unwrap();
        crate::write::set_mode(&inner, 0o700).unwrap();
        let error = Socket::make(&link.join("inner").join("g.sock"))
            .err()
            .unwrap();
        assert!(error.contains("goes through a link"), "{error}");
        assert!(!error.contains(dir.to_str().unwrap()), "a path in {error}");
        for folder in [&open, &private, &inner] {
            assert!(
                std::fs::symlink_metadata(folder.join("g.sock")).is_err(),
                "a socket was made"
            );
        }
        drop(Socket::make(&inner.join("g.sock")).unwrap());
        crate::write::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_private_folder_of_the_guards_own_user_passes() {
        assert_eq!(folder_refusal(true, 0o700, 501, 501), None);
        assert_eq!(
            folder_refusal(false, 0o700, 501, 501).as_deref(),
            Some("it is not a folder")
        );
        assert_eq!(
            folder_refusal(true, 0o1700, 501, 501).as_deref(),
            Some("its mode is 1700, not 700")
        );
        assert_eq!(
            folder_refusal(true, 0o700, 0, 501).as_deref(),
            Some("its owner is uid 0, not the guard's uid 501")
        );
    }
}
