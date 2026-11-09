//! The `browser-bridge` mode (Unix only): `clauseo-mcp browser-bridge --socket <path>`, the MCP
//! server the agent's harness starts inside the agent's jail when the browser guard runs outside
//! it, as `clauseo-mcp browser-guard --listen <path>` (`browser_guard/listen.rs`).
//!
//! It relays lines, unchanged: each stdin line to the guard's socket, each socket line to stdout,
//! with the guard's own caps (`browser_guard::lines`). It parses nothing but a line's `id`, for
//! the answers it gives itself: a request (a line with `method` and `id`) is pending until a
//! reply with its id comes back (`link.rs`). The guard checks everything else.
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
//! It sees the transport only through `crate::transport`'s [`Connection`]; [`connect_within`]
//! is the one Unix-socket call. It touches no file but the socket it connects to, and writes no
//! path anywhere. Exit 0 when
//! stdin ends; 1 when stdout fails; 2 for a bad command line.

mod link;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::Failure;
use crate::browser_guard::lines::{
    AGENT_MAX_LINE_BYTES, CHILD_MAX_LINE_BYTES, Line, Overflow, error_reply, read_line,
};
use crate::connector::head;
use crate::transport::Connection;
use link::{Link, Sent};

/// The mode's command line.
pub const USAGE: &str = "usage: clauseo-mcp browser-bridge --socket <path>";
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
    let socket = parse(args).map_err(|reason| Failure {
        code: EXIT_USAGE,
        message: format!("clauseo-mcp browser-bridge: {reason}\n{USAGE}"),
    })?;
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let connect = || connect_within(&socket, CONNECT_WAIT);
    serve(io::stdin().lock(), &out, &connect, Caps::GUARD).map_err(|error| Failure {
        code: EXIT_FAILED,
        message: format!("clauseo-mcp browser-bridge: stdio failed: {error}"),
    })?;
    Ok(0)
}

/// Exactly `--socket <path>`, the path absolute. The reasons name no path.
fn parse(args: &[OsString]) -> Result<PathBuf, &'static str> {
    let [flag, socket] = args else {
        return Err("expected --socket <path>");
    };
    if flag != "--socket" {
        return Err("expected --socket <path>");
    }
    let socket = Path::new(socket);
    if !socket.is_absolute() {
        return Err("--socket must be an absolute path");
    }
    Ok(socket.to_owned())
}

/// Connects to the socket at `path`, giving up after `within`. A connect still running then is
/// left to finish on its own thread, which closes what it gets.
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
/// `connect` when a request comes with no link. `Err` when stdin or stdout fails.
pub(crate) fn serve<C: Connection>(
    mut stdin: impl BufRead,
    out: &Out,
    connect: &dyn Fn() -> io::Result<C>,
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
            link = connect()
                .ok()
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
