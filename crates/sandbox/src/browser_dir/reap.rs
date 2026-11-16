//! The daemon reap: before a browser socket folder goes, the agent-browser daemon listening on a
//! socket in it is ended.
//!
//! agent-browser's daemon leaves the agent's process tree (`setsid`, its parent becomes pid 1),
//! so the host's stop of the agent's tree does not reach it, and when the host kills the browser
//! guard nothing alive knows about it: it would live until its idle timeout. The folder's
//! sockets name it. For each `*.sock` in the folder (a socket, not followed through a link) the
//! reap connects, asks the kernel which process listens on the other end (`SO_PEERCRED` on
//! Linux, `LOCAL_PEERPID` on macOS), and ends that process only when every check holds:
//!
//! - the connection is to that socket: its peer address is the socket's path (or names the same
//!   socket inode), so a socket swapped for a link between the listing and the connect is
//!   refused;
//! - its pid is above 1 and not this process's;
//! - its user id (`SO_PEERCRED`, `getpeereid`) is this process's effective one;
//! - its executable's real path (`/proc/<pid>/exe`, libproc's `proc_pidpath`) is the
//!   [`DaemonProgram`] the caller gave, canonical: Clauseo's own agent-browser;
//! - the connection has not hung up since, so the listener still runs (on Linux this is checked
//!   after its pidfd is open, so the pidfd names that listener and no reused pid).
//!
//! The connect never blocks (a non-blocking socket): a listener whose backlog is full is
//! refused, not waited on. At most [`MAX_SOCKETS`] sockets of a folder are looked at.
//!
//! A check that fails leaves the process alone, with one `warn!` line. Otherwise it gets SIGTERM,
//! up to [`TERM_WAIT`] to exit, then SIGKILL and up to [`KILL_WAIT`]; every wait is capped by
//! the time the caller has left. The connection that named the pid stays open until the last
//! signal: no pid is read from a file or kept from an earlier look. On Linux the pidfd comes
//! from the connection itself (`SO_PEERPIDFD`, kernel 6.5 and later), else from `pidfd_open` on
//! the `SO_PEERCRED` pid, and the signals go through it. On macOS there is no pidfd: the open
//! connection pins the socket, not the pid, and the signals go through `kill` by pid (see
//! `macos.rs` for why that pid cannot be reused in between).
//!
//! Windows has no such folder: agent-browser's daemon is in the guard's Job Object there.

use std::io::{self, Read as _};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::io::Errno;
use rustix::net::{AddressFamily, SocketAddrUnix, SocketType};
use rustix::process::Signal;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod other;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use other as os;

/// The ending of a socket file the reap looks at.
const SOCKET_SUFFIX: &str = ".sock";
/// The most sockets of one folder a reap looks at; the rest count as left. A launch's folder
/// holds one daemon socket.
pub(super) const MAX_SOCKETS: usize = 16;
/// How long a daemon gets to exit after SIGTERM.
const TERM_WAIT: Duration = Duration::from_millis(300);
/// How long the reap waits to see a daemon gone after SIGKILL.
const KILL_WAIT: Duration = Duration::from_millis(500);
/// One poll of the connection while waiting for it to hang up.
const STEP: Duration = Duration::from_millis(25);

/// The program whose listeners a reap may end: Clauseo's bundled agent-browser, the one the
/// browser guard runs. Its path is made canonical once, here; a listener's executable must be
/// that path exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonProgram {
    path: PathBuf,
}

impl DaemonProgram {
    /// The program at `path`, resolved to its canonical path (links followed). Fails when
    /// `path` cannot be resolved.
    pub fn new(path: &Path) -> io::Result<DaemonProgram> {
        Ok(DaemonProgram {
            path: dunce::canonicalize(path)?,
        })
    }

    /// The canonical path a listener's executable must have.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// What a reap did with the listeners it found.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Reaped {
    /// Listeners that passed every check and are gone.
    pub ended: usize,
    /// Listeners a check refused (or that could not be identified): left alone, one `warn!`
    /// line each.
    pub refused: usize,
    /// Listeners signalled but still there after the waits, sockets not reached before the time
    /// ran out or past [`MAX_SOCKETS`], and folders that could not be read. A sweep keeps their
    /// folder for next time.
    pub left: usize,
}

impl std::ops::AddAssign for Reaped {
    fn add_assign(&mut self, rhs: Reaped) {
        self.ended += rhs.ended;
        self.refused += rhs.refused;
        self.left += rhs.left;
    }
}

/// A reap's program and time: `left` answers how long the caller's budget has left, on the
/// caller's clock; each wait is capped by it, and a socket met once it is zero is not reached.
pub struct Reap<'a> {
    program: &'a DaemonProgram,
    left: &'a dyn Fn() -> Duration,
}

