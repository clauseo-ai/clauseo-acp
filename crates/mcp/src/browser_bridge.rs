//! The `browser-bridge` mode, the MCP server the agent's harness starts inside the agent's jail
//! when the browser guard runs outside it. On Unix `clauseo-mcp browser-bridge --socket <path>
//! --guard-pid <pid> --guard-pidversion <version>`, for a guard started as `clauseo-mcp
//! browser-guard --listen <path>` (`transport/unix.rs`): the host starts the guard before the
//! agent, reads the identity the guard prints (`listening as <pid>/<version>`), checks the pid is
//! the child it started, and passes both numbers here (`transport.rs`'s `Identity`: the
//! pidversion on macOS, the start time on Linux). On Windows `--pipe <name> --server-sid <SID>
//! --guard-pid <pid> --guard-pidversion <version>`, for a guard started as `--listen-pipe <name>
//! --client-sid <SID>` (`transport/windows.rs`), the version being the guard's creation time.
//!
//! It relays lines, unchanged: each stdin line to the guard's socket, each socket line to stdout,
//! with the guard's own caps (`browser_guard::lines`). It parses nothing but a line's `id`, for
//! the answers it gives itself: a request (a line with `method` and `id`) is pending until a
//! reply with its id comes back (`link.rs`). The guard checks everything else.
//!
//! Each connection, every time, must pass the peer check ([`Connection::peer_is`]): the process
//! at the other end must run as the bridge's own user and be that very process, its pid and its
//! version both. On macOS the agent and the lawyer share a uid, so the user alone proves
//! nothing; the identity is what keeps a process of the same user that squats the path (after
//! the guard has gone, say), or reuses the guard's pid, from posing as the guard. One that fails,
//! or whose identity cannot be read, is closed unused, as though the guard were not there. No
//! other identity is ever taken: if the guard dies, the browser is off for the session, and a
//! guard the host restarts comes with a restarted bridge.
//! The boundary on macOS is the guard's and the agent's Seatbelt profiles, the socket folder's
//! check (`transport/unix.rs`) and this check; checking the peer's code identity (its audit
//! token) is later hardening.
//! On Windows the pipe's owner must be the `--server-sid` account (the guard's), so a pipe the
//! agent's own account made first under the name is refused, and the pipe's server process must
//! be the guard by pid and creation time, so an instance another process of the browser's account
//! (agent-browser, its daemon) added is refused too (`transport/windows/identity.rs`).
//!
//! When it cannot reach the guard (no socket, refused, or the guard gone mid-session) it answers
//! each request with [`UNAVAILABLE`] and keeps reading stdin to its end, so the agent's client
//! never waits for an answer that will not come. A link that ends answers its pending requests
//! the same way. While there is no link, each request line gets one try to connect, bounded by
//! [`CONNECT_WAIT`]; a line that is no request (a notification) is dropped and tries nothing.
//! The first link is made at the first request too.
//!
//! A stdin line over the guard's cap gets the parse error the guard would give, and is not sent.
//! A socket line over the cap, or one the socket ends in the middle of, ends the link.
//!
//! It sees the transport only through `crate::transport`'s [`Connection`]; `connect_within` is the
//! one Unix-socket call, and `transport::windows::connect_within` the pipe's. A pipe has no
//! half-close, so on Windows the guard learns stdin has ended only when the bridge closes, after
//! [`EXIT_GRACE`] (2 s): an agent leaving takes up to 2 s longer to reach the guard there. It
//! touches no file but the socket it connects to, and writes no path, pipe name or SID anywhere (on
//! Linux, a development platform, it reads the peer's `/proc/<pid>/stat`). Exit 0 when stdin ends;
//! 1 when stdout fails; 2 for a bad command line, `--guard-pid` or `--guard-pidversion` missing
//! included.

mod link;
#[cfg(test)]
mod parse_tests;
// The bridge against a stand-in guard on a Unix socket; the pipe's own tests are in
// `transport/windows/tests.rs`.
#[cfg(test)]
#[cfg(unix)]
mod tests;

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
#[cfg(unix)]
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::Failure;
use crate::browser_guard::lines::{
    AGENT_MAX_LINE_BYTES, CHILD_MAX_LINE_BYTES, Line, Overflow, error_reply, read_line,
};
use crate::connector::head;
use crate::transport::{Connection, Identity, Peer, checks};
use link::{Link, Sent};

/// The mode's command line on this OS.
pub const USAGE: &str = if cfg!(windows) {
    "usage: clauseo-mcp browser-bridge --pipe <name> --server-sid <sid> --guard-pid <pid> --guard-pidversion <version>"
} else {
    "usage: clauseo-mcp browser-bridge --socket <path> --guard-pid <pid> --guard-pidversion <version>"
};
/// The error each request gets when the guard cannot be reached.
pub(crate) const UNAVAILABLE: &str = "The browser isn't available right now.";
/// How long one try to connect may take.
pub(crate) const CONNECT_WAIT: Duration = Duration::from_secs(3);
/// How long, once stdin has ended, the bridge waits for the guard's last lines.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// The exit code for a bad command line.
const EXIT_USAGE: i32 = 2;
/// The exit code when stdout cannot take a line: the agent has gone.
const EXIT_FAILED: i32 = 1;

