//! Splits the app-server's stdout into lines, each at most [`MAX_LINE_BYTES`].

use std::io;

use futures::io::{AsyncBufReadExt, AsyncRead, BufReader};

/// The longest line the app-server may send: 8 MiB, newline excluded, as `clauseo-acp` caps
/// an agent's lines.
pub(crate) const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// The stdout read buffer. A macOS pipe holds at most 64 KiB, so a larger one gains nothing.
const READ_BUFFER_BYTES: usize = 64 * 1024;

/// Why [`LineReader::next_line`] stopped.
#[derive(Debug)]
pub(crate) enum LineError {
    /// A line grew past [`MAX_LINE_BYTES`].
    TooLong,
    /// Reading the pipe failed.
    Io(io::Error),
}

/// Reads lines from a byte stream. [`Self::next_line`] keeps what it has read in `self`, so
/// dropping its future part way loses nothing.
pub(crate) struct LineReader<R> {
    reader: BufReader<R>,
    /// The start of a line that did not end in the bytes read so far.
    partial: Vec<u8>,
}

impl<R: AsyncRead + Unpin> LineReader<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader: BufReader::with_capacity(READ_BUFFER_BYTES, reader),
            partial: Vec::new(),
        }
    }

    /// The next line without its `\n`, or `None` at the end of the stream. A last line with no
    /// `\n` still counts.
    pub(crate) async fn next_line(&mut self) -> Result<Option<Vec<u8>>, LineError> {
        loop {
            let buffer = match self.reader.fill_buf().await {
                Ok(buffer) => buffer,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(LineError::Io(error)),
            };
            if buffer.is_empty() {
                return Ok((!self.partial.is_empty()).then(|| std::mem::take(&mut self.partial)));
            }
            let (chunk, consumed, ended) = match buffer.iter().position(|byte| *byte == b'\n') {
                Some(end) => (buffer.split_at(end).0, end + 1, true),
                None => (buffer, buffer.len(), false),
            };
            if self.partial.len() + chunk.len() > MAX_LINE_BYTES {
                return Err(LineError::TooLong);
            }
            self.partial.extend_from_slice(chunk);
            self.reader.consume_unpin(consumed);
            if ended {
                return Ok(Some(std::mem::take(&mut self.partial)));
            }
        }
    }
}
