//! The browser's side of the pipe: CDP messages as UTF-8 JSON, each ended by one NUL byte
//! (`devtools_pipe_handler.cc`: the browser splits what it reads at each `\0` and ends each
//! message it writes with one). Reads come in any sizes; a message may span many reads and one
//! read may hold many messages.

use std::io::{self, Read, Write};

use super::frames::MAX_MESSAGE;

/// How much one read asks for.
const CHUNK: usize = 64 * 1024;

/// Splits a byte stream at each NUL.
pub(crate) struct Reader<R> {
    input: R,
    max: usize,
    /// Bytes read past the last NUL.
    buffer: Vec<u8>,
    /// How much of `buffer` is known to hold no NUL.
    scanned: usize,
}

impl<R: Read> Reader<R> {
    pub(crate) fn new(input: R) -> Self {
        Self {
            input,
            max: MAX_MESSAGE,
            buffer: Vec::new(),
            scanned: 0,
        }
    }

    /// A reader whose messages may be at most `max` bytes, for tests.
    #[cfg(test)]
    pub(crate) fn with_max(input: R, max: usize) -> Self {
        Self {
            input,
            max,
            buffer: Vec::new(),
            scanned: 0,
        }
    }

    /// The next message without its NUL, or `None` once the stream ends (bytes after the last
    /// NUL are dropped: the browser never ends a message without one). A message over the limit
    /// is an error.
    pub(crate) fn next(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut chunk = vec![0_u8; CHUNK];
        loop {
            if let Some(at) = self
                .buffer
                .get(self.scanned..)
                .and_then(|rest| rest.iter().position(|byte| *byte == 0))
            {
                let end = self.scanned + at;
                let mut message: Vec<u8> = self.buffer.drain(..=end).collect();
                message.pop();
                self.scanned = 0;
                return Ok(Some(message));
            }
            self.scanned = self.buffer.len();
            if self.buffer.len() > self.max {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "a DevTools message from the browser is over the limit",
                ));
            }
            match self.input.read(&mut chunk) {
                Ok(0) => return Ok(None),
                Ok(read) => self
                    .buffer
                    .extend_from_slice(chunk.get(..read).unwrap_or_default()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

/// Writes `message` and its NUL in one buffer, so two writers under one lock never interleave.
pub(crate) fn write(output: &mut impl Write, message: &[u8]) -> io::Result<()> {
    let mut bytes = Vec::with_capacity(message.len() + 1);
    bytes.extend_from_slice(message);
    bytes.push(0);
    output.write_all(&bytes)?;
    output.flush()
}