/// Stdout, shared by the stdin side and the link's reader: one lock, whole lines.
pub(crate) type Out = Arc<Mutex<dyn Write + Send>>;

/// The line caps: the agent's lines and the guard's. The guard's own unless a test says less.
#[derive(Clone, Copy)]
pub(crate) struct Caps {
    pub(crate) agent: usize,
    pub(crate) guard: usize,
}

impl Caps {
    /// The guard's caps.
    pub(crate) const GUARD: Caps = Caps {
        agent: AGENT_MAX_LINE_BYTES,
        guard: CHILD_MAX_LINE_BYTES,
    };
}

/// Runs the mode with the arguments after `browser-bridge`, on the process's stdin and stdout.
pub fn run(args: &[OsString]) -> Result<i32, Failure> {
    let target = parse(args, cfg!(windows)).map_err(usage)?;
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let stdin = io::stdin().lock();
    let served = match target {
        #[cfg(unix)]
        Target::Socket { socket, guard } => {
            let connect = || connect_within(&socket, CONNECT_WAIT);
            serve(stdin, &out, &connect, guard, Caps::GUARD)
        }
        #[cfg(windows)]
        Target::Pipe {
            name,
            server_sid,
            guard,
        } => {
            use crate::transport::windows::{Sid, connect_within};
            let server = Sid::parse(&server_sid).ok_or_else(|| usage(SERVER_SID_BAD))?;
            let connect = || connect_within(&name, &server, CONNECT_WAIT);
            serve(stdin, &out, &connect, guard, Caps::GUARD)
        }
        // `parse` gives only this OS's target.
        #[cfg(unix)]
        Target::Pipe { .. } => return Err(usage(PIPE_WINDOWS_ONLY)),
        #[cfg(windows)]
        Target::Socket { .. } => return Err(usage(SOCKET_UNIX_ONLY)),
    };
    served.map_err(|error| Failure {
        code: EXIT_FAILED,
        message: format!("clauseo-mcp browser-bridge: stdio failed: {error}"),
    })?;
    Ok(0)
}

/// A bad command line: exit 2, the reason and the usage.
fn usage(reason: &str) -> Failure {
    Failure {
        code: EXIT_USAGE,
        message: format!("clauseo-mcp browser-bridge: {reason}\n{USAGE}"),
    }
}

/// Where the guard is.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// Unix: `--socket <path> --guard-pid <pid> --guard-pidversion <version>`.
    Socket { socket: PathBuf, guard: Peer },
    /// Windows: `--pipe <name> --server-sid <SID> --guard-pid <pid> --guard-pidversion <version>`.
    Pipe {
        name: String,
        server_sid: String,
        guard: Peer,
    },
}

/// The refusals of the command line. None names a path, a pipe or a SID.
const EXPECTED_SOCKET: &str =
    "expected --socket <path> --guard-pid <pid> --guard-pidversion <version>";
const EXPECTED_PIPE: &str =
    "expected --pipe <name> --server-sid <sid> --guard-pid <pid> --guard-pidversion <version>";
const SOCKET_UNIX_ONLY: &str = "--socket is for macOS and Linux only";
const PIPE_WINDOWS_ONLY: &str = "--pipe and --server-sid are for Windows only";
const PIPE_NAME_BAD: &str = "--pipe must be 1 to 200 letters, digits and dashes";
const SERVER_SID_BAD: &str = "--server-sid must be a SID string, S-1-...";

/// On Unix exactly `--socket <path>`, the path absolute, on Windows (`windows`) exactly `--pipe
/// <name> --server-sid <SID>`, the name letters, digits and dashes; then on both
/// `--guard-pid <pid> --guard-pidversion <version>` ([`guard_identity`]). Each OS refuses the
/// other's options.
fn parse(args: &[OsString], windows: bool) -> Result<Target, &'static str> {
    let pipe_flag = |flag: &OsString| flag == "--pipe" || flag == "--server-sid";
    let identity_flags = |pid: &OsString, version: &OsString| {
        pid == "--guard-pid" && version == "--guard-pidversion"
    };
    match (windows, args) {
        (false, [flag_socket, socket, flag_pid, pid, flag_version, version])
            if flag_socket == "--socket" && identity_flags(flag_pid, flag_version) =>
        {
            let socket = Path::new(socket);
            if !socket.is_absolute() {
                return Err("--socket must be an absolute path");
            }
            Ok(Target::Socket {
                socket: socket.to_owned(),
                guard: guard_identity(pid, version)?,
            })
        }
        (false, [flag, ..]) if pipe_flag(flag) => Err(PIPE_WINDOWS_ONLY),
        (false, _) => Err(EXPECTED_SOCKET),
        (
            true,
            [
                flag,
                name,
                sid_flag,
                sid,
                flag_pid,
                pid,
                flag_version,
                version,
            ],
        ) if flag == "--pipe"
            && sid_flag == "--server-sid"
            && identity_flags(flag_pid, flag_version) =>
        {
            let name = name
                .to_str()
                .filter(|name| checks::pipe_name_ok(name))
                .ok_or(PIPE_NAME_BAD)?;
            let sid = sid
                .to_str()
                .filter(|sid| checks::sid_text_ok(sid))
                .ok_or(SERVER_SID_BAD)?;
            Ok(Target::Pipe {
                name: name.to_owned(),
                server_sid: sid.to_owned(),
                guard: guard_identity(pid, version)?,
            })
        }
        (true, [flag, ..]) if flag == "--socket" => Err(SOCKET_UNIX_ONLY),
        (true, _) => Err(EXPECTED_PIPE),
    }
}

