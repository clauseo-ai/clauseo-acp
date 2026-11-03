//! The child and the two pumps. agent-browser starts with the map `env.rs` built, in `--cwd`,
//! with piped stdin and stdout and the guard's stderr. One thread copies its stdout to the
//! agent, whole lines and unchanged bytes; another reads the agent's lines, runs each through
//! `filter.rs`, and writes what passes to the child. Both write to the guard's stdout only under
//! one lock, a whole line at a time, so a refusal never lands inside a line of the child's.
//!
//! When the agent closes stdin, the guard asks agent-browser to close its browser (one
//! `agent_browser_close` call, whose reply is not relayed), waits up to [`CLOSE_WAIT`] for that
//! reply, closes the child's stdin and waits up to [`EXIT_WAIT`] for its stdout to end, then
//! kills it. agent-browser's daemon is a detached process (`setsid`, or `DETACHED_PROCESS` on
//! Windows), so closing it here is best-effort; the host's session end and the idle timeout end
//! it otherwise. When the child ends first, the guard exits at once.
//!
//! The child's lines are capped at [`CHILD_MAX_LINE_BYTES`]: a longer one kills the child and
//! the guard exits 1 (fail closed). A last line cut off by the child's end is dropped, with a
//! note on stderr, so the agent never gets half a JSON message.
//!
//! On Windows, after each line it relays, the stdout pump checks the daemon's `.port` file
//! (`daemon_port.rs`) until it holds the claimed port; a daemon on any other port kills the
//! child and the guard exits 1.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::daemon_port::{self, Expected, Status};
use super::filter::{self, MAX_LINE_BYTES, RESERVED_ID as CLOSE_ID, Verdict};
/// The request sent when the agent closes stdin.
pub(crate) const CLOSE_REQUEST: &str = r#"{"jsonrpc":"2.0","id":"clauseo-guard-close","method":"tools/call","params":{"name":"agent_browser_close","arguments":{}}}"#;
/// How long the guard waits for the close reply before closing the child's stdin anyway.
pub(crate) const CLOSE_WAIT: Duration = Duration::from_secs(10);
/// How long the guard waits for the child's stdout to end after closing its stdin, before it
/// kills the child.
pub(crate) const EXIT_WAIT: Duration = Duration::from_secs(5);
/// The longest child line the guard relays, in bytes without the newline.
pub(crate) const CHILD_MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

/// Windows: the child gets no console window (agent-browser issue #1644).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The guard's stdout, shared by both pumps.
type Out = Arc<Mutex<io::Stdout>>;
/// The child's stdin; `None` once closed.
type ToChild = Arc<Mutex<Option<ChildStdin>>>;

enum Event {
    /// The child's stdout reached EOF.
    ChildEnded,
    /// The child wrote a line over [`CHILD_MAX_LINE_BYTES`].
    ChildOversize,
    /// The agent's stdin reached EOF and the close request went out.
    AgentEnded,
    /// The reply to the close request arrived.
    CloseAnswered,
    /// The daemon is not on the claimed port; the reason.
    DaemonPortRefused(String),
}

/// How the relay ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The child exited (or was killed after [`EXIT_WAIT`]) with this code; 1 for a signal.
    Exited(i32),
    /// The child wrote a line over [`CHILD_MAX_LINE_BYTES`] and was killed.
    ChildLineTooLong,
    /// Windows: the daemon's `.port` file named another port, or could not be trusted, and the
    /// child was killed. The reason.
    DaemonPortRefused(String),
}

/// What [`wait_for_end`] saw.
enum End {
    ChildEnded,
    ChildOversize,
    ExitTimedOut,
    DaemonPortRefused(String),
}

