//! The agent's stderr: the pump that drains the pipe, the 256 KiB ring it fills, and the redacted
//! tail of at most 4 KiB that an exit report carries.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use async_process::ChildStderr;
use futures::AsyncReadExt;

pub(crate) use redact::Redactor;

mod redact;

/// How much stderr the ring keeps per agent. It never leaves the crate whole.
pub(crate) const RING: usize = 256 * 1024;
/// The most one read takes from the pipe.
const READ: usize = 16 * 1024;
/// The most bytes [`tail`] returns.
pub(crate) const TAIL: usize = 4096;
/// How much of the ring [`tail`] redacts first.
const WINDOW: usize = 16 * 1024;
/// The longest line [`StderrPump::lines`] passes whole; a longer one comes in pieces this long.
const MAX_LINE: usize = 16 * 1024;

/// The last [`RING`] bytes of stderr: a fixed buffer and the position the next byte goes to.
pub(crate) struct Ring {
    /// Always [`RING`] bytes.
    buf: Box<[u8]>,
    /// Where the next byte goes.
    pos: usize,
    /// Whether the ring has come round, so its oldest byte is not stderr's first.
    wrapped: bool,
}

impl Ring {
    pub(crate) fn new() -> Ring {
        Ring {
            // Zeroed by the allocator, so its pages are only touched as stderr fills them.
            buf: vec![0; RING].into_boxed_slice(),
            pos: 0,
            wrapped: false,
        }
    }

    /// Appends `bytes`, overwriting the oldest, with at most two copies.
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let bytes = &bytes[bytes.len().saturating_sub(RING)..];
        let first = bytes.len().min(RING - self.pos);
        let (head, rest) = bytes.split_at(first);
        self.buf[self.pos..self.pos + first].copy_from_slice(head);
        self.buf[..rest.len()].copy_from_slice(rest);
        let end = self.pos + bytes.len();
        self.wrapped |= end >= RING;
        self.pos = end % RING;
    }

    /// How many bytes it holds.
    pub(crate) fn len(&self) -> usize {
        if self.wrapped { RING } else { self.pos }
    }

    /// Whether its oldest byte is stderr's first.
    fn holds_start(&self) -> bool {
        !self.wrapped
    }

    /// The byte just before its last `n` bytes, if it holds one.
    fn byte_before_last(&self, n: usize) -> Option<u8> {
        if n >= self.len() {
            return None;
        }
        Some(self.buf[(self.pos + RING - n - 1) % RING])
    }

    /// Replaces `out` with the last `n` bytes it holds, oldest first (`n` at most [`Ring::len`]).
    fn last(&self, n: usize, out: &mut Vec<u8>) {
        out.clear();
        let n = n.min(self.len());
        if n <= self.pos {
            out.extend_from_slice(&self.buf[self.pos - n..self.pos]);
        } else {
            out.extend_from_slice(&self.buf[RING - (n - self.pos)..]);
            out.extend_from_slice(&self.buf[..self.pos]);
        }
    }
}

/// The ring an agent's pump fills, shared with its handle and exit reports.
pub(crate) type SharedRing = Arc<Mutex<Ring>>;

/// Drains the agent's stderr into its ring, to the end of the pipe. The caller must drive it, on
/// its own executor or joined with other work: the crate never spawns, and an undrained stderr
/// pipe stalls the agent once the OS buffer (about 64 KiB) fills.
#[must_use = "an undrained stderr pipe stalls the agent"]
pub struct StderrPump {
    pipe: ChildStderr,
    ring: SharedRing,
}

impl std::fmt::Debug for StderrPump {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StderrPump").finish_non_exhaustive()
    }
}

impl StderrPump {
    pub(crate) fn new(pipe: ChildStderr, ring: SharedRing) -> StderrPump {
        StderrPump { pipe, ring }
    }

    /// Reads stderr to its end into the ring. Ends with the read error, if one comes first.
    pub async fn drain(self) -> io::Result<()> {
        self.pump(None::<fn(&str)>).await
    }

    /// As [`StderrPump::drain`], and calls `on_line` with each line, without its `\n` or a `\r`
    /// before it. A line that is not UTF-8 comes with U+FFFD in place of the bad bytes. A line
    /// longer than 16 KiB comes in 16 KiB pieces, and a last line with no `\n` comes at the end.
    /// Every byte reaches the ring before its line reaches `on_line`.
    pub async fn lines(self, on_line: impl FnMut(&str)) -> io::Result<()> {
        self.pump(Some(on_line)).await
    }

