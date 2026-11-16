//! Line reading and the shared stdout. A line is read up to a cap, and a longer one is skipped to
//! its end and reported, so a flood never fills memory. Each line reaches stdout whole: both
//! threads write under one lock.

use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex, PoisonError};

/// The stdout both threads write to: one lock, whole lines.
pub(crate) type Out = Arc<Mutex<dyn Write + Send>>;

/// What [`read`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// The stream ended with nothing more to read.
    Eof,
    /// One line without its newline. A last line the stream ended without a newline is one too.
    Text(Vec<u8>),
    /// A line over the cap. Its bytes were skipped, so the next read starts on the next line.
    TooLong,
}

/// Reads one line from `reader`, keeping at most `max` bytes of it.
pub(crate) fn read<R: BufRead + ?Sized>(reader: &mut R, max: usize) -> io::Result<Line> {
    let mut text = Vec::new();
    let mut too_long = false;
    let mut read_any = false;
    loop {
        let buffered = reader.fill_buf()?;
        if buffered.is_empty() {
            return Ok(match (too_long, read_any) {
                (true, _) => Line::TooLong,
                (false, false) => Line::Eof,
                (false, true) => Line::Text(text),
            });
        }
        read_any = true;
        let (content, ended) = match buffered.iter().position(|&byte| byte == b'\n') {
            Some(index) => (&buffered[..index], true),
            None => (buffered, false),
        };
        if too_long || text.len() + content.len() > max {
            too_long = true;
            text = Vec::new();
        } else {
            text.extend_from_slice(content);
        }
        let consumed = content.len() + usize::from(ended);
        reader.consume(consumed);
        if ended {
            return Ok(if too_long {
                Line::TooLong
            } else {
                Line::Text(text)
            });
        }
    }
}

/// Writes `line` and a newline to `out` under its lock, so no other writer's bytes land inside it.
pub(crate) fn write_line(out: &Out, line: &[u8]) -> io::Result<()> {
    // A writer that panicked left whole lines behind; carry on.
    let mut stdout = out.lock().unwrap_or_else(PoisonError::into_inner);
    stdout.write_all(line)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

/// Whether `line` holds only ASCII whitespace. Such a line is not a message and is skipped.
pub(crate) fn is_blank(line: &[u8]) -> bool {
    line.iter().all(u8::is_ascii_whitespace)
}