/// The guard's identity from `--guard-pid` and `--guard-pidversion`: plain decimal, the pid
/// over 1 (no guard is pid 0 or init). The version is the pidversion on macOS, the start time on
/// Linux and the creation time on Windows.
fn guard_identity(pid: &OsString, version: &OsString) -> Result<Peer, &'static str> {
    let pid = decimal(pid)
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid > 1)
        .ok_or("--guard-pid must be the guard's process id, in plain decimal")?;
    let version = decimal(version)
        .ok_or("--guard-pidversion must be the guard's process version, in plain decimal")?;
    Ok(Peer::Process(Identity { pid, version }))
}

/// `value` as a number in plain decimal: digits only, no leading zero but for `0` itself.
fn decimal(value: &OsString) -> Option<u64> {
    let text = value.to_str()?;
    let plain = !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    plain.then(|| text.parse().ok()).flatten()
}

/// Connects to the socket at `path`, giving up after `within`. A connect still running then is
/// left to finish on its own thread, which closes what it gets.
#[cfg(unix)]
pub(crate) fn connect_within(path: &Path, within: Duration) -> io::Result<UnixStream> {
    let (done, result) = mpsc::channel();
    let path = path.to_owned();
    drop(thread::spawn(move || {
        // Nothing waits any more when the send fails; the stream, if any, closes here.
        drop(done.send(UnixStream::connect(&path)));
    }));
    result
        .recv_timeout(within)
        .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into()))
}

/// Relays `stdin` to the guard and the guard to `out` until stdin ends, connecting through
/// `connect` when a request comes with no link and keeping a connection only if its other end is
/// `guard`. `Err` when stdin or stdout fails.
pub(crate) fn serve<C: Connection>(
    mut stdin: impl BufRead,
    out: &Out,
    connect: &dyn Fn() -> io::Result<C>,
    guard: Peer,
    caps: Caps,
) -> io::Result<()> {
    let mut link: Option<Link<C>> = None;
    let mut line = Vec::new();
    loop {
        match read_line(&mut stdin, &mut line, caps.agent, Overflow::Skip)? {
            Line::Eof => break,
            Line::Oversize => {
                write_line(out, &parse_error())?;
                continue;
            }
            Line::Whole | Line::Partial => {}
        }
        let id = request_id(&line);
        if link.as_ref().is_some_and(Link::ended) {
            link = None;
        }
        if link.is_none() && id.is_some() {
            // The peer check: any process at the path but the guard (a squatter) is no guard.
            link = connect()
                .ok()
                .filter(|socket| socket.peer_is(guard))
                .and_then(|socket| Link::start(socket, out, caps.guard).ok());
        }
        let lost = match &mut link {
            Some(open) => match open.send(&line, id.as_ref()) {
                Sent::Sent => false,
                Sent::Lost { answer } => {
                    link = None;
                    answer
                }
            },
            None => true,
        };
        if lost && let Some(id) = &id {
            write_line(out, &unavailable(id))?;
        }
    }
    if let Some(link) = link {
        for id in link.finish(EXIT_GRACE) {
            write_line(out, &unavailable(&id))?;
        }
    }
    Ok(())
}

/// The `id` of a request (a line with `method` and `id`); `None` for anything else.
fn request_id(line: &[u8]) -> Option<Value> {
    head::read(line).and_then(|head| if head.method { head.id } else { None })
}

/// The bridge's error reply to request `id`.
pub(crate) fn unavailable(id: &Value) -> Vec<u8> {
    error_reply(id, UNAVAILABLE).into_bytes()
}

/// The parse error the guard gives a line over its cap (`filter.rs`).
fn parse_error() -> Vec<u8> {
    json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}})
        .to_string()
        .into_bytes()
}

/// Writes `line` and a newline to `out` under its lock, in one write.
pub(crate) fn write_line(out: &Out, line: &[u8]) -> io::Result<()> {
    let mut whole = Vec::with_capacity(line.len() + 1);
    whole.extend_from_slice(line);
    whole.push(b'\n');
    // A writer that panicked left whole lines behind; carry on.
    let mut stdout = out.lock().unwrap_or_else(PoisonError::into_inner);
    stdout.write_all(&whole)?;
    stdout.flush()
}