impl<'a> Reap<'a> {
    /// A reap of `program`'s listeners within the time `left` answers.
    pub fn new(program: &'a DaemonProgram, left: &'a dyn Fn() -> Duration) -> Reap<'a> {
        Reap { program, left }
    }

    /// The time left, capped at `most`.
    fn wait(&self, most: Duration) -> Duration {
        most.min((self.left)())
    }
}

impl std::fmt::Debug for Reap<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reap")
            .field("program", self.program)
            .finish_non_exhaustive()
    }
}

/// What became of one socket's listener.
pub(super) enum Outcome {
    /// Nobody listens (or the listener exited before any signal).
    Nobody,
    /// Ended.
    Ended,
    /// Left alone: a check failed.
    Refused,
    /// Signalled, still there.
    Left,
}

/// Ends the daemons listening on the sockets in `dir`, the folder of ours being removed (see the
/// module's documentation).
pub(super) fn reap_folder(dir: &Path, reap: &Reap<'_>) -> Reaped {
    let mut reaped = Reaped::default();
    let mut looked = 0usize;
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(%error, "browser daemon reap: a socket folder was not read");
            reaped.left += 1;
            return reaped;
        }
    };
    for entry in entries.flatten() {
        let is_socket_name = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.ends_with(SOCKET_SUFFIX));
        // `file_type` does not follow a link.
        if !is_socket_name || !entry.file_type().is_ok_and(|kind| kind.is_socket()) {
            continue;
        }
        if looked >= MAX_SOCKETS || (reap.left)().is_zero() {
            reaped.left += 1;
            continue;
        }
        looked += 1;
        match reap_socket(&entry.path(), reap) {
            Outcome::Nobody => {}
            Outcome::Ended => reaped.ended += 1,
            Outcome::Refused => reaped.refused += 1,
            Outcome::Left => reaped.left += 1,
        }
    }
    reaped
}

/// Connects to the socket `path` without blocking and ends its listener if every check holds.
/// The connection stays open until after the last signal.
pub(super) fn reap_socket(path: &Path, reap: &Reap<'_>) -> Outcome {
    let stream = match connect(path) {
        Ok(Some(stream)) => stream,
        // A socket nobody listens on: nothing to end.
        Ok(None) => return Outcome::Nobody,
        // A listener that would make the connect wait (its backlog full: a wedged daemon), or a
        // connect that failed otherwise: left, so a sweep keeps its folder for the next start.
        Err(error) => {
            tracing::warn!(
                socket = %path.display(),
                %error,
                "browser daemon reap: a socket's listener does not take connections; it is left"
            );
            return Outcome::Left;
        }
    };
    if !peer_is(&stream, path) {
        tracing::warn!(
            socket = %path.display(),
            "browser daemon reap: the connection reached another socket; its listener is left"
        );
        return Outcome::Refused;
    }
    let outcome = end_listener(&stream, path, reap);
    drop(stream);
    outcome
}

/// A non-blocking, close-on-exec connection to the socket `path`: `Ok(None)` when nobody
/// listens (`ECONNREFUSED`, `ENOENT`, `ENOTSOCK`, `EPROTOTYPE`: refused, gone, not a stream
/// socket; or a path too long for a socket address), an error for anything else, among them a
/// listener that would make the connect wait (its backlog is full: `EAGAIN`, `EINPROGRESS`).
fn connect(path: &Path) -> io::Result<Option<UnixStream>> {
    let Ok(address) = SocketAddrUnix::new(path) else {
        return Ok(None);
    };
    let socket = new_socket()?;
    match rustix::net::connect(&socket, &address) {
        Ok(()) => Ok(Some(UnixStream::from(socket))),
        Err(Errno::CONNREFUSED | Errno::NOENT | Errno::NOTSOCK | Errno::PROTOTYPE) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// A Unix stream socket, non-blocking and close-on-exec.
#[cfg(target_os = "linux")]
fn new_socket() -> io::Result<std::os::fd::OwnedFd> {
    use rustix::net::SocketFlags;
    let flags = SocketFlags::CLOEXEC | SocketFlags::NONBLOCK;
    Ok(rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        flags,
        None,
    )?)
}

/// A Unix stream socket, non-blocking and close-on-exec (two calls: no `SOCK_CLOEXEC` here).
#[cfg(not(target_os = "linux"))]
fn new_socket() -> io::Result<std::os::fd::OwnedFd> {
    let socket = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None)?;
    rustix::io::fcntl_setfd(&socket, rustix::io::FdFlags::CLOEXEC)?;
    rustix::io::ioctl_fionbio(&socket, true)?;
    Ok(socket)
}

/// Whether `stream` is connected to the socket `path`: its peer's address is `path`, or names
/// the socket inode `path` names (a listener that bound another spelling of the same path).
fn peer_is(stream: &UnixStream, path: &Path) -> bool {
    let Ok(peer) = stream.peer_addr() else {
        return false;
    };
    let Some(peer) = peer.as_pathname() else {
        return false;
    };
    if peer == path {
        return true;
    }
    match (
        std::fs::symlink_metadata(peer),
        std::fs::symlink_metadata(path),
    ) {
        (Ok(peer), Ok(named)) => {
            peer.file_type().is_socket()
                && named.file_type().is_socket()
                && peer.dev() == named.dev()
                && peer.ino() == named.ino()
        }
        _ => false,
    }
}