/// Starts agent-browser and relays until it exits. With `port_check` (Windows), the daemon's
/// port is checked after each relayed line until it is verified.
pub(crate) fn run(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    env: &BTreeMap<String, OsString>,
    port_check: Option<Expected>,
) -> io::Result<Outcome> {
    let mut child = spawn_child(program, args, cwd, env)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("the child's stdout is not piped"))?;
    let to_child: ToChild = Arc::new(Mutex::new(child.stdin.take()));
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let closing = Arc::new(AtomicBool::new(false));
    let (events, inbox) = mpsc::channel();

    let child_pump = {
        let (out, closing, events) = (Arc::clone(&out), Arc::clone(&closing), events.clone());
        thread::spawn(move || {
            let end = pump_child(BufReader::new(stdout), &out, &closing, &events, port_check);
            send(&events, end);
        })
    };
    {
        let (out, to_child) = (Arc::clone(&out), Arc::clone(&to_child));
        // Never joined: when the child ends first, the guard exits with this thread still
        // blocked on the agent's stdin.
        drop(thread::spawn(move || {
            pump_agent(&mut io::stdin().lock(), &to_child, &out, &closing, &events);
        }));
    }

    match wait_for_end(&inbox, &to_child) {
        End::ChildEnded => {
            if child_pump.join().is_err() {
                return Err(io::Error::other("the stdout relay thread panicked"));
            }
            Ok(Outcome::Exited(child.wait()?.code().unwrap_or(1)))
        }
        End::ChildOversize => {
            stop(&mut child)?;
            Ok(Outcome::ChildLineTooLong)
        }
        End::DaemonPortRefused(reason) => {
            stop(&mut child)?;
            Ok(Outcome::DaemonPortRefused(reason))
        }
        End::ExitTimedOut => {
            note(&format!(
                "agent-browser did not exit within {} s of its stdin closing; killed it",
                EXIT_WAIT.as_secs()
            ));
            Ok(Outcome::Exited(stop(&mut child)?.code().unwrap_or(1)))
        }
    }
}

/// Kills the child (it may have exited already) and reaps it. The stdout pump is not joined:
/// a grandchild could still hold the pipe open.
fn stop(child: &mut Child) -> io::Result<std::process::ExitStatus> {
    if child.try_wait()?.is_none() {
        // It may exit between the two calls; `wait` below reaps it either way.
        drop(child.kill());
    }
    child.wait()
}

/// The phase of the shutdown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Relaying; nothing has ended.
    Running,
    /// The agent has gone and the close request is out; waiting for its reply.
    Closing,
    /// The child's stdin is closed; waiting for its stdout to end.
    Draining,
}

/// Returns once the child's stdout has ended, it wrote an oversize line, or it outlived
/// [`EXIT_WAIT`] after its stdin was closed (following the agent's EOF and the close reply, or
/// [`CLOSE_WAIT`]). Each wait restarts on an event, but each phase admits only a bounded number
/// of them: the close reply is sent at most twice (by the reply, or by a failed write).
fn wait_for_end(inbox: &mpsc::Receiver<Event>, to_child: &ToChild) -> End {
    let mut phase = Phase::Running;
    loop {
        let event = match phase {
            Phase::Running => inbox.recv().unwrap_or(Event::ChildEnded),
            Phase::Closing => match inbox.recv_timeout(CLOSE_WAIT) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => Event::CloseAnswered,
                Err(RecvTimeoutError::Disconnected) => Event::ChildEnded,
            },
            Phase::Draining => match inbox.recv_timeout(EXIT_WAIT) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => return End::ExitTimedOut,
                Err(RecvTimeoutError::Disconnected) => Event::ChildEnded,
            },
        };
        match event {
            Event::ChildEnded => return End::ChildEnded,
            Event::ChildOversize => return End::ChildOversize,
            Event::DaemonPortRefused(reason) => return End::DaemonPortRefused(reason),
            Event::AgentEnded => {
                if phase == Phase::Running {
                    phase = Phase::Closing;
                }
            }
            Event::CloseAnswered => {
                if phase != Phase::Draining {
                    phase = Phase::Draining;
                    drop(lock(to_child).take());
                }
            }
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the guard's one fixed child: agent-browser, whose tree the host kills; the host \
              starts the guard itself through the process crate (docs/decisions/0029-browser.md)"
)]
fn spawn_child(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    env: &BTreeMap<String, OsString>,
) -> io::Result<Child> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn()
}

