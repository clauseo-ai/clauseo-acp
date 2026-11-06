//! Whole lines in and out: [`read_line`], which never holds more than its limit, and the writes
//! to the guard's stdout and to agent-browser's stdin, each one whole line under its lock.

use std::io::{self, BufRead, Write};
use std::process::ChildStdin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::{Value, json};

/// The guard's stdout, shared by every thread that writes to the agent.
pub(crate) type Out = Mutex<io::Stdout>;
/// agent-browser's stdin; `None` once closed.
pub(crate) type ToChild = Arc<Mutex<Option<ChildStdin>>>;

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
pub(crate) fn write_child(to_child: &ToChild, text: &str) -> io::Result<()> {
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
pub(crate) fn emit(out: &Out, bytes: &[u8]) -> io::Result<()> {
    let mut stdout = lock(out);
    stdout.write_all(bytes)?;
    stdout.flush()
}

/// The guard's own error reply to request `id`, which agent-browser will never answer now:
/// JSON-RPC's internal error, with `message`.
pub(crate) fn error_reply(id: &Value, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": message}}).to_string()
}

/// Answers each of `ids` with [`error_reply`]. The agent may have gone; nothing is left to tell
/// then.
pub(crate) fn answer_all(out: &Out, ids: &[Value], message: &str) {
    for id in ids {
        if emit(out, format!("{}\n", error_reply(id, message)).as_bytes()).is_err() {
            return;
        }
    }
}

/// Locks `mutex`, carrying on past a poisoned one: a thread that panicked holding it left whole
/// lines or a consistent state behind.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