/// Identifies, checks and ends the listener at the other end of `stream`.
fn end_listener(stream: &UnixStream, path: &Path, reap: &Reap<'_>) -> Outcome {
    let process = match os::Process::listening_on(stream) {
        Ok(process) => process,
        // The listener exited between the connect and the lookup (`SO_PEERPIDFD` or
        // `pidfd_open` answers `ESRCH`): nobody to end.
        Err(error) if error.raw_os_error() == Some(Errno::SRCH.raw_os_error()) => {
            return Outcome::Nobody;
        }
        Err(error) => {
            tracing::warn!(
                socket = %path.display(),
                %error,
                "browser daemon reap: a socket's listener was not identified; it is left"
            );
            return Outcome::Refused;
        }
    };
    if let Err(why) = check(&process, reap.program) {
        tracing::warn!(
            socket = %path.display(),
            pid = process.pid(),
            why,
            "browser daemon reap: a socket's listener is not Clauseo's agent-browser; it is left"
        );
        return Outcome::Refused;
    }
    // Still the listener the connection named: it has not exited, so its pid is not reused.
    if hung_up(stream, Duration::ZERO) {
        return Outcome::Nobody;
    }
    match process.signal(Signal::TERM) {
        Ok(()) => {}
        Err(error) => return failed_signal(error, &process),
    }
    if process.wait_gone(stream, reap.wait(TERM_WAIT)) {
        return Outcome::Ended;
    }
    // Checked again before SIGKILL: a listener whose identity no longer reads as before has
    // exited.
    if check(&process, reap.program).is_err() || hung_up(stream, Duration::ZERO) {
        return Outcome::Ended;
    }
    if let Err(error) = process.signal(Signal::KILL) {
        return failed_signal(error, &process);
    }
    if process.wait_gone(stream, reap.wait(KILL_WAIT)) {
        Outcome::Ended
    } else {
        tracing::warn!(
            pid = process.pid(),
            "browser daemon reap: agent-browser's daemon was still there after SIGKILL"
        );
        Outcome::Left
    }
}

/// A signal that failed: the process already gone (`ESRCH`) is ended; anything else leaves it.
fn failed_signal(error: Errno, process: &os::Process) -> Outcome {
    if error == Errno::SRCH {
        return Outcome::Ended;
    }
    tracing::warn!(
        pid = process.pid(),
        %error,
        "browser daemon reap: agent-browser's daemon could not be signalled"
    );
    Outcome::Left
}

/// The identity checks (see the module's documentation): `Err` says which failed.
fn check(process: &os::Process, program: &DaemonProgram) -> Result<(), String> {
    check_identity(
        process.pid(),
        process.uid(),
        || process.executable(),
        program,
    )
}

/// [`check`] on a listener's pid, user id and executable (read only once the first two pass).
fn check_identity(
    pid: i32,
    uid: u32,
    executable: impl FnOnce() -> io::Result<PathBuf>,
    program: &DaemonProgram,
) -> Result<(), String> {
    if pid <= 1 || u32::try_from(pid).ok() == Some(std::process::id()) {
        return Err(format!("pid {pid}"));
    }
    let ours = rustix::process::geteuid().as_raw();
    if uid != ours {
        return Err(format!("user id {uid}, not {ours}"));
    }
    match executable() {
        Ok(exe) if exe == program.path() => Ok(()),
        Ok(exe) => Err(format!("executable {}", exe.display())),
        Err(error) => Err(format!("executable not read: {error}")),
    }
}

/// Whether the connection hangs up within `wait`: the listener closed it (it exited). Data the
/// listener sends is read and dropped.
pub(super) fn hung_up(stream: &UnixStream, wait: Duration) -> bool {
    let steps = (wait.as_millis() / STEP.as_millis()).max(1);
    for _ in 0..steps {
        if hung_up_once(stream, STEP.min(wait)) {
            return true;
        }
    }
    false
}

/// One poll of `stream` of at most `wait`.
fn hung_up_once(stream: &UnixStream, wait: Duration) -> bool {
    let mut fds = [PollFd::new(stream, PollFlags::IN)];
    let timeout = Timespec::try_from(wait).ok();
    match rustix::event::poll(&mut fds, timeout.as_ref()) {
        Ok(0) | Err(_) => return false,
        Ok(_) => {}
    }
    let revents = fds.first().map_or(PollFlags::empty(), PollFd::revents);
    if revents.intersects(PollFlags::HUP | PollFlags::ERR) {
        return true;
    }
    let mut buffer = [0u8; 512];
    match (&*stream).read(&mut buffer) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) => !matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
        ),
    }
}

#[cfg(test)]
mod tests;
