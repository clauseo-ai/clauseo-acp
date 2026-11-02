//! The child and the two pumps. agent-browser starts with the map `env.rs` built, in `--cwd`,
//! with piped stdin and stdout and the guard's stderr. One thread copies its stdout to the
//! agent, whole lines and unchanged bytes; another reads the agent's lines, runs each through
//! `filter.rs`, and writes what passes to the child. Both write to the guard's stdout only under
//! one lock, a whole line at a time, so a refusal never lands inside a line of the child's.
//!
//! When the agent closes stdin, the guard asks agent-browser to close its browser (one
//! `agent_browser_close` call, whose reply is not relayed), waits up to [`CLOSE_WAIT`] for that
//! reply, closes the child's stdin and waits for it to exit. agent-browser's daemon is a detached
//! process (`setsid`, or `DETACHED_PROCESS` on Windows), so closing it here is best-effort; the
//! idle timeout ends it otherwise. When the child ends first, the guard exits at once.

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

use super::filter::{self, MAX_LINE_BYTES, Verdict};

/// The id of the guard's own close request; the reply with this id is swallowed.
pub(crate) const CLOSE_ID: &str = "clauseo-guard-close";
/// The request sent when the agent closes stdin.
pub(crate) const CLOSE_REQUEST: &str = r#"{"jsonrpc":"2.0","id":"clauseo-guard-close","method":"tools/call","params":{"name":"agent_browser_close","arguments":{}}}"#;
/// How long the guard waits for the close reply before closing the child's stdin anyway.
pub(crate) const CLOSE_WAIT: Duration = Duration::from_secs(10);

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
    /// The agent's stdin reached EOF and the close request went out.
    AgentEnded,
    /// The reply to the close request arrived.
    CloseAnswered,
}

/// Starts agent-browser and relays until it exits. Returns its exit code, or 1 when a signal
/// killed it.
pub(crate) fn run(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    env: &BTreeMap<String, OsString>,
) -> io::Result<i32> {
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
            pump_child(BufReader::new(stdout), &out, &closing, &events);
            send(&events, Event::ChildEnded);
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

    wait_for_end(&inbox, &to_child);
    if child_pump.join().is_err() {
        return Err(io::Error::other("the stdout relay thread panicked"));
    }
    Ok(child.wait()?.code().unwrap_or(1))
}

/// Returns once the child's stdout has ended, closing its stdin after the agent's EOF and the
/// close reply (or [`CLOSE_WAIT`]).
fn wait_for_end(inbox: &mpsc::Receiver<Event>, to_child: &ToChild) {
    let mut closing = false;
    loop {
        let event = if closing {
            match inbox.recv_timeout(CLOSE_WAIT) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => Event::CloseAnswered,
                Err(RecvTimeoutError::Disconnected) => Event::ChildEnded,
            }
        } else {
            inbox.recv().unwrap_or(Event::ChildEnded)
        };
        match event {
            Event::ChildEnded => return,
            Event::AgentEnded => closing = true,
            Event::CloseAnswered => {
                closing = false;
                drop(lock(to_child).take());
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

/// Child to agent: each line as read, the reply to the close request excepted.
fn pump_child(
    mut stdout: impl BufRead,
    out: &Out,
    closing: &AtomicBool,
    events: &mpsc::Sender<Event>,
) {
    let mut line = Vec::new();
    // Once the agent stops reading, the child's output is still drained so it never blocks.
    let mut agent_reads = true;
    loop {
        line.clear();
        match stdout.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if closing.load(Ordering::SeqCst) && is_close_reply(&line) {
            send(events, Event::CloseAnswered);
            continue;
        }
        if !line.ends_with(b"\n") {
            line.push(b'\n');
        }
        if agent_reads && emit(out, &line).is_err() {
            agent_reads = false;
        }
    }
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
        let verdict = match read_line(stdin, &mut line, MAX_LINE_BYTES) {
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::Oversize) => filter::parse_error(),
            Ok(Line::Whole) => filter::check(&line),
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
    /// A line, now in the buffer without its newline (the last one may have none).
    Whole,
    /// A line over the limit, read to its end and thrown away.
    Oversize,
    /// The end of input, nothing read.
    Eof,
}

/// Reads one line into `buf` (cleared first), never holding more than `max` bytes of it.
pub(crate) fn read_line(
    input: &mut impl BufRead,
    buf: &mut Vec<u8>,
    max: usize,
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
                (false, true) => Line::Whole,
                (false, false) => Line::Eof,
            });
        }
        any = true;
        let newline = chunk.iter().position(|&b| b == b'\n');
        let take = newline.unwrap_or(chunk.len());
        if !over {
            if buf.len() + take > max {
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

    use super::{Line, read_line};

    fn lines(input: &[u8], max: usize) -> Vec<(Line, Vec<u8>)> {
        let mut input = std::io::BufReader::with_capacity(4, Cursor::new(input.to_vec()));
        let mut buf = Vec::new();
        let mut found = Vec::new();
        loop {
            let line = read_line(&mut input, &mut buf, max).unwrap();
            if line == Line::Eof {
                return found;
            }
            found.push((line, buf.clone()));
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
                (Line::Whole, b"xyz".to_vec()),
            ]
        );
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
                (Line::Whole, b"ok".to_vec()),
            ]
        );
        assert_eq!(lines(b"123456789", 4), [(Line::Oversize, Vec::new())]);
    }
}