/// Child to agent: each whole line as read, the reply to the close request excepted. With
/// `port_check`, after each line the daemon's port until it is verified. Returns the event
/// that ends it: [`Event::ChildEnded`], [`Event::ChildOversize`] or
/// [`Event::DaemonPortRefused`].
fn pump_child(
    mut stdout: impl BufRead,
    out: &Out,
    closing: &AtomicBool,
    events: &mpsc::Sender<Event>,
    mut port_check: Option<Expected>,
) -> Event {
    let mut line = Vec::new();
    // Once the agent stops reading, the child's output is still drained so it never blocks.
    let mut agent_reads = true;
    loop {
        match read_line(&mut stdout, &mut line, CHILD_MAX_LINE_BYTES, Overflow::Stop) {
            Ok(Line::Eof) | Err(_) => return Event::ChildEnded,
            // `browser_guard::run` says so on stderr once the child is killed.
            Ok(Line::Oversize) => return Event::ChildOversize,
            Ok(Line::Partial) => {
                note(&format!(
                    "agent-browser ended in the middle of a line; dropped its last {} bytes",
                    line.len()
                ));
                return Event::ChildEnded;
            }
            Ok(Line::Whole) => {}
        }
        if closing.load(Ordering::SeqCst) && is_close_reply(&line) {
            send(events, Event::CloseAnswered);
            continue;
        }
        line.push(b'\n');
        if agent_reads && emit(out, &line).is_err() {
            agent_reads = false;
        }
        if let Some(expected) = &port_check {
            match daemon_port::judge(daemon_port::read(&expected.file), expected.port) {
                Status::Pending => {}
                Status::Verified => port_check = None,
                Status::Refused(reason) => return Event::DaemonPortRefused(reason),
            }
        }
    }
}

/// One line on the guard's stderr. Diagnostics only; a failed write has nowhere to go.
fn note(text: &str) {
    let mut stderr = io::stderr().lock();
    if writeln!(stderr, "clauseo-mcp browser-guard: {text}").is_err() {}
}

fn is_close_reply(line: &[u8]) -> bool {
    serde_json::from_slice::<Value>(line).is_ok_and(|reply| {
        reply.get("id").and_then(Value::as_str) == Some(CLOSE_ID) && reply.get("method").is_none()
    })
}

/// Agent to child: every line through the filter. On EOF, sends the close request.
fn pump_agent(
    stdin: &mut impl BufRead,
    to_child: &ToChild,
    out: &Out,
    closing: &AtomicBool,
    events: &mpsc::Sender<Event>,
) {
    let mut line = Vec::new();
    loop {
        let verdict = match read_line(stdin, &mut line, MAX_LINE_BYTES, Overflow::Skip) {
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::Oversize) => filter::parse_error(),
            // The agent's last line may lack its newline; it is checked like any other.
            Ok(Line::Whole | Line::Partial) => filter::check(&line),
        };
        let sent = match verdict {
            Verdict::Forward(text) => write_child(to_child, &text),
            // The agent has gone if its stdout fails; its stdin EOF follows.
            Verdict::Answer(text) => emit(out, format!("{text}\n").as_bytes()).or(Ok(())),
            Verdict::Drop => Ok(()),
        };
        if sent.is_err() {
            // The child has gone; the stdout pump sees its EOF and the guard exits.
            return;
        }
    }
    closing.store(true, Ordering::SeqCst);
    send(events, Event::AgentEnded);
    if write_child(to_child, CLOSE_REQUEST).is_err() {
        send(events, Event::CloseAnswered);
    }
}

/// What [`read_line`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// A line ended by a newline, now in the buffer without it.
    Whole,
    /// Bytes the input ended in without a newline, now in the buffer.
    Partial,
    /// A line over the limit, thrown away (read to its end with [`Overflow::Skip`]).
    Oversize,
    /// The end of input, nothing read.
    Eof,
}

/// What [`read_line`] does with a line over its limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overflow {
    /// Read it to its end and throw it away, so the next line can be read.
    Skip,
    /// Return at once, leaving the rest unread: the stream is not read again.
    Stop,
}