    async fn pump(mut self, mut on_line: Option<impl FnMut(&str)>) -> io::Result<()> {
        let mut buf = vec![0; READ];
        let mut lines = LineSplitter::default();
        loop {
            let n = self.pipe.read(&mut buf).await?;
            let Some(read) = buf.get(..n).filter(|read| !read.is_empty()) else {
                if let Some(on_line) = &mut on_line {
                    lines.finish(on_line);
                }
                return Ok(());
            };
            self.ring
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(read);
            if let Some(on_line) = &mut on_line {
                lines.feed(read, on_line);
            }
        }
    }
}

/// Splits reads into lines. A line that spans reads waits in `partial`, at most [`MAX_LINE`]
/// bytes; one within a read is passed straight from it.
#[derive(Default)]
struct LineSplitter {
    partial: Vec<u8>,
}

impl LineSplitter {
    fn feed(&mut self, read: &[u8], on_line: &mut impl FnMut(&str)) {
        let mut start = 0;
        for end in memchr::memchr_iter(b'\n', read) {
            let piece = &read[start..end];
            if self.partial.is_empty() {
                emit(piece, on_line);
            } else {
                self.push(piece, on_line);
                emit(&self.partial, on_line);
                self.partial.clear();
            }
            start = end + 1;
        }
        self.push(&read[start..], on_line);
    }

    /// Adds `bytes` to the waiting line. A full line is passed only when more bytes come, so a
    /// line of exactly [`MAX_LINE`] bytes still comes whole.
    fn push(&mut self, mut bytes: &[u8], on_line: &mut impl FnMut(&str)) {
        while !bytes.is_empty() {
            if self.partial.len() == MAX_LINE {
                emit(&self.partial, on_line);
                self.partial.clear();
            }
            let (fits, rest) = bytes.split_at(bytes.len().min(MAX_LINE - self.partial.len()));
            self.partial.extend_from_slice(fits);
            bytes = rest;
        }
    }

    /// Passes the last line, which had no `\n`.
    fn finish(&mut self, on_line: &mut impl FnMut(&str)) {
        if !self.partial.is_empty() {
            emit(&self.partial, on_line);
            self.partial.clear();
        }
    }
}

/// Passes `line` without a trailing `\r`, borrowed when it is UTF-8.
fn emit(line: &[u8], on_line: &mut impl FnMut(&str)) {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    match std::str::from_utf8(line) {
        Ok(line) => on_line(line),
        Err(_) => on_line(&String::from_utf8_lossy(line)),
    }
}

/// The end of the ring as text, at most [`TAIL`] bytes, redacted, opening on a whole line.
///
/// It redacts the last 16 KiB, opened on a whole line (after its first `\n`, unless the byte
/// before the window is a `\n` or the window holds stderr's start), then keeps the last [`TAIL`]
/// bytes the same way, cut at a char boundary. Redacting before cutting means no part of a secret
/// survives at the start. When the redacted text is shorter than [`TAIL`] while the ring holds
/// more, the window doubles. When opening on a whole line would leave nothing (one line longer
/// than the cut), the text starts mid-line instead of coming out empty.
pub(crate) fn tail(ring: &Mutex<Ring>, redactor: &Redactor) -> String {
    let mut window = WINDOW;
    let mut bytes = Vec::new();
    loop {
        // Only the copy holds the lock, so the pump never waits for the redaction.
        let (whole, on_line) = {
            let ring = ring.lock().unwrap_or_else(PoisonError::into_inner);
            ring.last(window, &mut bytes);
            let whole = bytes.len() == ring.len();
            let on_line =
                (whole && ring.holds_start()) || ring.byte_before_last(window) == Some(b'\n');
            (whole, on_line)
        };
        let text = String::from_utf8_lossy(&bytes);
        let text = open_on_line(&text, on_line);
        let redacted = redactor.redact(text);
        if redacted.len() >= TAIL || whole {
            return keep_last(&redacted, TAIL).to_owned();
        }
        window *= 2;
    }
}

/// `text` from its first whole line: all of it when `on_line` says it opens on one, else what
/// follows its first `\n`. When that leaves nothing (no `\n`, or only a last one), all of it.
fn open_on_line(text: &str, on_line: bool) -> &str {
    if on_line {
        return text;
    }
    match text.split_once('\n') {
        Some((_, rest)) if !rest.is_empty() => rest,
        _ => text,
    }
}

/// The last at most `max` bytes of `text`, starting on a char boundary and, when it cut
/// mid-line, after the first `\n` (see [`open_on_line`]).
fn keep_last(text: &str, max: usize) -> &str {
    let Some(mut start) = text.len().checked_sub(max).filter(|&start| start > 0) else {
        return text;
    };
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let on_line = text.as_bytes().get(start - 1) == Some(&b'\n');
    open_on_line(&text[start..], on_line)
}

#[cfg(test)]
mod tests;