/// Reads one line into `buf` (cleared first), never holding more than `max` bytes of it.
pub(crate) fn read_line(
    input: &mut impl BufRead,
    buf: &mut Vec<u8>,
    max: usize,
    overflow: Overflow,
) -> io::Result<Line> {
    buf.clear();
    let mut over = false;
    let mut any = false;
    loop {
        let chunk = match input.fill_buf() {
            Ok(chunk) => chunk,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if chunk.is_empty() {
            return Ok(match (over, any) {
                (true, _) => Line::Oversize,
                (false, true) => Line::Partial,
                (false, false) => Line::Eof,
            });
        }
        any = true;
        let newline = chunk.iter().position(|&b| b == b'\n');
        let take = newline.unwrap_or(chunk.len());
        if !over {
            if buf.len() + take > max {
                if overflow == Overflow::Stop {
                    buf.clear();
                    return Ok(Line::Oversize);
                }
                over = true;
                buf.clear();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        input.consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(if over { Line::Oversize } else { Line::Whole });
        }
    }
}

/// Writes one whole line to the child, newline included, in one call under its lock.
fn write_child(to_child: &ToChild, text: &str) -> io::Result<()> {
    let mut guard = lock(to_child);
    let stdin = guard
        .as_mut()
        .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?;
    let mut line = Vec::with_capacity(text.len() + 1);
    line.extend_from_slice(text.as_bytes());
    line.push(b'\n');
    stdin.write_all(&line)?;
    stdin.flush()
}

/// Writes whole lines to the guard's stdout under the shared lock.
fn emit(out: &Out, bytes: &[u8]) -> io::Result<()> {
    let mut stdout = lock(out);
    stdout.write_all(bytes)?;
    stdout.flush()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A pump that panicked holding the lock leaves whole lines behind; carry on.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send(events: &mpsc::Sender<Event>, event: Event) {
    // The receiver goes only when the guard is returning; nothing waits for this event then.
    if events.send(event).is_err() {}
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{Line, Overflow, read_line};

    fn lines(input: &[u8], max: usize) -> Vec<(Line, Vec<u8>)> {
        lines_with(input, max, Overflow::Skip)
    }

    fn lines_with(input: &[u8], max: usize, overflow: Overflow) -> Vec<(Line, Vec<u8>)> {
        let mut input = std::io::BufReader::with_capacity(4, Cursor::new(input.to_vec()));
        let mut buf = Vec::new();
        let mut found = Vec::new();
        loop {
            let line = read_line(&mut input, &mut buf, max, overflow).unwrap();
            if line == Line::Eof {
                return found;
            }
            let stop = overflow == Overflow::Stop && line == Line::Oversize;
            found.push((line, buf.clone()));
            if stop {
                return found;
            }
        }
    }

    #[test]
    fn lines_split_on_newline_across_reads() {
        let found = lines(b"abcdefgh\n\nxyz", 100);
        assert_eq!(
            found,
            [
                (Line::Whole, b"abcdefgh".to_vec()),
                (Line::Whole, Vec::new()),
                (Line::Partial, b"xyz".to_vec()),
            ]
        );
    }

    #[test]
    fn stop_returns_at_the_limit_without_reading_on() {
        let mut input =
            std::io::BufReader::with_capacity(4, Cursor::new(b"ok\n123456789\nmore".to_vec()));
        let mut buf = Vec::new();
        assert_eq!(
            read_line(&mut input, &mut buf, 4, Overflow::Stop).unwrap(),
            Line::Whole
        );
        assert_eq!(
            read_line(&mut input, &mut buf, 4, Overflow::Stop).unwrap(),
            Line::Oversize
        );
        assert_eq!(buf, b"");
        let mut rest = Vec::new();
        std::io::Read::read_to_end(&mut input, &mut rest).unwrap();
        assert!(rest.ends_with(b"\nmore") && rest.len() < b"123456789\nmore".len());
        assert_eq!(lines_with(b"1234\n12345", 4, Overflow::Stop).len(), 2);
    }

    #[test]
    fn a_line_over_the_limit_is_skipped_whole() {
        let found = lines(b"12345\n1234\n123456789\nok", 4);
        assert_eq!(
            found,
            [
                (Line::Oversize, Vec::new()),
                (Line::Whole, b"1234".to_vec()),
                (Line::Oversize, Vec::new()),
                (Line::Partial, b"ok".to_vec()),
            ]
        );
        assert_eq!(lines(b"123456789", 4), [(Line::Oversize, Vec::new())]);
    }
}
